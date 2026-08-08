//! A terminal surface built directly on `alacritty_terminal`.
//!
//! GPUI ships no terminal widget, so everything below - PTY lifecycle, grid to
//! element translation, key encoding and IME plumbing - is POC code. Zed has an
//! equivalent module; it is not published as a reusable crate.

mod colors;
pub mod keys;
pub(crate) mod search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alacritty_terminal::event::{Event as AlacEvent, EventListener, Notify as _, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Hsla, IntoElement, KeyDownEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent,
    SharedString, Styled as _, Subscription, UTF16Selection, Window, canvas, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Sizable as _, h_flex, v_flex};

use crate::theme::{ActiveTokens as _, EditorZoom, Radius, Space, Type, UiZoom};

use search::{GridMatch, PlainLink, find_in_lines, plain_link_at};

pub use colors::TerminalPalette;

/// Terminal grid geometry. `alacritty_terminal` needs a `Dimensions` value for
/// every resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermSize {
    pub cols: usize,
    pub lines: usize,
}

impl TermSize {
    /// Clamped constructor, exposed for tests.
    #[cfg(test)]
    pub fn for_test(cols: usize, lines: usize) -> Self {
        Self::clamped(cols, lines)
    }

    fn clamped(cols: usize, lines: usize) -> Self {
        Self {
            cols: cols.max(2),
            lines: lines.max(1),
        }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Bridges `alacritty_terminal`'s event listener onto an async channel the GPUI
/// task can await, so an idle terminal costs no polling.
#[derive(Clone)]
struct Proxy(async_channel::Sender<AlacEvent>);

impl EventListener for Proxy {
    fn send_event(&self, event: AlacEvent) {
        let _ = self.0.try_send(event);
    }
}

/// One live shell: PTY, parser state and the writer handle.
pub struct TerminalSession {
    term: Arc<FairMutex<Term<Proxy>>>,
    notifier: Notifier,
    size: TermSize,
    pub title: SharedString,
    pub exited: bool,
    events: async_channel::Receiver<AlacEvent>,
    pub cwd: PathBuf,
    commands: Vec<CommandRecord>,
    pending_command: Option<Line>,
    tracked_history: usize,
}

#[derive(Clone, Copy)]
struct CommandRecord {
    start: Line,
    end: Line,
    status: i32,
}

impl TerminalSession {
    pub fn spawn(cwd: PathBuf, size: TermSize) -> anyhow::Result<Self> {
        let (tx, rx) = async_channel::unbounded();
        let proxy = Proxy(tx);

        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        install_shell_integration(&mut env)?;

        let pty_options = PtyOptions {
            shell: Some(Shell::new("/bin/zsh".into(), vec!["-l".into()])),
            working_directory: Some(cwd.clone()),
            drain_on_exit: true,
            env,
        };

        let window_size = WindowSize {
            num_lines: size.lines as u16,
            num_cols: size.cols as u16,
            cell_width: 8,
            cell_height: 16,
        };

        let pty = tty::new(&pty_options, window_size, 0)?;

        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        let term = Term::new(config, &size, proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();

        Ok(Self {
            term,
            notifier,
            size,
            title: "zsh".into(),
            exited: false,
            events: rx,
            cwd,
            commands: Vec::new(),
            pending_command: None,
            tracked_history: 0,
        })
    }

    pub fn write(&self, bytes: Vec<u8>) {
        self.notifier.notify(bytes);
    }

    pub fn resize(&mut self, size: TermSize) {
        if size == self.size {
            return;
        }
        self.size = size;
        let window_size = WindowSize {
            num_lines: size.lines as u16,
            num_cols: size.cols as u16,
            cell_width: 8,
            cell_height: 16,
        };
        let _ = self.notifier.0.send(Msg::Resize(window_size));
        let mut term = self.term.lock();
        resize_term_preserving_selection(&mut term, size);
        drop(term);
        self.sync_command_lines();
    }

    pub fn scroll(&self, delta_lines: i32) {
        use alacritty_terminal::grid::Scroll;
        self.term.lock().scroll_display(Scroll::Delta(delta_lines));
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    fn apply_shell_marker(&mut self, title: &str) -> bool {
        if let Some(cwd) = title.strip_prefix("__ARTIFEX__;C;") {
            self.sync_command_lines();
            self.cwd = PathBuf::from(cwd);
            self.pending_command = Some(self.term.lock().grid().cursor.point.line);
            return true;
        }
        let Some(payload) = title.strip_prefix("__ARTIFEX__;P;") else {
            return false;
        };
        let Some((status, cwd)) = payload.split_once(';') else {
            return true;
        };
        self.sync_command_lines();
        self.cwd = PathBuf::from(cwd);
        let end = self.term.lock().grid().cursor.point.line;
        if let Some(start) = self.pending_command.take() {
            self.commands.push(CommandRecord {
                start,
                end,
                status: status.parse().unwrap_or_default(),
            });
            if self.commands.len() > 1_000 {
                self.commands.remove(0);
            }
        }
        true
    }

    fn sync_command_lines(&mut self) {
        let history = self.term.lock().history_size();
        let delta = history as i32 - self.tracked_history as i32;
        if delta != 0 {
            for command in &mut self.commands {
                command.start -= delta;
                command.end -= delta;
            }
            if let Some(start) = self.pending_command.as_mut() {
                *start -= delta;
            }
        }
        self.tracked_history = history;
    }
}

struct SelectionSnapshot {
    ty: SelectionType,
    text: String,
    old_start: usize,
}

fn selection_snapshot<T>(term: &Term<T>) -> Option<SelectionSnapshot> {
    let selection = term.selection.as_ref()?;
    let range = selection.to_range(term)?;
    if range.is_block {
        return None;
    }
    let text = term.selection_to_string()?;
    let (_, points) = flattened_grid(term);
    let old_start = points.iter().position(|point| *point == range.start)?;
    Some(SelectionSnapshot {
        ty: selection.ty,
        text,
        old_start,
    })
}

fn restore_selection<T>(term: &mut Term<T>, snapshot: SelectionSnapshot) {
    if snapshot.text.is_empty() {
        return;
    }
    let (text, points) = flattened_grid(term);
    let mut best: Option<(usize, usize)> = None;
    for (byte, _) in text.match_indices(&snapshot.text) {
        let index = text
            .get(..byte)
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0);
        let distance = index.abs_diff(snapshot.old_start);
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((index, distance));
        }
    }
    let Some((start_index, _)) = best else {
        return;
    };
    let length = snapshot.text.chars().count();
    let Some(start) = points.get(start_index).copied() else {
        return;
    };
    let Some(end) = points.get(start_index + length.saturating_sub(1)).copied() else {
        return;
    };
    let mut selection = Selection::new(snapshot.ty, start, Side::Left);
    selection.update(end, Side::Right);
    term.selection = Some(selection);
}

fn flattened_grid<T>(term: &Term<T>) -> (String, Vec<GridPoint>) {
    let mut text = String::new();
    let mut points = Vec::new();
    for raw_line in term.topmost_line().0..=term.bottommost_line().0 {
        let line = Line(raw_line);
        let wrapped = term.grid()[line][term.last_column()]
            .flags
            .contains(Flags::WRAPLINE);
        let end = if wrapped {
            term.columns()
        } else {
            (0..term.columns())
                .rev()
                .find(|column| {
                    let cell = &term.grid()[line][Column(*column)];
                    cell.c != ' ' || !cell.flags.is_empty()
                })
                .map(|column| column + 1)
                .unwrap_or(0)
        };
        for column in 0..end {
            let cell = &term.grid()[line][Column(column)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(cell.c);
            points.push(GridPoint::new(line, Column(column)));
        }
        if !wrapped {
            text.push('\n');
            points.push(GridPoint::new(line, term.last_column()));
        }
    }
    (text, points)
}

pub(crate) fn resize_term_preserving_selection<T: EventListener>(
    term: &mut Term<T>,
    size: TermSize,
) {
    let selection = selection_snapshot(term);
    term.resize(size);
    if let Some(selection) = selection {
        restore_selection(term, selection);
    }
}

fn install_shell_integration(env: &mut HashMap<String, String>) -> anyhow::Result<()> {
    let user_zdotdir = std::env::var_os("ZDOTDIR")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let integration_dir =
        std::env::temp_dir().join(format!("artifex-zdotdir-{}", std::process::id()));
    std::fs::create_dir_all(&integration_dir)?;

    for name in [".zshenv", ".zprofile", ".zlogin", ".zlogout"] {
        let source = format!(
            concat!(
                "[[ -r \"$ARTIFEX_USER_ZDOTDIR/{}\" ]] && source ",
                "\"$ARTIFEX_USER_ZDOTDIR/{}\"\n",
                "export ZDOTDIR=\"$ARTIFEX_INTEGRATION_ZDOTDIR\"\n",
            ),
            name, name
        );
        std::fs::write(integration_dir.join(name), source)?;
    }
    std::fs::write(
        integration_dir.join(".zshrc"),
        concat!(
            "[[ -r \"$ARTIFEX_USER_ZDOTDIR/.zshrc\" ]] && source \"$ARTIFEX_USER_ZDOTDIR/.zshrc\"\n",
            "export ZDOTDIR=\"$ARTIFEX_INTEGRATION_ZDOTDIR\"\n",
            "autoload -Uz add-zsh-hook\n",
            "_artifex_preexec() { print -rn -- $'\\e]2;__ARTIFEX__;C;'\"$PWD\"$'\\a' }\n",
            "_artifex_precmd() { local artifex_status=$?; print -rn -- $'\\e]2;__ARTIFEX__;P;'\"$artifex_status\"';'\"$PWD\"$'\\a' }\n",
            "add-zsh-hook preexec _artifex_preexec\n",
            "add-zsh-hook precmd _artifex_precmd\n",
            "precmd_functions=(_artifex_precmd ${precmd_functions:#_artifex_precmd})\n",
        ),
    )?;

    env.insert(
        "ARTIFEX_USER_ZDOTDIR".to_string(),
        user_zdotdir.to_string_lossy().into_owned(),
    );
    env.insert(
        "ZDOTDIR".to_string(),
        integration_dir.to_string_lossy().into_owned(),
    );
    env.insert(
        "ARTIFEX_INTEGRATION_ZDOTDIR".to_string(),
        integration_dir.to_string_lossy().into_owned(),
    );
    Ok(())
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

/// One rendered row: contiguous runs sharing a style.
struct Run {
    text: String,
    fg: Hsla,
    bg: Option<Hsla>,
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Clone)]
pub enum TerminalEvent {
    OpenPath(PathBuf),
}

impl EventEmitter<TerminalEvent> for TerminalView {}

struct FindState {
    query: Entity<InputState>,
    matches: Vec<GridMatch>,
    active: usize,
    cached: String,
    _subscription: Subscription,
}

#[derive(Clone)]
struct DetectedLink {
    line: Line,
    start: usize,
    end: usize,
    target: LinkTarget,
}

#[derive(Clone)]
enum LinkTarget {
    Url(String),
    Path(PathBuf),
}

impl DetectedLink {
    fn contains(&self, point: GridPoint) -> bool {
        point.line == self.line && point.column.0 >= self.start && point.column.0 < self.end
    }
}

/// The GPUI view. Keeps the session alive, owns focus and the IME state.
pub struct TerminalView {
    pub session: TerminalSession,
    focus: FocusHandle,
    cell: gpui::Size<Pixels>,
    /// Text the IME is still composing. Committed text is written straight to
    /// the PTY, so only the in-flight run lives here.
    marked: Option<String>,
    active: bool,
    viewport: Option<Bounds<Pixels>>,
    selecting: bool,
    workspace_root: PathBuf,
    find: Option<FindState>,
    hovered_link: Option<DetectedLink>,
    command_nav: Option<usize>,
}

impl TerminalView {
    /// Spawns the shell first so a PTY failure is reportable, then builds the
    /// entity around the live session.
    pub fn open(cwd: PathBuf, window: &mut Window, cx: &mut App) -> anyhow::Result<Entity<Self>> {
        let cell = measure_cell(window, cx);
        let workspace_root = cwd.clone();
        let session = TerminalSession::spawn(cwd, TermSize::clamped(80, 24))?;
        let events = session.events.clone();

        Ok(cx.new(|cx| {
            cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
                while let Ok(event) = events.recv().await {
                    let keep = this
                        .update(cx, |this, cx| {
                            match event {
                                AlacEvent::Title(title) => {
                                    if !this.session.apply_shell_marker(&title) {
                                        this.session.title = title.into();
                                    }
                                }
                                AlacEvent::ResetTitle => this.session.title = "zsh".into(),
                                AlacEvent::ChildExit(_) => this.session.exited = true,
                                AlacEvent::Exit => this.session.exited = true,
                                AlacEvent::PtyWrite(text) => {
                                    this.session.write(text.into_bytes());
                                }
                                AlacEvent::ClipboardStore(_, text) => {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                                AlacEvent::ClipboardLoad(_, format) => {
                                    let text = cx
                                        .read_from_clipboard()
                                        .and_then(|item| item.text())
                                        .unwrap_or_default();
                                    this.session.write(format(&text).into_bytes());
                                }
                                AlacEvent::TextAreaSizeRequest(format) => {
                                    let size = this.session.size;
                                    let response = format(WindowSize {
                                        num_lines: size.lines as u16,
                                        num_cols: size.cols as u16,
                                        cell_width: f32::from(this.cell.width).round() as u16,
                                        cell_height: f32::from(this.cell.height).round() as u16,
                                    });
                                    this.session.write(response.into_bytes());
                                }
                                _ => {}
                            }
                            cx.notify();
                        })
                        .is_ok();
                    if !keep {
                        break;
                    }
                }
            })
            .detach();

            Self {
                session,
                focus: cx.focus_handle(),
                cell,
                marked: None,
                active: true,
                viewport: None,
                selecting: false,
                workspace_root,
                find: None,
                hovered_link: None,
                command_nav: None,
            }
        }))
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    fn resize_to(&mut self, bounds: Bounds<Pixels>) {
        let cols = (bounds.size.width / self.cell.width).floor() as usize;
        let lines = (bounds.size.height / self.cell.height).floor() as usize;
        self.session.resize(TermSize::clamped(cols, lines));
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if m.platform && !m.control && !m.alt && key == "f" {
            self.open_find(window, cx);
            return;
        }
        let find_focused = self
            .find
            .as_ref()
            .is_some_and(|find| find.query.read(cx).focus_handle(cx).is_focused(window));
        if find_focused {
            if key == "escape" {
                self.close_find(window, cx);
            }
            return;
        }
        if m.platform && !m.control && !m.alt && matches!(key, "up" | "down") {
            self.navigate_command(key == "up");
            cx.notify();
            return;
        }
        if m.platform && !m.control && !m.alt && key == "c" {
            self.copy_selection(cx);
            return;
        }
        if m.platform && !m.control && !m.alt && key == "a" {
            self.select_all();
            cx.notify();
            return;
        }
        // Cmd-V is a clipboard action, not a PTY escape: `keys::encode` returns
        // None for it, so paste has to be handled here before that path.
        if m.platform && !m.control && !m.alt && key == "v" {
            self.paste(cx);
            return;
        }
        let mode = self.session.mode();
        if let Some(bytes) = keys::encode(&event.keystroke, mode) {
            self.send_input(bytes);
            cx.notify();
        }
    }

    fn send_input(&mut self, bytes: Vec<u8>) {
        use alacritty_terminal::grid::Scroll;
        {
            let mut term = self.session.term.lock();
            term.selection = None;
            term.scroll_display(Scroll::Bottom);
        }
        self.command_nav = None;
        self.session.write(bytes);
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
            let subscription = cx.subscribe_in(
                &query,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        this.refresh_matches(cx);
                        cx.notify();
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        this.find_step(if *shift { -1 } else { 1 }, cx);
                    }
                    _ => {}
                },
            );
            self.find = Some(FindState {
                query,
                matches: Vec::new(),
                active: 0,
                cached: String::new(),
                _subscription: subscription,
            });
        }
        if let Some(find) = self.find.as_ref() {
            let handle = find.query.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn refresh_matches(&mut self, cx: &App) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.query.read(cx).value().to_string();
        if query == find.cached {
            return;
        }
        find.matches = find_in_lines(&terminal_lines(&self.session.term.lock()), &query);
        find.active = 0;
        find.cached = query;
        self.reveal_active_match();
    }

    fn find_step(&mut self, step: isize, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        find.active =
            (find.active as isize + step).rem_euclid(find.matches.len() as isize) as usize;
        self.reveal_active_match();
        cx.notify();
    }

    fn reveal_active_match(&mut self) {
        let Some(hit) = self
            .find
            .as_ref()
            .and_then(|find| find.matches.get(find.active))
            .copied()
        else {
            return;
        };
        self.session
            .term
            .lock()
            .scroll_to_point(GridPoint::new(Line(hit.line), Column(hit.start)));
    }

    fn navigate_command(&mut self, previous: bool) {
        let count = self.session.commands.len();
        if count == 0 {
            return;
        }
        let next = if previous {
            self.command_nav
                .map(|index| index.saturating_sub(1))
                .unwrap_or(count - 1)
        } else {
            match self.command_nav {
                Some(index) if index + 1 < count => index + 1,
                _ => {
                    use alacritty_terminal::grid::Scroll;
                    self.session.term.lock().scroll_display(Scroll::Bottom);
                    self.command_nav = None;
                    return;
                }
            }
        };
        self.command_nav = Some(next);
        if let Some(command) = self.session.commands.get(next) {
            let _ = (command.end, command.status);
            self.session
                .term
                .lock()
                .scroll_to_point(GridPoint::new(command.start, Column(0)));
        }
    }

    fn copy_selection(&self, cx: &mut Context<Self>) {
        let text = self.session.term.lock().selection_to_string();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn select_all(&mut self) {
        let mut term = self.session.term.lock();
        let start = GridPoint::new(term.topmost_line(), Column(0));
        let end = GridPoint::new(term.bottommost_line(), term.last_column());
        let mut selection = Selection::new(SelectionType::Lines, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    fn point_and_side(&self, position: gpui::Point<Pixels>) -> Option<(GridPoint, Side)> {
        let bounds = self.viewport?;
        let offset = self.session.term.lock().grid().display_offset();
        Some(grid_point(
            position,
            bounds,
            self.cell,
            self.session.size,
            offset,
        ))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left && event.modifiers.platform {
            if self.open_link_at(event.position, cx) {
                window.focus(&self.focus, cx);
                cx.notify();
                return;
            }
        }
        if !event.modifiers.alt
            && self.report_mouse(
                event.position,
                Some(event.button),
                false,
                false,
                event.modifiers,
            )
        {
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some((point, side)) = self.point_and_side(event.position) else {
            return;
        };
        let selection_type = if event.click_count >= 3 {
            SelectionType::Lines
        } else if event.click_count == 2 {
            SelectionType::Semantic
        } else if event.modifiers.alt {
            SelectionType::Block
        } else {
            SelectionType::Simple
        };
        self.session.term.lock().selection = Some(Selection::new(selection_type, point, side));
        self.selecting = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.modifiers.alt
            && self.report_mouse(
                event.position,
                event.pressed_button,
                false,
                true,
                event.modifiers,
            )
        {
            cx.notify();
            return;
        }
        let hovered = self
            .point_and_side(event.position)
            .and_then(|(point, _)| self.detect_link(point));
        let changed = match (&self.hovered_link, &hovered) {
            (Some(old), Some(new)) => {
                old.line != new.line || old.start != new.start || old.end != new.end
            }
            (None, None) => false,
            _ => true,
        };
        self.hovered_link = hovered;
        if changed {
            cx.notify();
        }
        if !self.selecting || !event.dragging() {
            return;
        }
        let Some((point, side)) = self.point_and_side(event.position) else {
            return;
        };
        let mut term = self.session.term.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
        drop(term);
        cx.notify();
    }

    fn open_link_at(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some((point, _)) = self.point_and_side(position) else {
            return false;
        };
        let Some(link) = self.detect_link(point) else {
            return false;
        };
        match link.target {
            LinkTarget::Url(url) => cx.open_url(&url),
            LinkTarget::Path(path) => cx.emit(TerminalEvent::OpenPath(path)),
        }
        true
    }

    fn detect_link(&self, point: GridPoint) -> Option<DetectedLink> {
        let term = self.session.term.lock();
        if point.line < term.topmost_line() || point.line > term.bottommost_line() {
            return None;
        }
        if let Some(hyperlink) = term.grid()[point].hyperlink() {
            let mut start = point.column.0;
            while start > 0
                && term.grid()[point.line][Column(start - 1)].hyperlink() == Some(hyperlink.clone())
            {
                start -= 1;
            }
            let mut end = point.column.0 + 1;
            while end < term.columns()
                && term.grid()[point.line][Column(end)].hyperlink() == Some(hyperlink.clone())
            {
                end += 1;
            }
            let target = self.resolve_link_target(hyperlink.uri())?;
            return Some(DetectedLink {
                line: point.line,
                start,
                end,
                target,
            });
        }

        let text = terminal_line(&term, point.line);
        let found = plain_link_at(&text, point.column.0)?;
        let target = match found.target {
            PlainLink::Url(url) => LinkTarget::Url(url),
            PlainLink::Path(path) => self.resolve_file_link(&path).map(LinkTarget::Path)?,
        };
        Some(DetectedLink {
            line: point.line,
            start: found.start,
            end: found.end,
            target,
        })
    }

    fn resolve_link_target(&self, target: &str) -> Option<LinkTarget> {
        if target.starts_with("https://") || target.starts_with("http://") {
            Some(LinkTarget::Url(target.to_string()))
        } else if let Some(path) = target.strip_prefix("file://") {
            self.resolve_file_link(path).map(LinkTarget::Path)
        } else {
            self.resolve_file_link(target).map(LinkTarget::Path)
        }
    }

    fn resolve_file_link(&self, value: &str) -> Option<PathBuf> {
        let value = strip_position_suffix(value);
        let expanded = if let Some(rest) = value.strip_prefix("~/") {
            std::env::var_os("HOME").map(PathBuf::from)?.join(rest)
        } else {
            let path = Path::new(value);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                self.session.cwd.join(path)
            }
        };
        let path = expanded.canonicalize().ok()?;
        let root = self.workspace_root.canonicalize().ok()?;
        (path.is_file() && path.starts_with(root)).then_some(path)
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.modifiers.alt
            && self.report_mouse(
                event.position,
                Some(event.button),
                true,
                false,
                event.modifiers,
            )
        {
            cx.notify();
            return;
        }
        self.selecting = false;
        let empty = self
            .session
            .term
            .lock()
            .selection
            .as_ref()
            .is_some_and(Selection::is_empty);
        if empty {
            self.session.term.lock().selection = None;
        }
        cx.notify();
    }

    fn report_mouse(
        &self,
        position: gpui::Point<Pixels>,
        button: Option<MouseButton>,
        release: bool,
        motion: bool,
        modifiers: Modifiers,
    ) -> bool {
        let Some((point, _)) = self.point_and_side(position) else {
            return false;
        };
        let term = self.session.term.lock();
        let mode = *term.mode();
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return false;
        }
        if motion
            && !(mode.contains(TermMode::MOUSE_MOTION)
                || (mode.contains(TermMode::MOUSE_DRAG) && button.is_some()))
        {
            return true;
        }
        let Some(button) = button.and_then(mouse_button_code) else {
            return true;
        };
        let row = point.line.0 + term.grid().display_offset() as i32;
        let bytes = mouse_report(
            mode,
            point.column.0,
            row.max(0) as usize,
            button,
            release,
            motion,
            modifiers,
        );
        drop(term);
        self.session.write(bytes);
        true
    }

    /// Writes the clipboard text to the PTY. Honours bracketed paste so a shell
    /// or editor that enabled it receives the run as inert data, not keystrokes.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let bracketed = self.session.mode().contains(TermMode::BRACKETED_PASTE);
        self.send_input(paste_payload(&text, bracketed).into_bytes());
        cx.notify();
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let lines = (event.delta.pixel_delta(self.cell.height).y / self.cell.height).round() as i32;
        if lines != 0 {
            let button = if lines > 0 { 64 } else { 65 };
            if self.report_mouse_code(event.position, button, event.modifiers) {
                cx.notify();
                return;
            }
            self.session.scroll(lines);
            cx.notify();
        }
    }

    fn report_mouse_code(
        &self,
        position: gpui::Point<Pixels>,
        button: u8,
        modifiers: Modifiers,
    ) -> bool {
        let Some((point, _)) = self.point_and_side(position) else {
            return false;
        };
        let term = self.session.term.lock();
        let mode = *term.mode();
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return false;
        }
        let row = point.line.0 + term.grid().display_offset() as i32;
        let bytes = mouse_report(
            mode,
            point.column.0,
            row.max(0) as usize,
            button,
            false,
            false,
            modifiers,
        );
        drop(term);
        self.session.write(bytes);
        true
    }

    /// Translates the visible grid into styled runs. One pass over the viewport
    /// only; scrollback above the viewport is never walked.
    fn rows(&self, cx: &App) -> (Vec<Vec<Run>>, Option<(usize, usize)>) {
        let palette = TerminalPalette::for_theme(cx.tokens().dark);
        let selection_bg = cx.tokens().c.selection;
        let selection_fg = cx.tokens().c.ink;
        let match_bg = cx.tokens().c.selection;
        let active_match_bg = cx.tokens().c.accent;
        let active_match_fg = cx.tokens().c.accent_ink;
        let term = self.session.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let lines = term.screen_lines();
        let mut rows: Vec<Vec<Run>> = (0..lines).map(|_| Vec::new()).collect();

        let cursor = content.cursor;
        let selection = content.selection;
        let cursor_row = cursor.point.line.0 + offset;
        let cursor_pos = (cursor_row >= 0 && (cursor_row as usize) < lines)
            .then(|| (cursor_row as usize, cursor.point.column.0));

        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + offset) as usize;
            let Some(target) = rows.get_mut(row) else {
                continue;
            };
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            let inverse = cell.flags.contains(Flags::INVERSE);
            let mut fg = palette.resolve(cell.fg, true);
            let mut bg = palette.resolve(cell.bg, false);
            if inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::DIM) {
                fg.a *= 0.6;
            }
            if cell.flags.contains(Flags::HIDDEN) {
                fg.a = 0.;
            }

            if let Some(find) = self.find.as_ref()
                && let Some((index, _)) = find.matches.iter().enumerate().find(|(_, found)| {
                    found.line == indexed.point.line.0
                        && indexed.point.column.0 >= found.start
                        && indexed.point.column.0 < found.end
                })
            {
                if index == find.active {
                    fg = active_match_fg;
                    bg = active_match_bg;
                } else {
                    bg = match_bg;
                }
            }

            if selection
                .is_some_and(|range| range.contains_cell(&indexed, cursor.point, cursor.shape))
            {
                fg = selection_fg;
                bg = selection_bg;
            }

            // Block caret: invert the cell so the glyph stays legible on the
            // solid cursor fill. The inactive caret is a dim block drawn behind
            // the text in `render` instead.
            if self.active && cursor_pos == Some((row, indexed.point.column.0)) {
                fg = palette.background;
                bg = palette.cursor;
            }

            let painted_bg = (bg != palette.background).then_some(bg);
            let bold = cell.flags.contains(Flags::BOLD);
            let italic = cell.flags.contains(Flags::ITALIC);
            let underline = cell.flags.intersects(Flags::ALL_UNDERLINES)
                || self
                    .hovered_link
                    .as_ref()
                    .is_some_and(|link| link.contains(indexed.point));

            match target.last_mut() {
                Some(run)
                    if run.fg == fg
                        && run.bg == painted_bg
                        && run.bold == bold
                        && run.italic == italic
                        && run.underline == underline =>
                {
                    run.text.push(cell.c);
                }
                _ => target.push(Run {
                    text: cell.c.to_string(),
                    fg,
                    bg: painted_bg,
                    bold,
                    italic,
                    underline,
                }),
            }
        }

        (rows, cursor_pos)
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_matches(cx);
        let c = cx.tokens().c;
        let palette = TerminalPalette::for_theme(cx.tokens().dark);
        let (rows, cursor) = self.rows(cx);
        let font_size = Type::EDITOR * cx.global::<EditorZoom>().0;
        let cell_h = self.cell.height;
        let cell_w = self.cell.width;
        let entity = cx.entity();
        let focus = self.focus.clone();
        let marked = self.marked.clone();

        div()
            .track_focus(&self.focus)
            .key_context("Terminal")
            .size_full()
            .bg(c.editor)
            .cursor(if self.hovered_link.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            })
            // DESIGN.md: inset the terminal by spaceM horizontally, spaceS
            // vertically, and fill the inset with the editor surface.
            .px(Space::M)
            .py(Space::S)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |_, bounds, window, cx| {
                        // Re-measure the cell each paint so a zoom change reflows
                        // the grid to the new glyph size before it is resized.
                        let cell = measure_cell(window, cx);
                        entity.update(cx, |this: &mut TerminalView, cx| {
                            this.cell = cell;
                            this.viewport = Some(bounds);
                            this.resize_to(bounds);
                            cx.notify();
                        });
                        window.handle_input(
                            &focus,
                            gpui::ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                v_flex()
                    .size_full()
                    .font_family("JetBrains Mono")
                    .text_size(font_size)
                    // GPUI text defaults to a golden-ratio line box (~1.62x),
                    // taller than the grid cell measured at 1.3x. Pin the line
                    // box to the cell so each row tiles the grid exactly:
                    // box-drawing runs connect and tall glyphs stop clipping.
                    .line_height(cell_h)
                    .children(rows.into_iter().enumerate().map(|(row_index, runs)| {
                        let cursor_col = cursor
                            .filter(|(row, _)| *row == row_index)
                            .map(|(_, col)| col);
                        h_flex()
                            .h(cell_h)
                            .relative()
                            // Paint the caret first so it sits behind the glyph.
                            // Appended after the runs it overpaints the character
                            // under the cursor and hides it.
                            .when_some(cursor_col.filter(|_| !self.active), |this, col| {
                                // Inactive caret only: a dim block behind the
                                // text. The active caret inverts its cell in
                                // `rows`, so drawing it here too would double it.
                                this.child(
                                    div()
                                        .absolute()
                                        .left(cell_w * col as f32)
                                        .top_0()
                                        .w(cell_w)
                                        .h(cell_h)
                                        .bg(palette.cursor)
                                        .opacity(0.25),
                                )
                            })
                            .children(runs.into_iter().map(|run| {
                                let mut el = div().text_color(run.fg).child(run.text);
                                if let Some(bg) = run.bg {
                                    el = el.bg(bg);
                                }
                                if run.bold {
                                    el = el.font_weight(gpui::FontWeight::BOLD);
                                }
                                if run.italic {
                                    el = el.italic();
                                }
                                if run.underline {
                                    el = el.underline();
                                }
                                el
                            }))
                    }))
                    .when_some(marked.zip(cursor), |this, (text, (row, col))| {
                        // Composition preview. The PTY sees nothing until the
                        // IME commits, so the marked run is drawn by us.
                        this.child(
                            div()
                                .absolute()
                                .top(cell_h * row as f32)
                                .left(cell_w * col as f32)
                                .px(Space::S)
                                .bg(c.selection)
                                .text_color(c.ink)
                                .underline()
                                .child(text),
                        )
                    }),
            )
            .when_some(self.render_find_bar(cx), |this, bar| this.child(bar))
    }
}

impl TerminalView {
    fn render_find_bar(&self, cx: &Context<Self>) -> Option<gpui::AnyElement> {
        let find = self.find.as_ref()?;
        let c = cx.tokens().c;
        let ui_zoom = cx.global::<UiZoom>().0;
        let counter = if find.cached.is_empty() {
            String::new()
        } else if find.matches.is_empty() {
            "0".to_string()
        } else {
            format!("{}/{}", find.active + 1, find.matches.len())
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .px(Space::S)
                .py(px(2.))
                .rounded(Radius::CONTROL)
                .border_1()
                .border_color(c.border)
                .text_size(Type::MICRO * ui_zoom)
                .child(label)
        };

        Some(
            h_flex()
                .absolute()
                .top(Space::S)
                .right(Space::M)
                .p(Space::S)
                .gap(Space::S)
                .items_center()
                .rounded(Radius::CONTROL)
                .border_1()
                .border_color(c.border)
                .bg(c.canvas)
                .shadow(crate::app::chrome::shadow_floating())
                .child(div().w(px(200.)).child(Input::new(&find.query).xsmall()))
                .child(
                    div()
                        .text_size(Type::MICRO * ui_zoom)
                        .text_color(c.ink_secondary)
                        .font_family("JetBrains Mono")
                        .child(SharedString::from(counter)),
                )
                .child(
                    button("terminal-find-prev", "<")
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(-1, cx))),
                )
                .child(
                    button("terminal-find-next", ">")
                        .on_click(cx.listener(|this, _, _, cx| this.find_step(1, cx))),
                )
                .child(
                    button("terminal-find-close", "x")
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }

    /// The characters on the cursor row up to the caret, plus any text the IME
    /// is still composing.
    ///
    /// An input method needs the text before the caret to decide what the next
    /// keystroke means. A terminal has no document, so this rebuilds one from
    /// the grid. Without it macOS commits every Vietnamese keystroke on its own
    /// and Telex never composes.
    fn document_before_caret(&self) -> String {
        let mut line = String::new();
        {
            let term = self.session.term.lock();
            let cursor = term.grid().cursor.point;
            let grid = term.grid();
            for column in 0..cursor.column.0 {
                let cell = &grid[cursor.line][Column(column)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                line.push(cell.c);
            }
        }
        let trimmed = line.trim_end().to_string();
        match &self.marked {
            Some(marked) => format!("{trimmed}{marked}"),
            None => trimmed,
        }
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let document: Vec<char> = self.document_before_caret().chars().collect();
        let start = range.start.min(document.len());
        let end = range.end.max(start).min(document.len());
        if start != range.start || end != range.end {
            *adjusted = Some(start..end);
        }
        Some(document[start..end].iter().collect())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = self.document_before_caret().chars().count();
        Some(UTF16Selection {
            range: len..len,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        let marked = self.marked.as_ref()?;
        let total = self.document_before_caret().chars().count();
        let len = marked.chars().count();
        Some(total.saturating_sub(len)..total)
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        if !text.is_empty() {
            self.send_input(text.as_bytes().to_vec());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        new_text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (row, col) = self.cursor_viewport_position()?;
        Some(Bounds {
            origin: gpui::point(
                element_bounds.origin.x + self.cell.width * col as f32,
                element_bounds.origin.y + self.cell.height * row as f32,
            ),
            size: gpui::size(self.cell.width, self.cell.height),
        })
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl TerminalView {
    fn cursor_viewport_position(&self) -> Option<(usize, usize)> {
        let term = self.session.term.lock();
        let content = term.renderable_content();
        let row = content.cursor.point.line.0 + content.display_offset as i32;
        (row >= 0 && (row as usize) < term.screen_lines())
            .then(|| (row as usize, content.cursor.point.column.0))
    }
}

/// Builds the bytes a paste writes to the PTY.
///
/// Under bracketed paste the run is wrapped in `\e[200~`..`\e[201~` so the shell
/// treats it as inert data; any embedded end marker is stripped first so a
/// crafted clipboard cannot break out and run commands. Without bracketed paste,
/// newlines collapse to carriage returns, matching what the shell reads from
/// typed input.
pub(crate) fn paste_payload(text: &str, bracketed: bool) -> String {
    if bracketed {
        let body = text.replace("\x1b[201~", "");
        format!("\x1b[200~{body}\x1b[201~")
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r")
    }
}

/// Measures one monospace cell so the grid maps onto pixels.
fn measure_cell(window: &mut Window, cx: &mut App) -> gpui::Size<Pixels> {
    let font = gpui::font("JetBrains Mono");
    let font_size = Type::EDITOR * cx.global::<EditorZoom>().0;
    let line_height = px((f32::from(font_size) * 1.3).round());
    let font_id = window.text_system().resolve_font(&font);
    let width = window
        .text_system()
        .ch_advance(font_id, font_size)
        .unwrap_or(font_size * 0.6);
    gpui::size(width, line_height)
}

fn terminal_lines(term: &Term<Proxy>) -> Vec<(i32, String)> {
    (term.topmost_line().0..=term.bottommost_line().0)
        .map(|line| (line, terminal_line(term, Line(line))))
        .collect()
}

fn terminal_line(term: &Term<Proxy>, line: Line) -> String {
    let mut text = String::new();
    for column in 0..term.columns() {
        let cell = &term.grid()[line][Column(column)];
        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            text.push(cell.c);
        }
    }
    text.trim_end().to_string()
}

fn strip_position_suffix(value: &str) -> &str {
    let Some((head, tail)) = value.rsplit_once(':') else {
        return value;
    };
    if tail.parse::<usize>().is_err() {
        return value;
    }
    let Some((path, line)) = head.rsplit_once(':') else {
        return head;
    };
    if line.parse::<usize>().is_ok() {
        path
    } else {
        head
    }
}

fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Navigate(_) => None,
    }
}

pub(crate) fn mouse_report(
    mode: TermMode,
    col: usize,
    row: usize,
    mut button: u8,
    release: bool,
    motion: bool,
    modifiers: Modifiers,
) -> Vec<u8> {
    button += 4 * u8::from(modifiers.shift);
    button += 8 * u8::from(modifiers.alt);
    button += 16 * u8::from(modifiers.control);
    button += 32 * u8::from(motion);
    let x = col.saturating_add(1);
    let y = row.saturating_add(1);

    if mode.contains(TermMode::SGR_MOUSE) {
        let terminator = if release { 'm' } else { 'M' };
        return format!("\x1b[<{button};{x};{y}{terminator}").into_bytes();
    }

    if release {
        button = 3;
    }
    if mode.contains(TermMode::UTF8_MOUSE) {
        let mut report = String::from("\x1b[M");
        for value in [button as u32 + 32, x as u32 + 32, y as u32 + 32] {
            if let Some(ch) = char::from_u32(value.min(0x7ff)) {
                report.push(ch);
            }
        }
        report.into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            button.saturating_add(32),
            (x.min(223) as u8).saturating_add(32),
            (y.min(223) as u8).saturating_add(32),
        ]
    }
}

fn grid_point(
    position: gpui::Point<Pixels>,
    bounds: Bounds<Pixels>,
    cell: gpui::Size<Pixels>,
    size: TermSize,
    display_offset: usize,
) -> (GridPoint, Side) {
    let x = ((position.x - bounds.origin.x) / cell.width).max(0.);
    let y = ((position.y - bounds.origin.y) / cell.height).max(0.);
    let col = (x.floor() as usize).min(size.cols.saturating_sub(1));
    let row = (y.floor() as usize).min(size.lines.saturating_sub(1));
    let cell_x = position.x - bounds.origin.x - cell.width * col as f32;
    let side = if cell_x < cell.width / 2. {
        Side::Left
    } else {
        Side::Right
    };
    (
        GridPoint::new(Line(row as i32 - display_offset as i32), Column(col)),
        side,
    )
}
