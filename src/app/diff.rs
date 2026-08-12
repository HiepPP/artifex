//! Git diff surface.
//!
//! A read-only sibling of the editor, styled after VSCode's inline diff: two
//! gutters, a sign column, full-width tinted line backgrounds, syntax
//! highlighting and word-level change emphasis. It carries the editor's
//! selection model so the reader can drag-select the code and copy it. There is
//! no editing and no IME insertion; only selection and copy are shared with the
//! editor.
//!
//! Syntax colours come from two reconstructed pseudo-documents: the new side
//! (context + additions) and the old side (context + deletions). Each is parsed
//! whole once, then queried per visible range, exactly like the editor. Word
//! emphasis pairs each deleted line with the added line that replaced it and
//! marks the differing span with a stronger tint.

use std::ops::Range;
use std::path::Path;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, HighlightStyle, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, SharedString, Styled as _, StyledText, UniformListScrollHandle, Window, canvas,
    div, px, uniform_list,
};
use gpui_component::h_flex;
use gpui_component::scroll::{Scrollbar, ScrollbarShow};

use crate::app::editor::{byte_to_col, col_to_byte, ordered, x_to_col, y_to_row};
use crate::services::git::{DiffRow, parse_diff};
use crate::services::highlight::{Highlighter, Lang, Span, line_starts};
use crate::theme::{ActiveTokens as _, Colors, EditorZoom, Space, Type};

/// Two 40-point gutters plus the 16-point sign column: the x-origin of the
/// selectable text, measured from the padded row start.
const TEXT_X: Pixels = px(96.);

/// A line longer than this is skipped by the word-diff: the quadratic-ish trim
/// is cheap, but a minified megaline is not worth emphasising.
const WORD_DIFF_LIMIT: usize = 2000;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Add,
    Del,
    Ctx,
    Hunk,
}

/// Which pseudo-document a line's syntax colours are read from.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    New,
    Old,
    None,
}

/// One rendered row. `old`/`new` are the real file line numbers; `text` is the
/// visible, selectable content with the diff sign already stripped. `doc_line`
/// indexes the side's pseudo-document for syntax lookup; `word` holds the byte
/// ranges that changed against the paired line, for emphasis.
struct Line {
    old: Option<u32>,
    new: Option<u32>,
    kind: Kind,
    side: Side,
    doc_line: usize,
    text: String,
    word: Vec<Range<usize>>,
}

pub struct DiffView {
    lines: Vec<Line>,
    new_hl: Highlighter,
    old_hl: Highlighter,
    new_doc: String,
    old_doc: String,
    new_starts: Vec<usize>,
    old_starts: Vec<usize>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// Selection anchor. `Some` while a selection is active or forming; the head
    /// is always `(cursor_row, cursor_byte)`. `None` is a plain caret.
    anchor: Option<(usize, usize)>,
    cursor_row: usize,
    cursor_byte: usize,
    dragging: bool,
    /// Monospace advance and the view's frame, captured during paint so a click
    /// maps back to a `(row, byte)` position.
    char_w: std::cell::Cell<Pixels>,
    /// Text scale from `Cmd-=`/`Cmd--`, refreshed from the `EditorZoom` global
    /// each render so click math off the render thread stays in step.
    zoom: std::cell::Cell<f32>,
    content: std::cell::Cell<Bounds<Pixels>>,
}

impl DiffView {
    pub fn new(path: &Path, text: &str, cx: &mut App) -> Entity<Self> {
        let mut lines: Vec<Line> = Vec::new();
        // Running line counts per side; each becomes a line's `doc_line`.
        let mut new_count = 0usize;
        let mut old_count = 0usize;

        for row in parse_diff(text, 20_000) {
            let line = match row {
                DiffRow::Hunk { range, context } => Line {
                    old: None,
                    new: None,
                    kind: Kind::Hunk,
                    side: Side::None,
                    doc_line: 0,
                    text: if context.is_empty() {
                        format!("@@ {range}")
                    } else {
                        format!("@@ {range}  {context}")
                    },
                    word: Vec::new(),
                },
                // Context is present on both sides, keeping each side's line
                // numbering contiguous.
                DiffRow::Ctx { old, new, text } => {
                    let doc_line = new_count;
                    new_count += 1;
                    old_count += 1;
                    Line {
                        old: Some(old),
                        new: Some(new),
                        kind: Kind::Ctx,
                        side: Side::New,
                        doc_line,
                        text,
                        word: Vec::new(),
                    }
                }
                DiffRow::Add { new, text } => {
                    let doc_line = new_count;
                    new_count += 1;
                    Line {
                        old: None,
                        new: Some(new),
                        kind: Kind::Add,
                        side: Side::New,
                        doc_line,
                        text,
                        word: Vec::new(),
                    }
                }
                DiffRow::Del { old, text } => {
                    let doc_line = old_count;
                    old_count += 1;
                    Line {
                        old: Some(old),
                        new: None,
                        kind: Kind::Del,
                        side: Side::Old,
                        doc_line,
                        text,
                        word: Vec::new(),
                    }
                }
            };
            lines.push(line);
        }

        compute_word_diff(&mut lines);

        // Reconstruct each side's pseudo-document, in the same order the counters
        // above walked, so `doc_line` indexes straight into it.
        let mut new_parts: Vec<&str> = Vec::new();
        let mut old_parts: Vec<&str> = Vec::new();
        for line in &lines {
            match line.kind {
                Kind::Ctx => {
                    new_parts.push(&line.text);
                    old_parts.push(&line.text);
                }
                Kind::Add => new_parts.push(&line.text),
                Kind::Del => old_parts.push(&line.text),
                Kind::Hunk => {}
            }
        }
        let new_doc = new_parts.join("\n");
        let old_doc = old_parts.join("\n");

        let lang = Lang::for_path(path);
        let mut new_hl = Highlighter::new(lang);
        new_hl.parse(&new_doc);
        let mut old_hl = Highlighter::new(lang);
        old_hl.parse(&old_doc);
        let new_starts = line_starts(&new_doc);
        let old_starts = line_starts(&old_doc);

        cx.new(|cx| Self {
            lines,
            new_hl,
            old_hl,
            new_doc,
            old_doc,
            new_starts,
            old_starts,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            anchor: None,
            cursor_row: 0,
            cursor_byte: 0,
            dragging: false,
            char_w: std::cell::Cell::new(px(8.)),
            zoom: std::cell::Cell::new(1.0),
            content: std::cell::Cell::new(Bounds {
                origin: gpui::point(px(0.), px(0.)),
                size: gpui::size(px(0.), px(0.)),
            }),
        })
    }

    fn longest_cols(&self) -> usize {
        self.lines
            .iter()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0)
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
            return self
                .lines
                .get(sr)
                .map(|l| l.text.get(sb..eb).unwrap_or("").to_string());
        }
        let mut out = self.lines.get(sr)?.text.get(sb..).unwrap_or("").to_string();
        for row in (sr + 1)..er {
            out.push('\n');
            if let Some(line) = self.lines.get(row) {
                out.push_str(&line.text);
            }
        }
        out.push('\n');
        out.push_str(self.lines.get(er)?.text.get(..eb).unwrap_or(""));
        Some(out)
    }

    fn select_all(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        self.anchor = Some((0, 0));
        self.cursor_row = last;
        self.cursor_byte = self.lines.get(last).map(|l| l.text.len()).unwrap_or(0);
    }

    /// The rendered monospace size for the current zoom. The diff shares the
    /// editor's `Type::EDITOR` base so every content surface reads one size. The
    /// row-height fallback derives from it, so it must read the same scale.
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
            let item_h = st
                .last_item_size
                .map(|s| s.contents.height / count as f32)
                .filter(|h| *h > px(0.))
                .unwrap_or(px(f32::from(self.font_px()) * 1.4));
            let offset = st.base_handle.offset();
            (offset.x, offset.y, item_h)
        };
        let rel_y = pos.y - content.origin.y - Space::S - offset_y;
        let max_row = self.lines.len().saturating_sub(1);
        let row = y_to_row(rel_y, item_h, 0, max_row);
        let line = self.lines.get(row).map(|l| l.text.as_str()).unwrap_or("");
        let rel_x = pos.x - content.origin.x - Space::S - TEXT_X - offset_x;
        let col = x_to_col(rel_x, char_w, line.chars().count());
        (row, col_to_byte(line, col))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (row, byte) = self.position_at(event.position);
        self.cursor_row = row;
        self.cursor_byte = byte;
        self.anchor = Some((row, byte));
        self.dragging = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let (row, byte) = self.position_at(event.position);
        self.cursor_row = row;
        self.cursor_byte = byte;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.dragging = false;
        if self.anchor == Some((self.cursor_row, self.cursor_byte)) {
            self.anchor = None;
            cx.notify();
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = &event.keystroke.modifiers;
        if !m.platform {
            return;
        }
        match event.keystroke.key.as_str() {
            "c" => {
                if let Some(text) = self.selected_string() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "a" => {
                self.select_all();
                cx.notify();
            }
            _ => {}
        }
    }

    fn render_rows(&self, range: std::ops::Range<usize>, cx: &App) -> Vec<gpui::AnyElement> {
        let c = cx.tokens().c;
        let char_w = self.char_w.get();
        let selection = self.selection();
        let row_width = TEXT_X + char_w * self.longest_cols() as f32;

        // Syntax spans for exactly the visible lines of each side.
        let new_spans = self.side_spans(Side::New, &range, &c);
        let old_spans = self.side_spans(Side::Old, &range, &c);

        range
            .map(|row| {
                let Some(line) = self.lines.get(row) else {
                    return div().into_any_element();
                };
                let spans = match line.side {
                    Side::New => new_spans.get(&line.doc_line),
                    Side::Old => old_spans.get(&line.doc_line),
                    Side::None => None,
                };
                self.build_row(row, line, spans, char_w, row_width, selection, c)
            })
            .collect()
    }

    /// Highlights the visible lines belonging to `side` and returns them keyed by
    /// their pseudo-document line index.
    fn side_spans(
        &self,
        side: Side,
        range: &std::ops::Range<usize>,
        c: &Colors,
    ) -> std::collections::HashMap<usize, Vec<Span>> {
        let (hl, doc, starts) = match side {
            Side::New => (&self.new_hl, &self.new_doc, &self.new_starts),
            Side::Old => (&self.old_hl, &self.old_doc, &self.old_starts),
            Side::None => return std::collections::HashMap::new(),
        };
        let visible: Vec<usize> = self.lines[range.clone()]
            .iter()
            .filter(|l| l.side == side)
            .map(|l| l.doc_line)
            .collect();
        let (Some(&lo), Some(&hi)) = (visible.iter().min(), visible.iter().max()) else {
            return std::collections::HashMap::new();
        };
        let start = starts.get(lo).copied().unwrap_or(0);
        let end = starts.get(hi + 1).copied().unwrap_or(doc.len());
        hl.spans_in(doc, start..end, starts, c)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_row(
        &self,
        row: usize,
        line: &Line,
        spans: Option<&Vec<Span>>,
        char_w: Pixels,
        row_width: Pixels,
        selection: Option<((usize, usize), (usize, usize))>,
        c: Colors,
    ) -> gpui::AnyElement {
        let (sign, tint) = match line.kind {
            Kind::Add => ("+", Some(c.git_added)),
            Kind::Del => ("-", Some(c.git_deleted)),
            Kind::Ctx | Kind::Hunk => ("", None),
        };
        let text_color = if line.kind == Kind::Hunk {
            c.git_untracked
        } else {
            c.ink
        };

        let gutter = |number: Option<u32>| {
            div()
                .w(px(40.))
                .flex_none()
                .pr(Space::XS)
                .text_right()
                .text_color(c.ink_secondary.opacity(0.7))
                .child(SharedString::from(
                    number.map(|n| n.to_string()).unwrap_or_default(),
                ))
        };

        let text = &line.text;
        let line_cols = text.chars().count();

        // Syntax colours. `spans_in` already resolves overlaps to the narrowest
        // span, so the ranges never overlap and go straight to `StyledText`.
        let highlights: Vec<(Range<usize>, HighlightStyle)> = spans
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

        let sel_cols = selection.and_then(|((sr, sb), (er, eb))| {
            if row < sr || row > er {
                return None;
            }
            let start = if row == sr { byte_to_col(text, sb) } else { 0 };
            let end = if row == er {
                byte_to_col(text, eb)
            } else {
                line_cols + 1
            };
            (end > start).then_some((start, end))
        });

        let mut body = div()
            .relative()
            .flex_none()
            .w(char_w * line_cols as f32)
            .whitespace_nowrap();
        // Word-change emphasis sits under the text, over the line tint.
        for change in &line.word {
            let start = byte_to_col(text, change.start);
            let end = byte_to_col(text, change.end);
            if end > start {
                body = body.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(char_w * start as f32)
                        .w(char_w * (end - start) as f32)
                        .when_some(tint, |this, t| this.bg(t.opacity(0.32))),
                );
            }
        }
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

        h_flex()
            .flex_none()
            .w(row_width)
            .when(line.kind == Kind::Hunk, |this| this.bg(c.raised))
            .when_some(tint, |this, tint| this.bg(tint.opacity(0.12)))
            .child(gutter(line.old))
            .child(gutter(line.new))
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .text_center()
                    .when_some(tint, |this, tint| this.text_color(tint))
                    .child(SharedString::from(sign)),
            )
            .child(body.child(div().text_color(text_color).child(
                StyledText::new(SharedString::from(text.clone())).with_highlights(highlights),
            )))
            .into_any_element()
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        self.zoom.set(cx.global::<EditorZoom>().0);
        let font_px = self.font_px();
        let count = self.lines.len();
        let entity = cx.entity();
        let list_entity = entity.clone();
        let canvas_entity = entity.clone();

        let longest = self
            .lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.text.chars().count())
            .map(|(row, _)| row);
        let content_width = TEXT_X + Space::S * 2. + self.char_w.get() * self.longest_cols() as f32;

        let list = uniform_list("diff-rows", count, move |range, _window, cx| {
            list_entity.read(cx).render_rows(range, cx)
        })
        .track_scroll(&self.scroll)
        .with_width_from_item(longest)
        .with_sizing_behavior(gpui::ListSizingBehavior::Auto)
        .with_horizontal_sizing_behavior(gpui::ListHorizontalSizingBehavior::Unconstrained)
        .size_full()
        .p(Space::S);

        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("Diff")
            .size_full()
            .bg(c.editor)
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
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(list)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(16.))
                    .child(
                        Scrollbar::horizontal(&self.scroll)
                            .id("diff-horizontal-scrollbar")
                            .scroll_size(gpui::size(content_width, px(1.)))
                            .scrollbar_show(ScrollbarShow::Always),
                    ),
            )
    }
}

/// Pairs each deleted line with the added line that replaced it and records the
/// changed byte span on each, so a within-line edit is emphasised the way VSCode
/// does rather than repainting the whole line.
fn compute_word_diff(lines: &mut [Line]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != Kind::Del {
            i += 1;
            continue;
        }
        let del_start = i;
        let mut j = i;
        while j < lines.len() && lines[j].kind == Kind::Del {
            j += 1;
        }
        let add_start = j;
        let mut k = j;
        while k < lines.len() && lines[k].kind == Kind::Add {
            k += 1;
        }
        let pairs = (add_start - del_start).min(k - add_start);
        for p in 0..pairs {
            let (old_text, new_text) = (
                lines[del_start + p].text.clone(),
                lines[add_start + p].text.clone(),
            );
            let (old_range, new_range) = word_diff(&old_text, &new_text);
            if let Some(r) = old_range {
                lines[del_start + p].word.push(r);
            }
            if let Some(r) = new_range {
                lines[add_start + p].word.push(r);
            }
        }
        i = k.max(i + 1);
    }
}

/// The single differing span between two lines, trimming the shared prefix and
/// suffix. `None` on a side means that side is unchanged (or the line is too
/// long to bother). Byte offsets fall on char boundaries.
fn word_diff(a: &str, b: &str) -> (Option<Range<usize>>, Option<Range<usize>>) {
    if a == b || a.len() > WORD_DIFF_LIMIT || b.len() > WORD_DIFF_LIMIT {
        return (None, None);
    }
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();

    let mut p = 0;
    while p < ac.len() && p < bc.len() && ac[p] == bc[p] {
        p += 1;
    }
    let mut s = 0;
    while s < ac.len() - p && s < bc.len() - p && ac[ac.len() - 1 - s] == bc[bc.len() - 1 - s] {
        s += 1;
    }

    let prefix_a: usize = ac[..p].iter().map(|ch| ch.len_utf8()).sum();
    let prefix_b: usize = bc[..p].iter().map(|ch| ch.len_utf8()).sum();
    let suffix_a: usize = ac[ac.len() - s..].iter().map(|ch| ch.len_utf8()).sum();
    let suffix_b: usize = bc[bc.len() - s..].iter().map(|ch| ch.len_utf8()).sum();

    let old = (prefix_a < a.len() - suffix_a).then(|| prefix_a..a.len() - suffix_a);
    let new = (prefix_b < b.len() - suffix_b).then(|| prefix_b..b.len() - suffix_b);
    (old, new)
}
