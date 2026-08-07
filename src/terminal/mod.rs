//! A terminal surface built directly on `alacritty_terminal`.
//!
//! GPUI ships no terminal widget, so everything below - PTY lifecycle, grid to
//! element translation, key encoding and IME plumbing - is POC code. Zed has an
//! equivalent module; it is not published as a reusable crate.

mod colors;
pub mod keys;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event as AlacEvent, EventListener, Notify as _, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as GridPoint};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, EntityInputHandler, FocusHandle, Focusable, Hsla, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Render, ScrollWheelEvent, SharedString, Styled as _,
    UTF16Selection, Window, canvas, div, px,
};
use gpui_component::{h_flex, v_flex};

use crate::theme::{ActiveTokens as _, EditorZoom, Space, Type};

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
}

impl TerminalSession {
    pub fn spawn(cwd: PathBuf, size: TermSize) -> anyhow::Result<Self> {
        let (tx, rx) = async_channel::unbounded();
        let proxy = Proxy(tx);

        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());

        let pty_options = PtyOptions {
            shell: Some(Shell::new("/bin/zsh".into(), vec!["-l".into()])),
            working_directory: Some(cwd),
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
        self.term.lock().resize(size);
    }

    pub fn scroll(&self, delta_lines: i32) {
        use alacritty_terminal::grid::Scroll;
        self.term.lock().scroll_display(Scroll::Delta(delta_lines));
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }
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

/// The GPUI view. Keeps the session alive, owns focus and the IME state.
pub struct TerminalView {
    pub session: TerminalSession,
    focus: FocusHandle,
    cell: gpui::Size<Pixels>,
    /// Text the IME is still composing. Committed text is written straight to
    /// the PTY, so only the in-flight run lives here.
    marked: Option<String>,
    active: bool,
}

impl TerminalView {
    /// Spawns the shell first so a PTY failure is reportable, then builds the
    /// entity around the live session.
    pub fn open(cwd: PathBuf, window: &mut Window, cx: &mut App) -> anyhow::Result<Entity<Self>> {
        let cell = measure_cell(window, cx);
        let session = TerminalSession::spawn(cwd, TermSize::clamped(80, 24))?;
        let events = session.events.clone();

        Ok(cx.new(|cx| {
            cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
                while let Ok(event) = events.recv().await {
                    let keep = this
                        .update(cx, |this, cx| {
                            match event {
                                AlacEvent::Title(title) => this.session.title = title.into(),
                                AlacEvent::ResetTitle => this.session.title = "zsh".into(),
                                AlacEvent::ChildExit(_) => this.session.exited = true,
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

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        // Cmd-V is a clipboard action, not a PTY escape: `keys::encode` returns
        // None for it, so paste has to be handled here before that path.
        if m.platform && !m.control && !m.alt && event.keystroke.key == "v" {
            self.paste(cx);
            return;
        }
        let mode = self.session.mode();
        if let Some(bytes) = keys::encode(&event.keystroke, mode) {
            self.session.write(bytes);
            cx.notify();
        }
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
        self.session.write(paste_payload(&text, bracketed).into_bytes());
        cx.notify();
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let lines = (event.delta.pixel_delta(self.cell.height).y / self.cell.height).round() as i32;
        if lines != 0 {
            self.session.scroll(lines);
            cx.notify();
        }
    }

    /// Translates the visible grid into styled runs. One pass over the viewport
    /// only; scrollback above the viewport is never walked.
    fn rows(&self, cx: &App) -> (Vec<Vec<Run>>, Option<(usize, usize)>) {
        let palette = TerminalPalette::for_theme(cx.tokens().dark);
        let term = self.session.term.lock();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let lines = term.screen_lines();
        let mut rows: Vec<Vec<Run>> = (0..lines).map(|_| Vec::new()).collect();

        let cursor = content.cursor;
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
            let underline = cell.flags.intersects(Flags::ALL_UNDERLINES);

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
            // DESIGN.md: inset the terminal by spaceM horizontally, spaceS
            // vertically, and fill the inset with the editor surface.
            .px(Space::M)
            .py(Space::S)
            .on_key_down(cx.listener(Self::on_key))
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
                    .when_some(marked, |this, text| {
                        // Composition preview. The PTY sees nothing until the
                        // IME commits, so the marked run is drawn by us.
                        this.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .px(Space::S)
                                .bg(c.selection)
                                .text_color(c.ink)
                                .underline()
                                .child(text),
                        )
                    }),
            )
    }
}

impl TerminalView {
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
            self.session.write(text.as_bytes().to_vec());
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
        Some(Bounds {
            origin: gpui::point(
                element_bounds.origin.x,
                element_bounds.origin.y + element_bounds.size.height - self.cell.height,
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

/// Grid point helper kept for the search/selection work that is out of scope in
/// this POC but referenced by the resize path.
#[allow(dead_code)]
fn grid_point(row: usize, col: usize) -> GridPoint {
    GridPoint::new(Line(row as i32), Column(col))
}
