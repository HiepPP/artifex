//! Source editor.
//!
//! GPUI has no editor element, so this is POC code: a line buffer, a single
//! cursor, IME plumbing and a virtualised row list. Highlighting is resolved
//! for the rows the list actually asks for, never for the whole file.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, EntityInputHandler, FocusHandle, Focusable, HighlightStyle,
    IntoElement, KeyDownEvent, ParentElement, Pixels, Render, SharedString, Styled as _,
    StyledText, UTF16Selection, UniformListScrollHandle, Window, canvas, div, px, uniform_list,
};
use gpui_component::input::{Input, InputState};
use gpui_component::{Sizable as _, h_flex, v_flex};

use crate::services::highlight::{Highlighter, Lang, line_starts};
use crate::services::search::{LineMatch, find_in_lines};
use crate::theme::{ActiveTokens as _, Radius, Space, Type};

const GUTTER_WIDTH: Pixels = px(56.);

pub struct EditorView {
    pub path: PathBuf,
    pub dirty: bool,
    pub wrap: bool,
    lines: Vec<String>,
    highlighter: Highlighter,
    line_starts: Vec<usize>,
    source: String,
    cursor_row: usize,
    cursor_byte: usize,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    marked: Option<String>,
    find: Option<FindState>,
    /// Rows the list asked for on the last frame. Reported by the POC so the
    /// viewport claim is measurable rather than asserted.
    pub last_visible: std::cell::Cell<(usize, usize)>,
}

/// Find bar state. Created on first `Cmd-F`, dropped on close.
struct FindState {
    query: Entity<InputState>,
    replace: Entity<InputState>,
    show_replace: bool,
    matches: Vec<LineMatch>,
    active: usize,
    /// Query text the matches were computed for. Render recomputes when the
    /// input no longer agrees.
    cached: String,
}

impl EditorView {
    pub fn open(path: PathBuf, cx: &mut App) -> Entity<Self> {
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        let lang = Lang::for_path(&path);
        let mut highlighter = Highlighter::new(lang);
        highlighter.parse(&source);
        let lines = source.split('\n').map(|l| l.to_string()).collect();
        let starts = line_starts(&source);

        cx.new(|cx| Self {
            path,
            dirty: false,
            wrap: false,
            lines,
            highlighter,
            line_starts: starts,
            source,
            cursor_row: 0,
            cursor_byte: 0,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            marked: None,
            find: None,
            last_visible: std::cell::Cell::new((0, 0)),
        })
    }

    /// The 1-based line the cursor sits on, for `path:line` references.
    pub fn cursor_line(&self) -> usize {
        self.cursor_row + 1
    }

    /// Opens the find bar, or focuses it when already open. `with_replace`
    /// also shows the replace row.
    pub fn open_find(&mut self, with_replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
            let replace = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));
            self.find = Some(FindState {
                query,
                replace,
                show_replace: false,
                matches: Vec::new(),
                active: 0,
                cached: String::new(),
            });
        }
        if let Some(find) = self.find.as_mut() {
            find.show_replace = find.show_replace || with_replace;
            let handle = find.query.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    pub fn find_open(&self) -> bool {
        self.find.is_some()
    }

    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Moves the active match by `step` (1 next, -1 previous), wrapping.
    pub fn find_step(&mut self, step: isize, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let count = find.matches.len();
        if count == 0 {
            return;
        }
        find.active = (find.active as isize + step).rem_euclid(count as isize) as usize;
        let hit = find.matches[find.active];
        self.reveal_match(hit);
        cx.notify();
    }

    fn reveal_match(&mut self, hit: LineMatch) {
        self.cursor_row = hit.row.min(self.lines.len().saturating_sub(1));
        self.cursor_byte = hit.start;
        self.scroll
            .scroll_to_item(self.cursor_row, gpui::ScrollStrategy::Center);
    }

    /// Replaces the active match, or every match with `all`.
    pub fn replace_active(&mut self, all: bool, cx: &mut Context<Self>) {
        self.refresh_matches(cx);
        let Some(find) = self.find.as_ref() else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        let replacement = find.replace.read(cx).value().to_string();
        let targets: Vec<LineMatch> = if all {
            find.matches.clone()
        } else {
            vec![find.matches[find.active]]
        };
        // Back to front so earlier ranges stay valid while later ones change.
        for hit in targets.iter().rev() {
            let Some(line) = self.lines.get_mut(hit.row) else {
                continue;
            };
            if hit.end <= line.len() {
                line.replace_range(hit.start..hit.end, &replacement);
            }
        }
        self.reindex();
        if let Some(find) = self.find.as_mut() {
            // Force a recompute against the edited buffer.
            find.cached = String::new();
        }
        self.refresh_matches(cx);
        cx.notify();
    }

    /// Recomputes matches when the query text changed since the last pass.
    fn refresh_matches(&mut self, cx: &App) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.query.read(cx).value().to_string();
        if query == find.cached {
            return;
        }
        find.matches = find_in_lines(&self.lines, &query);
        find.active = 0;
        find.cached = query;
        if let Some(hit) = find.matches.first().copied() {
            self.reveal_match(hit);
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn byte_len(&self) -> usize {
        self.source.len()
    }

    pub fn language(&self) -> Lang {
        self.highlighter.lang()
    }

    pub fn reveal_line(&mut self, line: usize) {
        self.cursor_row = line.min(self.lines.len().saturating_sub(1));
        self.cursor_byte = 0;
        self.scroll
            .scroll_to_item(self.cursor_row.saturating_sub(4), gpui::ScrollStrategy::Top);
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        std::fs::write(&self.path, &self.source)?;
        self.dirty = false;
        Ok(())
    }

    fn reindex(&mut self) {
        self.source = self.lines.join("\n");
        self.line_starts = line_starts(&self.source);
        self.highlighter.parse(&self.source);
        self.dirty = true;
    }

    fn insert(&mut self, text: &str) {
        if text.contains('\n') {
            let mut parts = text.split('\n');
            if let Some(first) = parts.next() {
                self.insert_flat(first);
            }
            for part in parts {
                self.newline();
                self.insert_flat(part);
            }
            return;
        }
        self.insert_flat(text);
    }

    fn insert_flat(&mut self, text: &str) {
        let line = &mut self.lines[self.cursor_row];
        let at = self.cursor_byte.min(line.len());
        line.insert_str(at, text);
        self.cursor_byte = at + text.len();
        self.reindex();
    }

    fn newline(&mut self) {
        let line = self.lines[self.cursor_row].clone();
        let at = self.cursor_byte.min(line.len());
        let (head, tail) = line.split_at(at);
        self.lines[self.cursor_row] = head.to_string();
        self.lines.insert(self.cursor_row + 1, tail.to_string());
        self.cursor_row += 1;
        self.cursor_byte = 0;
        self.reindex();
    }

    fn backspace(&mut self) {
        if self.cursor_byte > 0 {
            let line = &mut self.lines[self.cursor_row];
            let mut start = self.cursor_byte - 1;
            while start > 0 && !line.is_char_boundary(start) {
                start -= 1;
            }
            line.replace_range(start..self.cursor_byte, "");
            self.cursor_byte = start;
        } else if self.cursor_row > 0 {
            let tail = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_byte = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&tail);
        } else {
            return;
        }
        self.reindex();
    }

    fn move_cursor(&mut self, key: &str) {
        match key {
            "left" => {
                if self.cursor_byte > 0 {
                    let line = &self.lines[self.cursor_row];
                    let mut at = self.cursor_byte - 1;
                    while at > 0 && !line.is_char_boundary(at) {
                        at -= 1;
                    }
                    self.cursor_byte = at;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_byte = self.lines[self.cursor_row].len();
                }
            }
            "right" => {
                let line = &self.lines[self.cursor_row];
                if self.cursor_byte < line.len() {
                    let mut at = self.cursor_byte + 1;
                    while at < line.len() && !line.is_char_boundary(at) {
                        at += 1;
                    }
                    self.cursor_byte = at;
                } else if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_byte = 0;
                }
            }
            "up" => {
                self.cursor_row = self.cursor_row.saturating_sub(1);
                self.cursor_byte = self.cursor_byte.min(self.lines[self.cursor_row].len());
            }
            "down" => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_byte = self.cursor_byte.min(self.lines[self.cursor_row].len());
                }
            }
            "home" => self.cursor_byte = 0,
            "end" => self.cursor_byte = self.lines[self.cursor_row].len(),
            _ => {}
        }
        self.scroll
            .scroll_to_item(self.cursor_row, gpui::ScrollStrategy::Center);
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        if m.platform {
            return;
        }
        match event.keystroke.key.as_str() {
            "backspace" => self.backspace(),
            "enter" => self.newline(),
            "tab" => self.insert("    "),
            key @ ("left" | "right" | "up" | "down" | "home" | "end") => self.move_cursor(key),
            _ => {
                // Plain characters arrive through the IME path, not here, so
                // nothing else is handled: see `replace_text_in_range`.
                return;
            }
        }
        cx.notify();
    }

    /// Renders exactly the rows the uniform list asked for. Highlight queries
    /// are bounded to those rows' byte range.
    fn render_rows(&self, range: std::ops::Range<usize>, cx: &App) -> Vec<gpui::AnyElement> {
        let c = cx.tokens().c;
        self.last_visible.set((range.start, range.end));

        let start_byte = self.line_starts.get(range.start).copied().unwrap_or(0);
        let end_byte = self
            .line_starts
            .get(range.end)
            .copied()
            .unwrap_or(self.source.len());
        let spans =
            self.highlighter
                .spans_in(&self.source, start_byte..end_byte, &self.line_starts, &c);

        range
            .map(|row| {
                let text = self.lines.get(row).cloned().unwrap_or_default();
                let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = spans
                    .get(&row)
                    .map(|list| {
                        list.iter()
                            .filter(|s| s.end <= text.len() && s.start < s.end)
                            .map(|s| {
                                (
                                    s.start..s.end,
                                    HighlightStyle {
                                        color: Some(s.color),
                                        ..Default::default()
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let highlights = self.overlay_find_matches(row, highlights, text.len(), cx);
                let is_cursor_row = row == self.cursor_row;
                h_flex()
                    .w_full()
                    .when(is_cursor_row, |this| this.bg(c.selection.opacity(0.35)))
                    .child(
                        div()
                            .w(GUTTER_WIDTH)
                            .flex_none()
                            .pr(Space::M)
                            .text_right()
                            .text_color(c.ink_secondary.opacity(0.7))
                            .child(SharedString::from((row + 1).to_string())),
                    )
                    .child(div().flex_1().child(
                        StyledText::new(SharedString::from(text)).with_highlights(highlights),
                    ))
                    .into_any_element()
            })
            .collect()
    }

    /// Lays the find matches over the syntax highlights for one row. Ranges
    /// handed to `StyledText` must not overlap, so syntax spans are clipped
    /// around every match before the match styles are added.
    fn overlay_find_matches(
        &self,
        row: usize,
        syntax: Vec<(std::ops::Range<usize>, HighlightStyle)>,
        line_len: usize,
        cx: &App,
    ) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
        let Some(find) = self.find.as_ref() else {
            return syntax;
        };
        let hits: Vec<(std::ops::Range<usize>, bool)> = find
            .matches
            .iter()
            .enumerate()
            .filter(|(_, hit)| hit.row == row && hit.end <= line_len)
            .map(|(index, hit)| (hit.start..hit.end, index == find.active))
            .collect();
        if hits.is_empty() {
            return syntax;
        }

        let c = cx.tokens().c;
        let mut merged: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
        for (range, style) in syntax {
            let mut cursor = range.start;
            for (hit, _) in &hits {
                if hit.end <= cursor || hit.start >= range.end {
                    continue;
                }
                if hit.start > cursor {
                    merged.push((cursor..hit.start, style));
                }
                cursor = hit.end.min(range.end);
            }
            if cursor < range.end {
                merged.push((cursor..range.end, style));
            }
        }
        for (hit, active) in hits {
            merged.push((
                hit,
                HighlightStyle {
                    color: active.then_some(c.accent_ink),
                    background_color: Some(if active { c.accent } else { c.selection }),
                    ..Default::default()
                },
            ));
        }
        merged.sort_by_key(|(range, _)| range.start);
        merged
    }

    fn render_find_bar(&self, cx: &Context<Self>) -> Option<gpui::AnyElement> {
        let find = self.find.as_ref()?;
        let c = cx.tokens().c;
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
                .text_size(Type::MICRO)
                .child(label)
        };

        Some(
            v_flex()
                .absolute()
                .top(Space::S)
                .right(Space::M)
                .p(Space::S)
                .gap(Space::S)
                .rounded(Radius::CONTROL)
                .border_1()
                .border_color(c.border)
                .bg(c.canvas)
                .shadow(crate::app::chrome::shadow_floating())
                .child(
                    h_flex()
                        .gap(Space::S)
                        .items_center()
                        .child(div().w(px(200.)).child(Input::new(&find.query).xsmall()))
                        .child(
                            div()
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .font_family("JetBrains Mono")
                                .child(SharedString::from(counter)),
                        )
                        .child(button("find-prev", "<").on_click(cx.listener(
                            |this, _, _, cx| this.find_step(-1, cx),
                        )))
                        .child(button("find-next", ">").on_click(cx.listener(
                            |this, _, _, cx| this.find_step(1, cx),
                        )))
                        .child(button("find-close", "x").on_click(cx.listener(
                            |this, _, window, cx| this.close_find(window, cx),
                        ))),
                )
                .when(find.show_replace, |this| {
                    this.child(
                        h_flex()
                            .gap(Space::S)
                            .items_center()
                            .child(div().w(px(200.)).child(Input::new(&find.replace).xsmall()))
                            .child(button("replace-one", "Replace").on_click(cx.listener(
                                |this, _, _, cx| this.replace_active(false, cx),
                            )))
                            .child(button("replace-all", "All").on_click(cx.listener(
                                |this, _, _, cx| this.replace_active(true, cx),
                            ))),
                    )
                })
                .into_any_element(),
        )
    }
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_matches(cx);
        let c = cx.tokens().c;
        let entity = cx.entity();
        let count = self.lines.len();
        let focus = self.focus.clone();
        let list_entity = entity.clone();

        div()
            .track_focus(&self.focus)
            .key_context("Editor")
            .size_full()
            .bg(c.editor)
            .text_color(c.ink)
            .font_family("JetBrains Mono")
            .text_size(Type::EDITOR)
            .on_key_down(cx.listener(Self::on_key))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |_, bounds, window, cx| {
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
                uniform_list("editor-rows", count, move |range, _window, cx| {
                    list_entity.read(cx).render_rows(range, cx)
                })
                .track_scroll(&self.scroll)
                .size_full()
                .p(Space::S),
            )
            .when_some(self.render_find_bar(cx), |this, bar| this.child(bar))
            .when_some(self.marked.clone(), |this, text| {
                this.child(
                    v_flex()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .p(Space::S)
                        .child(div().px(Space::S).bg(c.selection).underline().child(text)),
                )
            })
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = self.marked.as_ref().map(|m| m.chars().count()).unwrap_or(0);
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
        self.marked.as_ref().map(|m| 0..m.chars().count())
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
            self.insert(text);
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
            origin: element_bounds.origin,
            size: gpui::size(px(2.), (Type::EDITOR * 1.4)),
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
