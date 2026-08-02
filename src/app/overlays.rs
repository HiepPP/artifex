//! Quick Open, Command Palette and Search All Files.

use std::path::PathBuf;

use gpui::Focusable as _;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, IntoElement, ParentElement, SharedString, Styled as _,
    Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, Sizable as _, h_flex, v_flex};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

use crate::app::chrome::file_icon;
use crate::app::shell::{
    AddWorkspace, CancelOverlay, CommandPalette, NewTerminal, QuickOpen, SaveFile, SearchAllFiles,
    Shell, ToggleAppearance, ToggleInspector, TogglePreview, ToggleSidebar, ToggleSidebarTab,
};
use crate::services::search::{self, Batch, Cancel};
use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    QuickOpen,
    Palette,
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTerminal,
    ToggleSidebar,
    ToggleInspector,
    ToggleSidebarTab,
    TogglePreview,
    ToggleAppearance,
    Save,
    AddWorkspace,
    RefreshGit,
    Reindex,
}

impl Command {
    const ALL: [Command; 10] = [
        Command::NewTerminal,
        Command::ToggleSidebar,
        Command::ToggleInspector,
        Command::ToggleSidebarTab,
        Command::TogglePreview,
        Command::ToggleAppearance,
        Command::Save,
        Command::AddWorkspace,
        Command::RefreshGit,
        Command::Reindex,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::NewTerminal => "Terminal: New Terminal",
            Self::ToggleSidebar => "View: Toggle Sidebar",
            Self::ToggleInspector => "View: Toggle Inspector",
            Self::ToggleSidebarTab => "View: Toggle Explorer and Git",
            Self::TogglePreview => "View: Toggle Source and Preview",
            Self::ToggleAppearance => "View: Toggle Light and Dark",
            Self::Save => "File: Save",
            Self::AddWorkspace => "Workspace: Add Workspace",
            Self::RefreshGit => "Git: Refresh",
            Self::Reindex => "Workspace: Rebuild File Index",
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            Self::NewTerminal => "Cmd-T",
            Self::ToggleSidebar => "Cmd-Shift-R",
            Self::ToggleInspector => "Cmd-Shift-T",
            Self::ToggleSidebarTab => "Cmd-E",
            Self::TogglePreview => "Cmd-D",
            Self::Save => "Cmd-S",
            Self::AddWorkspace => "Cmd-0",
            _ => "",
        }
    }
}

#[derive(Default)]
pub struct OverlayState {
    pub kind: Option<Overlay>,
    pub query: Option<Entity<InputState>>,
    pub selected: usize,
    pub quick: Vec<(String, PathBuf)>,
    pub commands: Vec<Command>,
    pub batches: Vec<Batch>,
    pub total: usize,
    pub searching: bool,
    /// Bumped per search. A batch from an older generation is dropped, because
    /// cancellation only takes effect between files and the worker can still
    /// deliver one more batch after the query changed.
    generation: u64,
    cancel: Option<Cancel>,
    _sub: Option<Subscription>,
}

impl Shell {
    fn open_overlay(&mut self, kind: Overlay, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match kind {
            Overlay::QuickOpen => "Go to file by path",
            Overlay::Palette => "Run a command",
            Overlay::Search => "Search all files",
        };
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &query,
            window,
            |this, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.refresh_overlay(text, cx);
                }
                InputEvent::PressEnter { .. } => this.activate_overlay(window, cx),
                _ => {}
            },
        );

        let handle = query.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.overlay = OverlayState {
            kind: Some(kind),
            query: Some(query),
            selected: 0,
            quick: Vec::new(),
            commands: Command::ALL.to_vec(),
            batches: Vec::new(),
            total: 0,
            searching: false,
            generation: 0,
            cancel: None,
            _sub: Some(sub),
        };
        self.refresh_overlay(String::new(), cx);
        cx.notify();
    }

    pub(crate) fn on_quick_open(
        &mut self,
        _: &QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_overlay(Overlay::QuickOpen, window, cx);
    }

    pub(crate) fn on_command_palette(
        &mut self,
        _: &CommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_overlay(Overlay::Palette, window, cx);
    }

    pub(crate) fn on_search_all(
        &mut self,
        _: &SearchAllFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_overlay(Overlay::Search, window, cx);
    }

    pub(crate) fn on_cancel_overlay(
        &mut self,
        _: &CancelOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_overlay(window, cx);
    }

    /// Closing an overlay drops the query entity that owns focus. Without
    /// handing focus back to the shell the next `Cmd-Shift-F` has no path to a
    /// key binding and silently does nothing.
    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(cancel) = self.overlay.cancel.take() {
            cancel.cancel();
        }
        self.overlay = OverlayState::default();
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    fn refresh_overlay(&mut self, query: String, cx: &mut Context<Self>) {
        match self.overlay.kind {
            Some(Overlay::QuickOpen) => {
                let files = &self.workspace().index;
                let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
                let results: Vec<(String, PathBuf)> = if query.is_empty() {
                    files
                        .iter()
                        .take(60)
                        .map(|f| (f.relative.clone(), f.absolute.clone()))
                        .collect()
                } else {
                    let pattern =
                        Pattern::parse(&query, CaseMatching::Ignore, Normalization::Smart);
                    let mut scored: Vec<(u32, &crate::services::file_index::IndexedFile)> = files
                        .iter()
                        .filter_map(|f| {
                            let mut buf = Vec::new();
                            let haystack = nucleo_matcher::Utf32Str::new(&f.relative, &mut buf);
                            pattern
                                .score(haystack, &mut matcher)
                                .map(|score| (score, f))
                        })
                        .collect();
                    scored.sort_by(|a, b| b.0.cmp(&a.0));
                    scored
                        .into_iter()
                        .take(60)
                        .map(|(_, f)| (f.relative.clone(), f.absolute.clone()))
                        .collect()
                };
                self.overlay.quick = results;
                self.overlay.selected = 0;
            }
            Some(Overlay::Palette) => {
                let lowered = query.to_lowercase();
                self.overlay.commands = Command::ALL
                    .into_iter()
                    .filter(|command| {
                        lowered.is_empty() || command.title().to_lowercase().contains(&lowered)
                    })
                    .collect();
                self.overlay.selected = 0;
            }
            Some(Overlay::Search) => {
                if let Some(cancel) = self.overlay.cancel.take() {
                    cancel.cancel();
                }
                self.overlay.batches.clear();
                self.overlay.total = 0;
                if query.len() < 2 {
                    self.overlay.searching = false;
                    cx.notify();
                    return;
                }
                self.start_search(query, cx);
            }
            None => {}
        }
        cx.notify();
    }

    /// Runs the search off the main thread and streams ordered batches back.
    fn start_search(&mut self, query: String, cx: &mut Context<Self>) {
        let files = self.workspace().index.clone();
        let cancel = Cancel::new();
        self.overlay.cancel = Some(cancel.clone());
        self.overlay.searching = true;
        self.overlay.generation += 1;
        let generation = self.overlay.generation;

        let (tx, rx) = async_channel::unbounded::<Batch>();
        let worker_cancel = cancel.clone();
        cx.background_executor()
            .spawn(async move {
                search::run(&files, &query, false, false, worker_cancel, |batch| {
                    let _ = tx.send_blocking(batch);
                });
            })
            .detach();

        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            while let Ok(batch) = rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if this.overlay.generation != generation {
                        return false;
                    }
                    this.overlay.total += batch.hits.len();
                    this.overlay.batches.push(batch);
                    cx.notify();
                    true
                });
                match alive {
                    Ok(true) => {}
                    _ => break,
                }
            }
            let _ = this.update(cx, |this, cx| {
                if this.overlay.generation == generation {
                    this.overlay.searching = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn activate_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.overlay.selected;
        match self.overlay.kind {
            Some(Overlay::QuickOpen) => {
                if let Some((_, path)) = self.overlay.quick.get(selected).cloned() {
                    self.workspace_mut().open_file(path, false, cx);
                    self.close_overlay(window, cx);
                }
            }
            Some(Overlay::Palette) => {
                if let Some(command) = self.overlay.commands.get(selected).copied() {
                    self.close_overlay(window, cx);
                    self.run_command(command, window, cx);
                }
            }
            Some(Overlay::Search) => {
                let hit = self
                    .overlay
                    .batches
                    .iter()
                    .flat_map(|batch| batch.hits.iter())
                    .nth(selected)
                    .cloned();
                if let Some(hit) = hit {
                    self.workspace_mut()
                        .open_file(hit.absolute.clone(), false, cx);
                    if let Some(tab) = self.workspace().selected_tab()
                        && let crate::app::workspace::TabKind::File { editor, .. } = &tab.kind
                    {
                        let editor = editor.clone();
                        let line = hit.line.saturating_sub(1) as usize;
                        editor.update(cx, |editor, _| editor.reveal_line(line));
                    }
                    self.close_overlay(window, cx);
                }
            }
            None => {}
        }
        cx.notify();
    }

    fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        // Every palette entry runs the same handler the key binding runs.
        // Dispatching an action here would go through the focused element,
        // which the closing overlay has just released.
        match command {
            Command::NewTerminal => self.on_new_terminal(&NewTerminal, window, cx),
            Command::ToggleSidebar => self.on_toggle_sidebar(&ToggleSidebar, window, cx),
            Command::ToggleInspector => self.on_toggle_inspector(&ToggleInspector, window, cx),
            Command::ToggleSidebarTab => self.on_toggle_sidebar_tab(&ToggleSidebarTab, window, cx),
            Command::TogglePreview => self.on_toggle_preview(&TogglePreview, window, cx),
            Command::ToggleAppearance => self.on_toggle_appearance(&ToggleAppearance, window, cx),
            Command::Save => self.on_save(&SaveFile, window, cx),
            Command::AddWorkspace => self.on_add_workspace(&AddWorkspace, window, cx),
            Command::RefreshGit => {
                self.workspace_mut().refresh_git();
                self.set_status("git refreshed");
            }
            Command::Reindex => {
                self.workspace_mut().reindex();
                self.set_status("index rebuilt");
            }
        }
        cx.notify();
    }

    pub(crate) fn render_overlay(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let kind = self.overlay.kind?;
        let c = cx.tokens().c;
        let query = self.overlay.query.clone()?;
        let wide = kind == Overlay::Search;

        let body: AnyElement = match kind {
            Overlay::QuickOpen => self.render_quick_results(cx).into_any_element(),
            Overlay::Palette => self.render_command_results(cx).into_any_element(),
            Overlay::Search => self.render_search_results(cx).into_any_element(),
        };

        let footer = match kind {
            Overlay::Search => format!(
                "{} matches{}",
                self.overlay.total,
                if self.overlay.searching {
                    " - searching"
                } else {
                    ""
                }
            ),
            _ => "Up / Down / Return / click outside to close".to_string(),
        };

        Some(
            div()
                // `DESIGN.md` treats the palette as a blocking overlay, so it
                // owns a key context of its own. Escape is bound for `Input`
                // inside it, which outranks the component kit's own binding and
                // is the only way the shell hears the key while the query field
                // holds focus.
                .id("overlay-scrim")
                .key_context("AtelierOverlay")
                // DESIGN.md: the scrim dismisses on click. It is also the only
                // reliable way out on macOS, because the input method eats a
                // plain Escape before GPUI ever sees it. See FEASIBILITY.md.
                .on_click(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                // DESIGN.md: 0.12 black scrim behind a blocking overlay.
                .bg(gpui::hsla(0., 0., 0., 0.12))
                .flex()
                .justify_center()
                .when(!wide, |this| this.pt(px(80.)))
                .when(wide, |this| this.p(Space::L))
                .child(
                    v_flex()
                        .id("overlay-panel")
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .w_full()
                        .max_w(if wide {
                            px(1100.)
                        } else {
                            Metrics::PALETTE_WIDTH
                        })
                        .when(!wide, |this| this.h(Metrics::PALETTE_HEIGHT))
                        .when(wide, |this| this.h_full())
                        .rounded(Radius::PANEL)
                        .overflow_hidden()
                        .border_1()
                        .border_color(c.border)
                        .bg(c.raised)
                        .child(
                            div()
                                .h(Metrics::PALETTE_FIELD)
                                .w_full()
                                .flex()
                                .items_center()
                                .px(Space::M)
                                .bg(c.editor)
                                .border_b_1()
                                .border_color(c.border)
                                .child(Input::new(&query).appearance(false)),
                        )
                        .child(
                            div()
                                .id("overlay-body")
                                .flex_1()
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(
                            div()
                                .h(Metrics::STATUS_BAR)
                                .w_full()
                                .flex()
                                .items_center()
                                .px(Space::M)
                                .bg(c.chrome)
                                .border_t_1()
                                .border_color(c.border)
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .child(SharedString::from(footer)),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_quick_results(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let selected = self.overlay.selected;
        let rows: Vec<(usize, String, String)> = self
            .overlay
            .quick
            .iter()
            .enumerate()
            .map(|(index, (relative, path))| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                (index, name, relative.clone())
            })
            .collect();

        v_flex().children(rows.into_iter().map(|(index, name, relative)| {
            let (icon, tint) = file_icon(&name, c);
            h_flex()
                .id(("quick", index))
                .cursor_pointer()
                .items_center()
                .gap(Space::S)
                .px(Space::M)
                .py(Space::XS)
                .when(index == selected, |this| this.bg(c.selection))
                .hover(|this| this.bg(c.hover))
                .child(Icon::new(icon).xsmall().text_color(tint))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .child(
                            div()
                                .truncate()
                                .text_size(Type::BODY)
                                .child(SharedString::from(name)),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_family("JetBrains Mono")
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .child(SharedString::from(relative)),
                        ),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.overlay.selected = index;
                    this.activate_overlay(window, cx);
                }))
        }))
    }

    fn render_command_results(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let selected = self.overlay.selected;
        let commands = self.overlay.commands.clone();

        v_flex().children(commands.into_iter().enumerate().map(|(index, command)| {
            h_flex()
                .id(("command", index))
                .cursor_pointer()
                .h(Metrics::ROW)
                .items_center()
                .px(Space::M)
                .when(index == selected, |this| this.bg(c.selection))
                .hover(|this| this.bg(c.hover))
                .child(div().flex_1().text_size(Type::BODY).child(command.title()))
                .child(
                    div()
                        .font_family("JetBrains Mono")
                        .text_size(Type::MICRO)
                        .text_color(c.ink_secondary)
                        .child(command.shortcut()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.overlay.selected = index;
                    this.activate_overlay(window, cx);
                }))
        }))
    }

    fn render_search_results(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let mut flat = 0usize;
        let batches = self.overlay.batches.clone();

        v_flex().children(batches.into_iter().map(|batch| {
            let header = h_flex()
                .h(Metrics::ROW)
                .items_center()
                .px(Space::M)
                .gap(Space::S)
                .bg(c.panel)
                .child(
                    div()
                        .font_family("JetBrains Mono")
                        .text_size(Type::CAPTION)
                        .child(SharedString::from(batch.relative.clone())),
                )
                .child(
                    div()
                        .text_size(Type::MICRO)
                        .text_color(c.ink_secondary)
                        .child(SharedString::from(batch.hits.len().to_string())),
                );

            let rows: Vec<_> = batch
                .hits
                .into_iter()
                .map(|hit| {
                    let index = flat;
                    flat += 1;
                    h_flex()
                        .id(("hit", index))
                        .cursor_pointer()
                        .px(Space::M)
                        .py(px(2.))
                        .gap(Space::S)
                        .hover(|this| this.bg(c.hover))
                        .child(
                            div()
                                .w(px(48.))
                                .flex_none()
                                .text_right()
                                .font_family("JetBrains Mono")
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .child(SharedString::from(hit.line.to_string())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .font_family("JetBrains Mono")
                                .text_size(Type::CAPTION)
                                .child(SharedString::from(hit.text.clone())),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.overlay.selected = index;
                            this.activate_overlay(window, cx);
                        }))
                })
                .collect();

            v_flex().child(header).children(rows)
        }))
    }
}
