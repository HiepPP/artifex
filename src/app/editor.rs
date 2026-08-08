//! Source editor.
//!
//! GPUI has no editor element, so this is POC code: a line buffer, a single
//! cursor, IME plumbing and a virtualised row list. Highlighting is resolved
//! for the rows the list actually asks for, never for the whole file.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle, Focusable,
    HighlightStyle, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, SharedString, Styled as _, StyledText,
    UTF16Selection, UniformListScrollHandle, Window, canvas, div, px, uniform_list,
};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use gpui_component::{Sizable as _, h_flex, v_flex};

use crate::services::highlight::{Highlighter, Lang, Span, line_starts};
use crate::services::search::{LineMatch, find_in_lines};
use crate::services::settings;
use crate::theme::{ActiveTokens as _, Colors, EditorZoom, Radius, Space, Type, UiZoom};

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
    /// Selection anchor. `Some` while a selection is active or forming; the
    /// head is always `(cursor_row, cursor_byte)`. `None` is a plain caret.
    anchor: Option<(usize, usize)>,
    /// True while a mouse drag is extending the selection.
    dragging: bool,
    /// First click on a token. This backs up the native click count when the
    /// platform reports two quick presses as separate clicks.
    last_token_click: Option<(std::time::Instant, usize, std::ops::Range<usize>)>,
    /// Monospace advance and the editor's frame, captured during paint so a
    /// click can be mapped back to a `(row, byte)` position.
    char_w: std::cell::Cell<Pixels>,
    /// Text scale from `Cmd-=`/`Cmd--`, refreshed from the `EditorZoom` global
    /// each render so click and IME math off the render thread stay in step.
    zoom: std::cell::Cell<f32>,
    content: std::cell::Cell<Bounds<Pixels>>,
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
            wrap: settings::word_wrap(cx),
            lines,
            highlighter,
            line_starts: starts,
            source,
            cursor_row: 0,
            cursor_byte: 0,
            anchor: None,
            dragging: false,
            last_token_click: None,
            char_w: std::cell::Cell::new(px(8.)),
            zoom: std::cell::Cell::new(1.0),
            content: std::cell::Cell::new(Bounds {
                origin: gpui::point(px(0.), px(0.)),
                size: gpui::size(px(0.), px(0.)),
            }),
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
        self.anchor = None;
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
        self.anchor = None;
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
        // Typing over a selection replaces it.
        self.delete_selection();
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
        self.delete_selection();
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
        // A backspace with a selection deletes the selection, nothing more.
        if self.delete_selection() {
            return;
        }
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

    fn move_cursor(&mut self, key: &str, select: bool) {
        // Shift extends from the existing anchor, dropping one only when the
        // move is unshifted.
        if select {
            if self.anchor.is_none() {
                self.anchor = Some((self.cursor_row, self.cursor_byte));
            }
        } else {
            self.anchor = None;
        }
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
        let key = event.keystroke.key.as_str();
        if m.platform {
            // The editor owns copy, cut and select-all. Save and find stay on
            // the shell keymap.
            match key {
                "c" => {
                    if let Some(text) = self.selected_string() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
                "x" => {
                    if let Some(text) = self.selected_string() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        self.delete_selection();
                        cx.notify();
                    }
                }
                "a" => {
                    self.select_all();
                    cx.notify();
                }
                _ => {}
            }
            return;
        }
        match key {
            "backspace" => self.backspace(),
            "enter" => self.newline(),
            "tab" => self.insert("    "),
            k @ ("left" | "right" | "up" | "down" | "home" | "end") => {
                self.move_cursor(k, m.shift)
            }
            _ => {
                // Plain characters arrive through the IME path, not here, so
                // nothing else is handled: see `replace_text_in_range`.
                return;
            }
        }
        cx.notify();
    }

    /// The ordered selection range, or `None` for a collapsed caret.
    fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.anchor?;
        let head = (self.cursor_row, self.cursor_byte);
        (anchor != head).then(|| ordered(anchor, head))
    }

    /// The selected text, joining rows with `\n`, or `None` when empty.
    fn selected_string(&self) -> Option<String> {
        let ((sr, sb), (er, eb)) = self.selection()?;
        if sr == er {
            return self.lines.get(sr).map(|l| l.get(sb..eb).unwrap_or("").to_string());
        }
        let mut out = self.lines.get(sr)?.get(sb..).unwrap_or("").to_string();
        for row in (sr + 1)..er {
            out.push('\n');
            if let Some(line) = self.lines.get(row) {
                out.push_str(line);
            }
        }
        out.push('\n');
        out.push_str(self.lines.get(er)?.get(..eb).unwrap_or(""));
        Some(out)
    }

    /// Removes the selection, collapses the caret to its start, clears the
    /// anchor. Returns whether anything was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some(((sr, sb), (er, eb))) = self.selection() else {
            return false;
        };
        if sr == er {
            if let Some(line) = self.lines.get_mut(sr)
                && sb <= line.len()
                && eb <= line.len()
            {
                line.replace_range(sb..eb, "");
            }
        } else {
            let tail = self.lines.get(er).and_then(|l| l.get(eb..)).unwrap_or("").to_string();
            if let Some(first) = self.lines.get_mut(sr) {
                let head = first.get(..sb).unwrap_or("").to_string();
                *first = head + &tail;
            }
            let drain_end = (er + 1).min(self.lines.len());
            self.lines.drain((sr + 1).min(drain_end)..drain_end);
        }
        self.cursor_row = sr;
        self.cursor_byte = sb;
        self.anchor = None;
        self.reindex();
        true
    }

    /// Selects the whole document, head at the end.
    fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        self.anchor = Some((0, 0));
        self.cursor_row = last;
        self.cursor_byte = self.lines.get(last).map(|l| l.len()).unwrap_or(0);
    }

    /// The rendered monospace size for the current zoom. Row height and IME
    /// bounds derive from this, so they must read the same scale render does.
    fn font_px(&self) -> Pixels {
        Type::EDITOR * self.zoom.get()
    }

    /// Maps a window-space point to the `(row, byte)` it falls on, using the
    /// frame and advance captured during paint and the live scroll offset.
    fn position_at(&self, pos: Point<Pixels>) -> (usize, usize) {
        let content = self.content.get();
        let char_w = self.char_w.get();
        let count = self.lines.len().max(1);
        let (offset_x, offset_y, item_h) = {
            let st = self.scroll.0.borrow();
            // `last_item_size.item` is the viewport, not one row; the row height
            // is the content height divided across the rows.
            let item_h = st
                .last_item_size
                .map(|s| s.contents.height / count as f32)
                .filter(|h| *h > px(0.))
                .unwrap_or(px(f32::from(self.font_px()) * 1.4));
            let offset = st.base_handle.offset();
            (offset.x, offset.y, item_h)
        };
        // The list is inset by Space::S; rows begin below that, text begins
        // after the gutter. Scroll shifts every row by (offset_x, offset_y),
        // both negative once scrolled away from the origin.
        let rel_y = pos.y - content.origin.y - Space::S - offset_y;
        let max_row = self.lines.len().saturating_sub(1);
        let row = y_to_row(rel_y, item_h, 0, max_row);
        let line = self.lines.get(row).map(String::as_str).unwrap_or("");
        let rel_x = pos.x - content.origin.x - Space::S - GUTTER_WIDTH - offset_x;
        let col = x_to_col(rel_x, char_w, line.chars().count());
        (row, col_to_byte(line, col))
    }

    /// Places the caret from a click inside a wrapped row. The row is known
    /// from the row's own handler, so only the column is mapped; on a wrapped
    /// continuation line that column is approximate.
    fn place_caret_wrapped(&mut self, row: usize, pos: Point<Pixels>) {
        let content = self.content.get();
        let char_w = self.char_w.get();
        let row = row.min(self.lines.len().saturating_sub(1));
        let line = self.lines.get(row).map(String::as_str).unwrap_or("");
        let rel_x = pos.x - content.origin.x - Space::S - GUTTER_WIDTH;
        let col = x_to_col(rel_x, char_w, line.chars().count());
        self.cursor_row = row;
        self.cursor_byte = col_to_byte(line, col);
        self.anchor = None;
    }

    /// Selects the identifier token touching `byte` on `row`.
    fn select_token(&mut self, row: usize, byte: usize) {
        let Some(line) = self.lines.get(row) else {
            self.anchor = None;
            return;
        };
        let Some(range) = token_range(line, byte) else {
            self.anchor = None;
            return;
        };
        self.anchor = Some((row, range.start));
        self.cursor_row = row;
        self.cursor_byte = range.end;
    }

    fn is_token_double_click(&mut self, row: usize, byte: usize, click_count: usize) -> bool {
        let now = std::time::Instant::now();
        let range = self.lines.get(row).and_then(|line| token_range(line, byte));
        let repeated = range.as_ref().is_some_and(|range| {
            self.last_token_click
                .as_ref()
                .is_some_and(|(last_at, last_row, last_range)| {
                    *last_row == row
                        && last_range == range
                        && now.saturating_duration_since(*last_at)
                            <= std::time::Duration::from_millis(750)
                })
        });
        let double_click = click_count >= 2 || repeated;
        self.last_token_click = if double_click {
            None
        } else {
            range.map(|range| (now, row, range))
        };
        double_click
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Wrapped rows carry their own click handler; the whole-editor mapping
        // assumes fixed-height virtualised rows, so it must not run here.
        if self.wrap {
            return;
        }
        let (row, byte) = self.position_at(event.position);
        let double_click = self.is_token_double_click(row, byte, event.click_count);
        self.cursor_row = row;
        self.cursor_byte = byte;
        if double_click {
            self.select_token(row, byte);
            self.dragging = false;
        } else {
            self.anchor = Some((row, byte));
            self.dragging = true;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.wrap || !self.dragging {
            return;
        }
        let (row, byte) = self.position_at(event.position);
        self.cursor_row = row;
        self.cursor_byte = byte;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.wrap || !self.dragging {
            return;
        }
        self.dragging = false;
        // A click that never moved collapses to a caret.
        if self.anchor == Some((self.cursor_row, self.cursor_byte)) {
            self.anchor = None;
            cx.notify();
        }
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

        let char_w = self.char_w.get();
        let selection = self.selection();

        range
            .map(|row| self.build_row(row, spans.get(&row), char_w, selection, c, false))
            .collect()
    }

    /// Builds one editor row. `wrap` off gives a natural-width row that never
    /// wraps, so a long line drives the horizontal scroll; `wrap` on gives a
    /// full-width row whose text wraps. Selection fill and caret are painted by
    /// character column, exact off-wrap and on the first visual line when
    /// wrapped.
    fn build_row(
        &self,
        row: usize,
        row_spans: Option<&Vec<Span>>,
        char_w: Pixels,
        selection: Option<((usize, usize), (usize, usize))>,
        c: Colors,
        wrap: bool,
    ) -> gpui::AnyElement {
        let text = self.lines.get(row).cloned().unwrap_or_default();
        let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = row_spans
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

        let highlights = self.overlay_find_matches(row, highlights, text.len(), c);
        let is_cursor_row = row == self.cursor_row;
        let caret_col = byte_to_col(&text, self.cursor_byte);
        let line_cols = text.chars().count();
        // Selected columns on this row: full rows below the first draw one extra
        // cell to hint that the newline is inside the run.
        let sel_cols = selection.and_then(|((sr, sb), (er, eb))| {
            if row < sr || row > er {
                return None;
            }
            let start = if row == sr { byte_to_col(&text, sb) } else { 0 };
            let end = if row == er { byte_to_col(&text, eb) } else { line_cols + 1 };
            (end > start).then_some((start, end))
        });

        // Wrapped: fill the width so the text wraps. Off-wrap: hug the text so a
        // long line overflows into the horizontal scroll.
        let mut body = if wrap {
            div().flex_1().min_w(px(0.)).relative()
        } else {
            div()
                .relative()
                .flex_none()
                .w(char_w * line_cols as f32)
                .whitespace_nowrap()
        };
        if let Some((start, end)) = sel_cols {
            body = body.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(char_w * start as f32)
                    .w(char_w * (end - start) as f32)
                    .bg(c.selection),
            );
        }
        if is_cursor_row {
            body = body.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(char_w * caret_col as f32)
                    .w(px(2.))
                    .bg(c.accent),
            );
        }

        h_flex()
            .when(wrap, |this| this.w_full())
            .when(!wrap, |this| {
                this.flex_none()
                    .w(GUTTER_WIDTH + char_w * line_cols as f32)
            })
            .when(is_cursor_row && sel_cols.is_none(), |this| {
                this.bg(c.selection.opacity(0.35))
            })
            .child(
                div()
                    .w(GUTTER_WIDTH)
                    .flex_none()
                    .pr(Space::M)
                    .text_right()
                    .text_color(c.ink_secondary.opacity(0.7))
                    .child(SharedString::from((row + 1).to_string())),
            )
            .child(
                body.child(StyledText::new(SharedString::from(text)).with_highlights(highlights)),
            )
            .into_any_element()
    }

    /// Renders the wrapping column. Not virtualised: wrapping gives rows
    /// varying heights, which `uniform_list` cannot express, so every row is
    /// built. Each row carries its own click handler, since the whole-editor
    /// point mapping assumes fixed-height rows. Wrap is opt-in, so this cost
    /// only applies to a file the reader chose to wrap.
    fn render_wrapped(&self, c: Colors, entity: Entity<Self>) -> gpui::AnyElement {
        let char_w = self.char_w.get();
        let selection = self.selection();
        let spans = self.highlighter.spans_in(
            &self.source,
            0..self.source.len(),
            &self.line_starts,
            &c,
        );

        let rows = (0..self.lines.len()).map(|row| {
            let handler = entity.clone();
            div()
                .id(("editor-wrap-row", row))
                .on_mouse_down(
                    MouseButton::Left,
                    move |event: &MouseDownEvent, window, cx| {
                        let pos = event.position;
                        handler.update(cx, |this, cx| {
                            this.place_caret_wrapped(row, pos);
                            if this.is_token_double_click(row, this.cursor_byte, event.click_count) {
                                this.select_token(row, this.cursor_byte);
                            }
                            window.focus(&this.focus, cx);
                            cx.notify();
                        });
                    },
                )
                .child(self.build_row(row, spans.get(&row), char_w, selection, c, true))
        });

        div()
            .id("editor-wrap")
            .size_full()
            .overflow_y_scroll()
            .p(Space::S)
            .child(v_flex().children(rows))
            .into_any_element()
    }

    /// Lays the find matches over the syntax highlights for one row. Ranges
    /// handed to `StyledText` must not overlap, so syntax spans are clipped
    /// around every match before the match styles are added.
    fn overlay_find_matches(
        &self,
        row: usize,
        syntax: Vec<(std::ops::Range<usize>, HighlightStyle)>,
        line_len: usize,
        c: Colors,
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
                                .text_size(Type::MICRO * ui_zoom)
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
        self.zoom.set(cx.global::<EditorZoom>().0);
        let font_px = self.font_px();
        let entity = cx.entity();
        let count = self.lines.len();
        let longest_line = self
            .lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.chars().count())
            .map(|(row, _)| row);
        let longest_line_width = self
            .lines
            .iter()
            .map(|line| self.char_w.get() * line.chars().count() as f32)
            .max()
            .unwrap_or(px(0.));
        let content_width = GUTTER_WIDTH + Space::S * 2. + longest_line_width;
        let focus = self.focus.clone();
        let list_entity = entity.clone();
        let canvas_entity = entity.clone();

        // Wrap on: a non-virtualised wrapping column. Wrap off: the virtualised
        // list, now unconstrained horizontally so a long line scrolls sideways.
        let content = if self.wrap {
            self.render_wrapped(c, entity.clone())
        } else {
            div()
                .relative()
                .size_full()
                .child(
                    uniform_list("editor-rows", count, move |range, _window, cx| {
                        list_entity.read(cx).render_rows(range, cx)
                    })
                    .track_scroll(&self.scroll)
                    .with_width_from_item(longest_line)
                    .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
                    .with_horizontal_sizing_behavior(
                        gpui::ListHorizontalSizingBehavior::Unconstrained,
                    )
                    .size_full()
                    .p(Space::S),
                )
                .into_any_element()
        };
        let horizontal_scrollbar = (!self.wrap).then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(16.))
                .child(
                    Scrollbar::horizontal(&self.scroll)
                        .id("editor-horizontal-scrollbar")
                        .scroll_size(gpui::size(content_width, px(1.)))
                        .scrollbar_show(ScrollbarShow::Always),
                )
                .into_any_element()
        });

        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("Editor")
            .size_full()
            .bg(c.editor)
            .text_color(c.ink)
            .font_family("JetBrains Mono")
            .text_size(font_px)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |_, bounds, window, cx| {
                        // Capture the frame and the monospace advance so a click
                        // can be mapped back to a (row, byte) position.
                        let font = gpui::font("JetBrains Mono");
                        let font_id = window.text_system().resolve_font(&font);
                        let cw = window
                            .text_system()
                            .ch_advance(font_id, font_px)
                            .unwrap_or(font_px * 0.6);
                        canvas_entity.update(cx, |this, _| {
                            this.content.set(bounds);
                            this.char_w.set(cw);
                        });
                        window.handle_input(
                            &focus,
                            gpui::ElementInputHandler::new(bounds, canvas_entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(content)
            .when_some(horizontal_scrollbar, |this, scrollbar| {
                this.child(scrollbar)
            })
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
            size: gpui::size(px(2.), (self.font_px() * 1.4)),
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

// --- Geometry and selection helpers (pure, unit-tested) ------------------

/// Byte offset of the `col`-th character in `line`, clamped to the line end.
pub(crate) fn col_to_byte(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len())
}

/// Character index of byte offset `byte` within `line`.
pub(crate) fn byte_to_col(line: &str, byte: usize) -> usize {
    line.char_indices().take_while(|(b, _)| *b < byte).count()
}

/// Nearest caret column for a horizontal offset `x` into the text. Rounds to
/// the closest character boundary and clamps to `[0, cols]`.
pub(crate) fn x_to_col(x: Pixels, char_w: Pixels, cols: usize) -> usize {
    if char_w <= px(0.) {
        return 0;
    }
    let c = (f32::from(x) / f32::from(char_w)).round();
    (c.max(0.) as usize).min(cols)
}

/// Row for a vertical offset `y` measured from the top of `first_row`. Clamps
/// to `[0, max_row]`.
pub(crate) fn y_to_row(y: Pixels, row_h: Pixels, first_row: usize, max_row: usize) -> usize {
    if row_h <= px(0.) {
        return first_row.min(max_row);
    }
    let delta = (f32::from(y) / f32::from(row_h)).floor() as isize;
    (first_row as isize + delta).clamp(0, max_row as isize) as usize
}

/// Orders two `(row, byte)` positions low-to-high.
pub(crate) fn ordered(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Identifier token touching `byte`. Punctuation checks its right token first,
/// then its left token, so rounded caret positions stay forgiving.
pub(crate) fn token_range(line: &str, byte: usize) -> Option<std::ops::Range<usize>> {
    let mut at = byte.min(line.len());
    while at > 0 && !line.is_char_boundary(at) {
        at -= 1;
    }

    let current = line.get(at..).and_then(|tail| tail.chars().next());
    let seed = match current {
        Some(ch) if is_token_char(ch) => at,
        Some(ch) if ch.is_whitespace() => return None,
        Some(ch) => {
            let next = at + ch.len_utf8();
            line.get(next..)
                .and_then(|tail| tail.chars().next())
                .filter(|ch| is_token_char(*ch))
                .map(|_| next)
                .or_else(|| {
                    line.get(..at)
                        .and_then(|head| head.char_indices().next_back())
                        .filter(|(_, ch)| is_token_char(*ch))
                        .map(|(start, _)| start)
                })?
        }
        None => line
            .get(..at)
            .and_then(|head| head.char_indices().next_back())
            .filter(|(_, ch)| is_token_char(*ch))
            .map(|(start, _)| start)?,
    };

    let mut start = seed;
    while let Some((previous, ch)) = line
        .get(..start)
        .and_then(|head| head.char_indices().next_back())
    {
        if !is_token_char(ch) {
            break;
        }
        start = previous;
    }

    let mut end = seed;
    while let Some(ch) = line.get(end..).and_then(|tail| tail.chars().next()) {
        if !is_token_char(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    Some(start..end)
}

fn is_token_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
