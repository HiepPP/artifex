//! Markdown preview.
//!
//! Parses with `pulldown-cmark` and renders headings, prose, lists, tables,
//! code cards and quotes as GPUI elements. Prose blocks (heading, paragraph,
//! list item, quote) reparse their inline Markdown into selectable
//! `StyledText` runs, preserving the Primer fonts, colours and mono code chips.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, BorderStyle, Bounds, ClipboardItem, Context, Edges, Element, ElementId,
    Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, HighlightStyle, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, ListAlignment, ListOffset, ListState,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, ScrollHandle, SharedString, StyledText, TextLayout, Window, canvas, div, list, point,
    px, quad,
};
use gpui_component::clipboard::Clipboard;
use gpui_component::scroll::Scrollbar;
use gpui_component::{h_flex, v_flex};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::services::highlight::{Highlighter, Lang, line_starts};
use crate::theme::{ActiveTokens as _, Colors, EditorZoom, Space, Type};

/// GitHub renders `.markdown-body` on a 980-point measure with 32 points of
/// padding; every block, including tables and code, shares the one column.
const PROSE_WIDTH: f32 = 980.;
const BLEED_WIDTH: f32 = PROSE_WIDTH;
const VIEWPORT_PADDING: Pixels = px(32.);

/// GitHub Primer markdown palette. The preview matches github.com instead of
/// the application tokens, in light and dark.
#[derive(Clone, Copy)]
struct Gh {
    bg: Hsla,
    fg: Hsla,
    muted: Hsla,
    border: Hsla,
    code_bg: Hsla,
    alt_row: Hsla,
    checked: Hsla,
    chip: Hsla,
}

fn gh_palette(dark: bool) -> Gh {
    fn c(value: u32) -> Hsla {
        let rgba: gpui::Rgba = gpui::rgb(value);
        rgba.into()
    }
    if dark {
        Gh {
            bg: c(0x0D1117),
            fg: c(0xF0F6FC),
            muted: c(0x9198A1),
            border: c(0x3D444D),
            code_bg: c(0x151B23),
            alt_row: c(0x151B23),
            checked: c(0x4493F8),
            chip: c(0x656C76).opacity(0.2),
        }
    } else {
        Gh {
            bg: c(0xFFFFFF),
            fg: c(0x1F2328),
            muted: c(0x59636E),
            border: c(0xD1D9E0),
            code_bg: c(0xF6F8FA),
            alt_row: c(0xF6F8FA),
            checked: c(0x0969DA),
            chip: c(0x818B98).opacity(0.12),
        }
    }
}

#[derive(Clone)]
struct HighlightedCode {
    source: String,
    highlighter: Rc<Highlighter>,
    line_starts: Rc<Vec<usize>>,
}

impl HighlightedCode {
    fn new(language: String, source: String) -> Self {
        let mut highlighter = Highlighter::new(fence_language(&language));
        highlighter.parse(&source);
        Self {
            line_starts: Rc::new(line_starts(&source)),
            source,
            highlighter: Rc::new(highlighter),
        }
    }
}

#[derive(Clone)]
enum Block {
    Heading(u8, String),
    Paragraph(String),
    ListItem(usize, String, Option<bool>, Option<u64>),
    Code(HighlightedCode),
    Quote(String),
    Rule,
    Table(Vec<Vec<String>>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProsePosition {
    block: usize,
    byte: usize,
}

#[derive(Clone)]
struct ProseLayout {
    block: usize,
    range_start: usize,
    text: SharedString,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

#[derive(Clone)]
struct ProseRenderState {
    layouts: Rc<RefCell<HashMap<(usize, usize), ProseLayout>>>,
    selection: Option<(ProsePosition, ProsePosition)>,
    selection_color: Hsla,
}

fn distance_to_bounds(point: Point<Pixels>, bounds: Bounds<Pixels>) -> f32 {
    let dx = if point.x < bounds.left() {
        bounds.left() - point.x
    } else if point.x > bounds.right() {
        point.x - bounds.right()
    } else {
        px(0.)
    };
    let dy = if point.y < bounds.top() {
        bounds.top() - point.y
    } else if point.y > bounds.bottom() {
        point.y - bounds.bottom()
    } else {
        px(0.)
    };
    f32::from(dx).powi(2) + f32::from(dy).powi(2)
}

/// One heading in the "On This Page" rail.
struct Heading {
    block: usize,
    level: u8,
    title: SharedString,
}

#[derive(Clone, Copy, Debug)]
pub struct ActiveHeadingChanged {
    pub block: Option<usize>,
}

/// A parsed Markdown document.
///
/// The parse happens on open and on reload, never per frame. Rendering a
/// 100 KB document from scratch on every notify was the obvious way to get this
/// wrong.
pub struct MarkdownView {
    path: PathBuf,
    blocks: Vec<Block>,
    headings: Vec<Heading>,
    active_heading: Option<usize>,
    links: Vec<PathBuf>,
    /// Virtualised block list. Styled prose and its selection layout are heavier
    /// than a plain-text `div`; `list` measures and paints only the blocks in the
    /// viewport (plus overdraw), so scroll cost tracks the viewport.
    list: ListState,
    /// Measured width of the document column.
    ///
    /// A table needs definite column widths: a flex or percentage cell inside a
    /// scrolling box has nothing to resolve against, so the row reports no
    /// height and the table disappears. The measure arrives one frame late,
    /// which is why it starts at the prose measure instead of zero.
    measure: Rc<Cell<Pixels>>,
    /// One horizontal scroll handle per code card, owned by the view so the
    /// scrollbar keeps a stable handle across virtualised re-renders.
    code_scrolls: RefCell<HashMap<usize, ScrollHandle>>,
    /// Exact `StyledText` layouts for prose currently painted by the virtual
    /// list. Mouse positions map back to byte offsets through these layouts.
    prose_layouts: Rc<RefCell<HashMap<(usize, usize), ProseLayout>>>,
    focus: FocusHandle,
    selection_anchor: Option<ProsePosition>,
    selection_head: Option<ProsePosition>,
    dragging_selection: bool,
}

impl MarkdownView {
    pub fn open(path: PathBuf, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let mut view = Self {
                path,
                blocks: Vec::new(),
                headings: Vec::new(),
                active_heading: None,
                links: Vec::new(),
                list: ListState::new(0, ListAlignment::Top, px(400.)),
                measure: Rc::new(Cell::new(px(PROSE_WIDTH))),
                code_scrolls: RefCell::new(HashMap::new()),
                prose_layouts: Rc::new(RefCell::new(HashMap::new())),
                focus: cx.focus_handle(),
                selection_anchor: None,
                selection_head: None,
                dragging_selection: false,
            };
            view.reload();
            view
        })
    }

    pub fn reload(&mut self) {
        let source = std::fs::read_to_string(&self.path).unwrap_or_default();
        self.blocks = parse(&source);
        self.links = local_links(&source, &self.path);
        self.headings = self
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(block, item)| match item {
                Block::Heading(level, text) => Some(Heading {
                    block,
                    level: *level,
                    title: SharedString::from(text.clone()),
                }),
                _ => None,
            })
            .collect();
        // New block count, fresh (unmeasured) items.
        self.list.reset(self.blocks.len());
        self.code_scrolls.borrow_mut().clear();
        self.prose_layouts.borrow_mut().clear();
        self.selection_anchor = None;
        self.selection_head = None;
        self.dragging_selection = false;
        self.active_heading = active_heading_block(&self.headings, 0);
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn outline_items(&self) -> Vec<(usize, u8, SharedString, bool)> {
        self.headings
            .iter()
            .map(|heading| {
                (
                    heading.block,
                    heading.level,
                    heading.title.clone(),
                    Some(heading.block) == self.active_heading,
                )
            })
            .collect()
    }

    pub fn linked_files(&self) -> Vec<PathBuf> {
        self.links.clone()
    }

    pub fn scroll_to_block(&mut self, block: usize, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: block,
            offset_in_item: px(0.),
        });
        self.set_active_heading(Some(block), cx);
    }

    fn publish_active_heading(&mut self, cx: &mut Context<Self>) {
        let top = self.list.logical_scroll_top().item_ix;
        let active = active_heading_block(&self.headings, top);
        self.set_active_heading(active, cx);
    }

    fn set_active_heading(&mut self, block: Option<usize>, cx: &mut Context<Self>) {
        if self.active_heading == block {
            return;
        }
        self.active_heading = block;
        cx.emit(ActiveHeadingChanged { block });
    }

    fn selection(&self) -> Option<(ProsePosition, ProsePosition)> {
        let anchor = self.selection_anchor?;
        let head = self.selection_head?;
        (anchor != head).then(|| {
            if anchor <= head {
                (anchor, head)
            } else {
                (head, anchor)
            }
        })
    }

    fn prose_position_at(&self, point: Point<Pixels>, strict: bool) -> Option<ProsePosition> {
        let layouts = self.prose_layouts.borrow();
        let mut layouts: Vec<&ProseLayout> = layouts.values().collect();
        layouts.sort_by_key(|layout| (layout.block, layout.range_start));

        let exact = layouts
            .iter()
            .copied()
            .find(|layout| layout.bounds.contains(&point));
        let layout = if let Some(layout) = exact {
            layout
        } else if strict {
            return None;
        } else {
            let first = *layouts.first()?;
            let last = *layouts.last()?;
            if point.y < first.bounds.top() {
                return Some(ProsePosition {
                    block: first.block,
                    byte: first.range_start,
                });
            }
            if point.y > last.bounds.bottom() {
                return Some(ProsePosition {
                    block: last.block,
                    byte: last.range_start + last.text.len(),
                });
            }
            layouts
                .iter()
                .copied()
                .min_by(|left, right| {
                    distance_to_bounds(point, left.bounds)
                        .total_cmp(&distance_to_bounds(point, right.bounds))
                })?
        };

        let byte = layout
            .layout
            .index_for_position(point)
            .unwrap_or_else(|nearest| nearest)
            .min(layout.text.len())
            + layout.range_start;
        Some(ProsePosition {
            block: layout.block,
            byte,
        })
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(position) = self.prose_position_at(event.position, true) else {
            return;
        };
        self.selection_anchor = Some(position);
        self.selection_head = Some(position);
        self.dragging_selection = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging_selection {
            return;
        }
        if let Some(position) = self.prose_position_at(event.position, false) {
            self.selection_head = Some(position);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging_selection {
            return;
        }
        if let Some(position) = self.prose_position_at(event.position, false) {
            self.selection_head = Some(position);
        }
        self.dragging_selection = false;
        if self.selection().is_none() {
            self.selection_anchor = None;
            self.selection_head = None;
        }
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.keystroke.modifiers.platform {
            return;
        }
        match event.keystroke.key.as_str() {
            "c" => {
                if let Some(text) = selected_prose_text(&self.blocks, self.selection()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "a" => {
                let first = self.blocks.iter().enumerate().find_map(|(block, item)| {
                    block_selectable_text(item).map(|_| ProsePosition { block, byte: 0 })
                });
                let last = self
                    .blocks
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(block, item)| {
                        block_selectable_text(item).map(|text| ProsePosition {
                            block,
                            byte: text.len(),
                        })
                    });
                if let (Some(first), Some(last)) = (first, last) {
                    self.selection_anchor = Some(first);
                    self.selection_head = Some(last);
                    cx.notify();
                }
            }
            _ => {}
        }
    }
}

impl EventEmitter<ActiveHeadingChanged> for MarkdownView {}

impl Focusable for MarkdownView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MarkdownView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let gh = gh_palette(cx.tokens().dark);
        let zoom = cx.global::<EditorZoom>().0;
        let viewport_padding = VIEWPORT_PADDING;
        let measure = self.measure.clone();
        self.prose_layouts.borrow_mut().clear();
        let prose_layouts = self.prose_layouts.clone();
        let scroll_view = cx.entity();

        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(gh.bg)
            .text_color(gh.fg)
            .track_focus(&self.focus)
            .key_context("MarkdownPreview")
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_scroll_wheel(move |_, window, cx| {
                prose_layouts.borrow_mut().clear();
                let scroll_view = scroll_view.clone();
                window.defer(cx, move |_, cx| {
                    let _ = scroll_view.update(cx, |view, cx| {
                        view.publish_active_heading(cx);
                    });
                });
            })
            // A scrolling box is sized by its content on the cross axis, so a
            // plain `flex_1` column grows to the widest table instead of
            // wrapping it. Giving the viewport an absolute frame inside a
            // flex-sized box hands it a definite width to lay out against.
            .child({
                let view = cx.entity();
                let colors = c;
                div().flex_1().min_w(px(0.)).relative().child(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .px(viewport_padding)
                        .pt(viewport_padding)
                        .child(
                            // A flow child measures the content box. An
                            // absolute one resolves `size_full` against the
                            // padding box, which reports 2 * XL too much and
                            // pushes every definite-width table past its card.
                            // Zero height keeps it out of the layout.
                            canvas(
                                move |bounds, _, _| measure.set(bounds.size.width),
                                |_, _, _, _| {},
                            )
                            .w_full()
                            .h(px(0.))
                            .flex_none(),
                        )
                        // The list is the scroller now. It reads blocks and the
                        // measured width straight off the entity, so no snapshot
                        // is cloned into the closure each frame.
                        .child(
                            list(self.list.clone(), move |index, _window, cx| {
                                let view_entity = view.clone();
                                let view = view_entity.read(cx);
                                match view.blocks.get(index) {
                                    Some(block) => {
                                        let scroll = matches!(block, Block::Code(_)).then(|| {
                                            view.code_scrolls
                                                .borrow_mut()
                                                .entry(index)
                                                .or_default()
                                                .clone()
                                        });
                                        let prose_state = ProseRenderState {
                                            layouts: view.prose_layouts.clone(),
                                            selection: view.selection(),
                                            selection_color: colors.text_selection.opacity(0.22),
                                        };
                                        render_block(
                                            index,
                                            block,
                                            &colors,
                                            &gh,
                                            view.measure.get(),
                                            zoom,
                                            scroll,
                                            &prose_state,
                                        )
                                    }
                                    None => div().into_any_element(),
                                }
                            })
                            .flex_1()
                            .min_h(px(0.)),
                        ),
                )
            })
    }
}

fn active_heading_block(headings: &[Heading], top: usize) -> Option<usize> {
    headings
        .iter()
        .rfind(|heading| heading.block <= top)
        .map(|heading| heading.block)
}

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[test]
    fn heading_before_viewport_is_not_marked_active() {
        let headings = vec![
            Heading {
                block: 3,
                level: 1,
                title: SharedString::from("First"),
            },
            Heading {
                block: 8,
                level: 2,
                title: SharedString::from("Second"),
            },
        ];

        assert_eq!(active_heading_block(&headings, 0), None);
        assert_eq!(active_heading_block(&headings, 7), Some(3));
        assert_eq!(active_heading_block(&headings, 8), Some(8));
    }

    #[test]
    fn selection_flattens_inline_markdown_across_prose_blocks() {
        let blocks = vec![
            Block::Heading(1, "Hello **world**".to_string()),
            Block::Paragraph("Second line".to_string()),
        ];
        let selection = Some((
            ProsePosition { block: 0, byte: 6 },
            ProsePosition { block: 1, byte: 6 },
        ));

        assert_eq!(
            selected_prose_text(&blocks, selection).as_deref(),
            Some("world\nSecond")
        );
    }

    #[test]
    fn selection_flattens_table_as_tsv_without_wrap_hints() {
        let rows = vec![
            vec!["Gate".to_string(), "Verdict".to_string()],
            vec!["Viet\u{200b}namese".to_string(), "Go".to_string()],
        ];
        let text = table_text(&rows);
        let blocks = vec![Block::Table(rows.clone())];
        let selection = Some((
            ProsePosition { block: 0, byte: 0 },
            ProsePosition {
                block: 0,
                byte: text.len(),
            },
        ));

        assert_eq!(
            table_cell_ranges(&rows),
            vec![vec![0..4, 5..12], vec![13..26, 27..29]]
        );
        assert_eq!(
            selected_prose_text(&blocks, selection).as_deref(),
            Some("Gate\tVerdict\nVietnamese\tGo")
        );
    }
}

fn local_links(source: &str, document: &Path) -> Vec<PathBuf> {
    let Some(parent) = document.parent() else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for event in Parser::new_ext(source, Options::all()) {
        let Event::Start(Tag::Link { dest_url, .. }) = event else {
            continue;
        };
        let target = dest_url.split('#').next().unwrap_or_default();
        if target.is_empty()
            || target.contains("://")
            || target.starts_with("mailto:")
            || target.starts_with('#')
        {
            continue;
        }
        let path = parent.join(target);
        if path.is_file() && !links.contains(&path) {
            links.push(path);
        }
    }
    links
}

fn block_inline_source(block: &Block) -> Option<&str> {
    match block {
        Block::Heading(_, text)
        | Block::Paragraph(text)
        | Block::ListItem(_, text, _, _)
        | Block::Quote(text) => Some(text),
        Block::Code(_) | Block::Rule | Block::Table(_) => None,
    }
}

fn flat_inline_text(source: &str) -> String {
    let gh = gh_palette(false);
    inline_runs(source, &gh, gh.fg, gh.fg, gpui::FontWeight::NORMAL).0
}

fn table_text(rows: &[Vec<String>]) -> String {
    rows.iter()
        .map(|row| row.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_selectable_text(block: &Block) -> Option<String> {
    if let Some(source) = block_inline_source(block) {
        return Some(flat_inline_text(source));
    }
    match block {
        Block::Table(rows) => Some(table_text(rows)),
        Block::Code(_) | Block::Rule => None,
        Block::Heading(_, _)
        | Block::Paragraph(_)
        | Block::ListItem(_, _, _, _)
        | Block::Quote(_) => None,
    }
}

fn selected_prose_text(
    blocks: &[Block],
    selection: Option<(ProsePosition, ProsePosition)>,
) -> Option<String> {
    let selection = selection?;
    let mut selected = Vec::new();
    for (block, item) in blocks.iter().enumerate() {
        let Some(text) = block_selectable_text(item) else {
            continue;
        };
        let Some(range) = prose_selection_range(block, text.len(), Some(selection)) else {
            continue;
        };
        let Some(part) = text.get(range) else {
            continue;
        };
        selected.push(part.replace('\u{200b}', ""));
    }
    (!selected.is_empty()).then(|| selected.join("\n"))
}

/// Inline Markdown as one selectable, wrapped `StyledText` with per-run fonts.
/// Selection is painted from the exact `TextLayout`, so enabling it does not
/// replace or restyle the Primer rendering.
fn prose_text(
    index: usize,
    text: String,
    gh: &Gh,
    color: Hsla,
    link: Hsla,
    weight: gpui::FontWeight,
    state: &ProseRenderState,
) -> impl IntoElement {
    let (flat, runs) = inline_runs(&text, gh, color, link, weight);
    let text = SharedString::from(flat);
    let range = 0..text.len();
    let selection = selection_range_for_segment(index, &range, state.selection);
    let styled_text = StyledText::new(text.clone()).with_runs(runs);
    div()
        .w_full()
        .cursor_text()
        .child(SelectableStyledText::new(
            index,
            range,
            text,
            styled_text,
            state,
            selection,
        ))
}

fn inline_runs(
    source: &str,
    gh: &Gh,
    color: Hsla,
    link: Hsla,
    weight: gpui::FontWeight,
) -> (String, Vec<gpui::TextRun>) {
    use gpui::{FontStyle, FontWeight, StrikethroughStyle, TextRun, font};

    let mut flat = String::with_capacity(source.len() + 8);
    let mut runs: Vec<TextRun> = Vec::new();
    let (mut bold, mut italic, mut strike, mut in_link) = (false, false, false, false);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    for event in Parser::new_ext(source, options) {
        let (chunk, code): (String, bool) = match event {
            Event::Start(Tag::Strong) => {
                bold = true;
                continue;
            }
            Event::End(TagEnd::Strong) => {
                bold = false;
                continue;
            }
            Event::Start(Tag::Emphasis) => {
                italic = true;
                continue;
            }
            Event::End(TagEnd::Emphasis) => {
                italic = false;
                continue;
            }
            Event::Start(Tag::Strikethrough) => {
                strike = true;
                continue;
            }
            Event::End(TagEnd::Strikethrough) => {
                strike = false;
                continue;
            }
            Event::Start(Tag::Link { .. }) => {
                in_link = true;
                continue;
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
                continue;
            }
            Event::Text(text) => (text.to_string(), false),
            // Spaces inside the run give the chip its horizontal padding.
            Event::Code(text) => (format!(" {text} "), true),
            Event::SoftBreak => (" ".to_string(), false),
            Event::HardBreak => ("\n".to_string(), false),
            _ => continue,
        };
        if chunk.is_empty() {
            continue;
        }
        let mut face = if code {
            font("JetBrains Mono")
        } else {
            font(".SystemUIFont")
        };
        face.weight = if bold && !code { FontWeight::BOLD } else { weight };
        face.style = if italic && !code { FontStyle::Italic } else { FontStyle::Normal };
        runs.push(TextRun {
            len: chunk.len(),
            font: face,
            color: if in_link { link } else { color },
            background_color: code.then_some(gh.chip),
            underline: None,
            strikethrough: strike.then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
        });
        flat.push_str(&chunk);
    }
    (flat, runs)
}

struct SelectableStyledText {
    id: ElementId,
    block: usize,
    range_start: usize,
    text: SharedString,
    styled_text: StyledText,
    layouts: Rc<RefCell<HashMap<(usize, usize), ProseLayout>>>,
    selection: Option<Range<usize>>,
    selection_color: Hsla,
}

impl SelectableStyledText {
    fn new(
        block: usize,
        range: Range<usize>,
        text: SharedString,
        styled_text: StyledText,
        state: &ProseRenderState,
        selection: Option<Range<usize>>,
    ) -> Self {
        Self {
            id: ElementId::Name(format!("markdown-selectable-{block}-{}", range.start).into()),
            block,
            range_start: range.start,
            text,
            styled_text,
            layouts: state.layouts.clone(),
            selection,
            selection_color: state.selection_color,
        }
    }

    fn paint_selection(
        selection: &Range<usize>,
        layout: &TextLayout,
        bounds: Bounds<Pixels>,
        color: Hsla,
        window: &mut Window,
    ) {
        let Some(start) = layout.position_for_index(selection.start) else {
            return;
        };
        let Some(end) = layout.position_for_index(selection.end) else {
            return;
        };
        let line_height = layout.line_height();
        if start.y == end.y {
            window.paint_quad(quad(
                Bounds::from_corners(start, point(end.x, end.y + line_height)),
                px(0.),
                color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
            return;
        }

        window.paint_quad(quad(
            Bounds::from_corners(start, point(bounds.right(), start.y + line_height)),
            px(0.),
            color,
            Edges::default(),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
        if end.y > start.y + line_height {
            window.paint_quad(quad(
                Bounds::from_corners(
                    point(bounds.left(), start.y + line_height),
                    point(bounds.right(), end.y),
                ),
                px(0.),
                color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
        window.paint_quad(quad(
            Bounds::from_corners(
                point(bounds.left(), end.y),
                point(end.x, end.y + line_height),
            ),
            px(0.),
            color,
            Edges::default(),
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
    }
}

impl IntoElement for SelectableStyledText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableStyledText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled_text
            .request_layout(global_id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, request_layout, window, cx);
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        self.layouts.borrow_mut().insert(
            (self.block, self.range_start),
            ProseLayout {
                block: self.block,
                range_start: self.range_start,
                text: self.text.clone(),
                layout: layout.clone(),
                bounds,
            },
        );
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
        if let Some(selection) = &self.selection {
            Self::paint_selection(selection, &layout, bounds, self.selection_color, window);
        }
    }
}

fn prose_selection_range(
    block: usize,
    len: usize,
    selection: Option<(ProsePosition, ProsePosition)>,
) -> Option<Range<usize>> {
    selection_range_for_segment(block, &(0..len), selection)
}

fn selection_range_for_segment(
    block: usize,
    segment: &Range<usize>,
    selection: Option<(ProsePosition, ProsePosition)>,
) -> Option<Range<usize>> {
    let (start, end) = selection?;
    if block < start.block || block > end.block {
        return None;
    }
    let start = if block == start.block {
        start.byte.max(segment.start)
    } else {
        segment.start
    };
    let end = if block == end.block {
        end.byte.min(segment.end)
    } else {
        segment.end
    };
    if end <= start || start >= segment.end {
        return None;
    }
    Some((start - segment.start)..(end - segment.start))
}

fn table_cell_ranges(rows: &[Vec<String>]) -> Vec<Vec<Range<usize>>> {
    let mut cursor = 0;
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            if row_index > 0 {
                cursor += 1;
            }
            row.iter()
                .enumerate()
                .map(|(column, cell)| {
                    if column > 0 {
                        cursor += 1;
                    }
                    let start = cursor;
                    cursor += cell.len();
                    start..cursor
                })
                .collect()
        })
        .collect()
}

fn table_cell_text(
    block: usize,
    range: Range<usize>,
    text: String,
    state: &ProseRenderState,
) -> SelectableStyledText {
    let text = SharedString::from(text);
    let selection = selection_range_for_segment(block, &range, state.selection);
    let styled_text = StyledText::new(text.clone());
    SelectableStyledText::new(block, range, text, styled_text, state, selection)
}

fn render_block(
    index: usize,
    block: &Block,
    c: &Colors,
    gh: &Gh,
    measure: Pixels,
    zoom: f32,
    scroll: Option<ScrollHandle>,
    prose_state: &ProseRenderState,
) -> AnyElement {
    // Definite widths, not `w_full` plus `max_w`. Text is measured against the
    // width the parent proposes, which is the full column, and the maximum is
    // applied afterwards. A paragraph that wraps to two lines at 640 points
    // still reports the height of one line at 900, so the next block draws on
    // top of it and a long line is cut off at the column edge.
    let prose = px(f32::from(measure).min(PROSE_WIDTH));
    let bleed = px(f32::from(measure).min(BLEED_WIDTH));
    // Zoom scales the type and the rhythm derived from it; the column widths
    // and structural padding stay fixed, so bigger text simply wraps sooner.
    let ed = Type::EDITOR * zoom;

    match block.clone() {
        Block::Heading(level, text) => {
            // GitHub: h1 2em / h2 1.5em with a bottom hairline and 0.3em of
            // padding above it; h3 1.25 / h4 1 / h5 .875 / h6 .85 muted. All
            // weight 600, line-height 1.25, 24 above, 16 below.
            let ratio = heading_ratio(level);
            let size = ed * ratio;
            v_flex()
                .w(prose)
                .mx_auto()
                // Roomier than Primer's flat 24: section headings (h1/h2) get
                // 2em above so long documents breathe between sections.
                // Padding, not margin: the virtualised list measures the
                // border box, so a margin is dropped from the item height and
                // the next block draws into it.
                .pt(if index == 0 {
                    px(0.)
                } else if level <= 2 {
                    ed * 2.
                } else {
                    ed * 1.5
                })
                .pb(ed)
                .child(
                    div()
                        .w_full()
                        .when(level <= 2, |this| {
                            this.pb(size * 0.3).border_b_1().border_color(gh.border)
                        })
                        .text_size(size)
                        .line_height(size * 1.25)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(prose_text(
                            index,
                            text,
                            gh,
                            if level == 6 { gh.muted } else { gh.fg },
                            c.link,
                            gpui::FontWeight::SEMIBOLD,
                            prose_state,
                        )),
                )
                .into_any_element()
        }
        Block::Paragraph(text) => div()
            .w(prose)
            .mx_auto()
            .pb(ed)
            .text_size(ed)
            .line_height(ed * 1.6)
            .child(prose_text(
                index,
                text,
                gh,
                gh.fg,
                c.link,
                gpui::FontWeight::NORMAL,
                prose_state,
            ))
            .into_any_element(),
        Block::ListItem(depth, text, checked, ordinal) => h_flex()
            .w(prose)
            .mx_auto()
            .items_start()
            // GitHub indents the list body 2em per level with the marker
            // hanging inside it, so even the first level starts 1em in.
            .pl(px(16. + depth as f32 * 32.))
            .pb(ed * 0.375)
            .gap(Space::S)
            .child(match checked {
                // GitHub draws the native disabled checkbox: a 13-point square
                // with a 3-point radius, blue with a white check when done.
                Some(done) => div()
                    .size(px(13.))
                    .flex_none()
                    .mt(px(((f32::from(ed) * 1.6) - 13.).max(0.) / 2.))
                    .rounded(px(3.))
                    .when(!done, |this| this.border_1().border_color(gh.border))
                    .when(done, |this| {
                        this.bg(gh.checked)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .text_color(gpui::white())
                            .child("\u{2713}")
                    })
                    .into_any_element(),
                None if ordinal.is_some() => div()
                    .w(px(22.))
                    .flex_none()
                    .text_right()
                    .text_size(ed)
                    .line_height(ed * 1.6)
                    .child(SharedString::from(format!(
                        "{}.",
                        ordinal.unwrap_or_default()
                    )))
                    .into_any_element(),
                // A drawn disc, not a glyph: the bullet characters render at a
                // fraction of the type size and read as flecks. GitHub's
                // markers are solid disc, hollow circle, then square by depth.
                None => div()
                    .w(px(16.))
                    .h(ed * 1.6)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match depth {
                        0 => div().size(px(6.)).rounded_full().bg(gh.fg),
                        1 => div()
                            .size(px(6.))
                            .rounded_full()
                            .border_1()
                            .border_color(gh.fg),
                        _ => div().size(px(5.)).bg(gh.fg),
                    })
                    .into_any_element(),
            })
            .child(
                // `min_w(0)` beats the automatic minimum, which is the width of
                // the text on one line. Without it the item never wraps and the
                // tail runs past the column.
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(ed)
                    .line_height(ed * 1.6)
                    .child(prose_text(
                        index,
                        text,
                        gh,
                        gh.fg,
                        c.link,
                        gpui::FontWeight::NORMAL,
                        prose_state,
                    )),
            )
            .into_any_element(),
        // No `overflow_hidden` on either card: clipping a card whose rows are
        // sized by wrapped text collapses it to nothing inside the scrolling
        // document. The rounded corners lose their clip; the content stays.
        Block::Code(code) => {
            render_code_block(index, code, c, gh, bleed, ed, scroll.unwrap_or_default())
        }
        // GitHub: 0.25em grey rule at the left, 1em of text inset, muted
        // upright text.
        Block::Quote(text) => h_flex()
            .w(prose)
            .mx_auto()
            .pb(ed)
            .items_stretch()
            .child(div().w(px(4.)).flex_none().bg(gh.border))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(ed)
                    .text_size(ed)
                    .line_height(ed * 1.6)
                    .child(prose_text(
                        index,
                        text,
                        gh,
                        gh.muted,
                        c.link,
                        gpui::FontWeight::NORMAL,
                        prose_state,
                    )),
            )
            .into_any_element(),
        // GitHub: a 0.25em solid bar with 24 points of vertical margin.
        Block::Rule => div()
            .w(prose)
            .mx_auto()
            .py(ed * 1.5)
            .child(div().w_full().h(px(4.)).bg(gh.border))
            .into_any_element(),
        Block::Table(rows) => {
            let ranges = table_cell_ranges(&rows);
            let mut iter = rows.into_iter().zip(ranges);
            let (header, header_ranges) = iter.next().unwrap_or_default();
            let body: Vec<(Vec<String>, Vec<Range<usize>>)> = iter.collect();
            // Each column takes a definite share of the card. A flex-sized cell
            // has no intrinsic width to grow from, so the row collapses and the
            // whole table disappears.

            let columns = header
                .len()
                .max(
                    body.iter()
                        .map(|(row, _)| row.len())
                        .max()
                        .unwrap_or(0),
                )
                .max(1);
            // Definite pixel widths, resolved from the card the cells sit in,
            // not from the document column. The card stops growing at
            // `BLEED_WIDTH`, so a share taken from a wider measure pushes the
            // trailing cells past the border. The card takes the same definite
            // width, and the last column absorbs the rounding remainder, so the
            // columns always add up to exactly the space inside the border.
            let card = f32::from(measure).min(BLEED_WIDTH).max(2.);
            let inner = card - 2.;
            let base = (inner / columns as f32).floor().max(1.);
            let last = (inner - base * (columns - 1) as f32).max(1.);
            let width = move |index: usize| px(if index + 1 == columns { last } else { base });

            // GitHub: 6x13-point cell padding, 1-point grid borders, a plain
            // bold header row, and every second body row on the subtle fill.
            let gh = *gh;
            let cell_style = move |element: gpui::Div, column: usize| {
                element
                    .w(width(column))
                    .flex_none()
                    .px(px(13.))
                    .py(px(9.))
                    .text_size(ed)
                    .line_height(ed * 1.5)
                    .cursor_text()
                    .when(column > 0, |this| {
                        this.border_l_1().border_color(gh.border)
                    })
            };
            div()
                .w(px(card))
                .mx_auto()
                .pb(ed)
                .child(
                    v_flex()
                        .w_full()
                        .flex_none()
                        .border_1()
                        .border_color(gh.border)
                        .child(h_flex().w_full().items_stretch().children(
                            header
                                .into_iter()
                                .zip(header_ranges)
                                .enumerate()
                                .map(|(column, (cell, range))| {
                                    cell_style(div(), column)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(table_cell_text(index, range, cell, prose_state))
                                }),
                        ))
                        .children(body.into_iter().enumerate().map(
                            |(row_index, (row, ranges))| {
                                // `items_stretch`: a cell sized to its own text leaves the
                                // column border one line tall; stretched cells carry the
                                // border the full row height like a real table grid.
                                h_flex()
                                    .w_full()
                                    .items_stretch()
                                    .border_t_1()
                                    .border_color(gh.border)
                                    .when(row_index % 2 == 1, |this| this.bg(gh.alt_row))
                                    .children(row.into_iter().zip(ranges).enumerate().map(
                                        |(column, (cell, range))| {
                                            cell_style(div(), column).child(table_cell_text(
                                                index,
                                                range,
                                                cell,
                                                prose_state,
                                            ))
                                        },
                                    ))
                            },
                        )),
                )
                .into_any_element()
        }
    }
}

/// GitHub heading sizes as ratios of the 16-point body: h1 2em, h2 1.5em,
/// h3 1.25em, h4 1em, h5 .875em, h6 .85em.
fn heading_ratio(level: u8) -> f32 {
    match level {
        1 => 2.,
        2 => 1.5,
        3 => 1.25,
        4 => 1.,
        5 => 0.875,
        _ => 0.85,
    }
}

/// GitHub code block: one flat card on the subtle fill, 16 points of inset, a
/// 6-point radius, 85% mono type at 1.45 line-height, no header band and no
/// line numbers. The copy control floats at the top-right like github.com.
fn render_code_block(
    index: usize,
    code: HighlightedCode,
    c: &Colors,
    gh: &Gh,
    width: Pixels,
    editor_size: Pixels,
    scroll: ScrollHandle,
) -> AnyElement {
    let copy_value = SharedString::from(code.source.clone());
    let spans = code
        .highlighter
        .spans_in(&code.source, 0..code.source.len(), &code.line_starts, c);
    let mono = editor_size * 0.85;

    div()
        .w(width)
        .mx_auto()
        .pb(editor_size)
        .relative()
        .child(
            div()
                .id(format!("markdown-code-scroll-{index}"))
                .w_full()
                .rounded(px(6.))
                .bg(gh.code_bg)
                // A block container stretches its child to its own width, so
                // the content never exceeds the bounds and there is nothing
                // to scroll. A row flex lets the `flex_none` column take its
                // max-content width and overflow.
                .flex()
                .flex_row()
                .items_start()
                .overflow_x_scroll()
                .track_scroll(&scroll)
                .child(
                    v_flex()
                        .flex_none()
                        .p(px(16.))
                        .font_family("JetBrains Mono")
                        .text_size(mono)
                        .line_height(mono * 1.45)
                        .children(code.source.lines().enumerate().map(|(line_index, line)| {
                            let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = spans
                                .get(&line_index)
                                .map(|line_spans| {
                                    line_spans
                                        .iter()
                                        .filter(|span| {
                                            span.start < span.end && span.end <= line.len()
                                        })
                                        .map(|span| {
                                            (
                                                span.start..span.end,
                                                HighlightStyle {
                                                    color: Some(span.color),
                                                    ..Default::default()
                                                },
                                            )
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            div().flex_none().whitespace_nowrap().child(
                                StyledText::new(SharedString::from(line.to_string()))
                                    .with_highlights(highlights),
                            )
                        })),
                ),
        )
        // The scrollbar overlays the card, not the padded wrapper, so its
        // thumb sits on the card's bottom edge.
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom(editor_size)
                .child(Scrollbar::horizontal(&scroll).id(format!("markdown-code-scrollbar-{index}"))),
        )
        .child(
            div().absolute().top(px(8.)).right(px(8.)).child(
                Clipboard::new(format!("markdown-code-copy-{index}"))
                    .value(copy_value)
                    .tooltip("Copy code"),
            ),
        )
        .into_any_element()
}

fn fence_language(language: &str) -> Lang {
    let normalized = language
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "rust" | "rs" => Lang::Rust,
        "swift" => Lang::Swift,
        "python" | "py" => Lang::Python,
        "javascript" | "js" | "jsx" => Lang::JavaScript,
        "typescript" | "ts" => Lang::TypeScript,
        "tsx" => Lang::Tsx,
        "json" | "jsonc" => Lang::Json,
        "toml" => Lang::Toml,
        "css" => Lang::Css,
        "html" | "htm" => Lang::Html,
        "bash" | "sh" | "shell" | "zsh" => Lang::Bash,
        "yaml" | "yml" => Lang::Yaml,
        _ => Lang::None,
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;

    #[test]
    fn heading_hierarchy_matches_the_rendering_spec() {
        assert_eq!(heading_ratio(1), 2.);
        assert_eq!(heading_ratio(2), 1.5);
        assert_eq!(heading_ratio(3), 1.25);
        assert_eq!(heading_ratio(4), 1.);
        assert_eq!(heading_ratio(5), 0.875);
        assert_eq!(heading_ratio(6), 0.85);
    }

    #[test]
    fn fenced_language_aliases_use_the_shared_highlighter() {
        assert_eq!(fence_language("rust"), Lang::Rust);
        assert_eq!(fence_language("js title=demo"), Lang::JavaScript);
        assert_eq!(fence_language("zsh"), Lang::Bash);
        assert_eq!(fence_language("unknown"), Lang::None);
    }

    #[test]
    fn ordered_items_carry_their_ordinals_and_bullets_do_not() {
        let blocks = parse("3. three\n4. four\n\n- dash\n\n1. one\n");
        let ordinals: Vec<Option<u64>> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::ListItem(_, _, _, ordinal) => Some(*ordinal),
                _ => None,
            })
            .collect();

        assert_eq!(ordinals, vec![Some(3), Some(4), None, Some(1)]);
    }

    #[test]
    fn soft_wrapped_prose_joins_into_one_line() {
        let blocks = parse("first line of a paragraph\nsecond line, same paragraph\n");
        let Some(Block::Paragraph(text)) = blocks.first() else {
            panic!("expected a paragraph");
        };
        assert_eq!(text, "first line of a paragraph second line, same paragraph");
    }

    #[test]
    fn hard_breaks_and_item_paragraphs_survive_the_join() {
        assert_eq!(join_soft_wraps("kept  \nbreak", false), "kept  \nbreak");
        assert_eq!(join_soft_wraps("kept\\\nbreak", false), "kept\\\nbreak");
        assert_eq!(join_soft_wraps("one\n\n  two", false), "one\n\ntwo");
        assert_eq!(join_soft_wraps("quote line\n> continues", true), "quote line continues");
    }

    #[test]
    fn supported_fenced_code_caches_syntax_spans() {
        let code = HighlightedCode::new("rust".into(), "fn main() { let answer = 42; }".into());
        let colors = Colors::for_test();
        let spans = code.highlighter.spans_in(
            &code.source,
            0..code.source.len(),
            &code.line_starts,
            &colors,
        );

        assert!(spans.values().any(|line| !line.is_empty()));
    }
}

/// A `raw` slice keeps the source's soft line wraps, and `TextView` renders a
/// newline as a line break, so a hard-wrapped paragraph would keep its
/// authoring width instead of filling the prose measure. Join those wraps into
/// single spaces. A Markdown hard break - two trailing spaces or a backslash -
/// keeps its newline. A quote slice carries the `>` marker of every
/// continuation line; `quoted` drops it so the reparse sees only the text.
fn join_soft_wraps(inline: &str, quoted: bool) -> String {
    let mut out = String::with_capacity(inline.len());
    for (index, line) in inline.lines().enumerate() {
        let line = if quoted && index > 0 {
            line.trim_start()
                .trim_start_matches('>')
                .trim_start_matches([' ', '\t'])
        } else {
            line
        };
        if index == 0 {
            out.push_str(line);
            continue;
        }
        // A blank line separates paragraphs inside one block (a multi-paragraph
        // list item); keep that break instead of joining across it.
        if line.trim().is_empty() {
            if !out.ends_with("\n\n") {
                out.push_str("\n\n");
            }
            continue;
        }
        if out.ends_with("\n\n") {
            // First line after a paragraph break: nothing to join onto.
        } else if out.ends_with("  ") || out.ends_with('\\') {
            out.push('\n');
        } else {
            out.truncate(out.trim_end_matches([' ', '\t']).len());
            out.push(' ');
        }
        out.push_str(line.trim_start_matches([' ', '\t']));
    }
    out
}

fn parse(source: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut blocks = Vec::new();
    // Plain text, still used for the code cards and table cells.
    let mut text = String::new();
    // Byte range of the current prose block's inline content, sliced back out
    // of `source` so `TextView` reparses the raw Markdown (keeping emphasis).
    // Inline events carry the marker characters in their range; the leading
    // `#`/`-` of a heading or list marker sits on the container Start, which is
    // never folded in, so the slice stays free of block markers.
    let mut span: Option<(usize, usize)> = None;
    // One counter per open list: `Some` carries the next ordinal of an ordered
    // list, `None` marks an unordered one. Depth is the stack height.
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut task: Option<bool> = None;
    let mut in_code: Option<String> = None;
    let mut in_quote = false;
    let mut in_item = false;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_row: Vec<String> = Vec::new();

    let raw = |span: Option<(usize, usize)>| -> String {
        span.and_then(|(start, end)| source.get(start..end))
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. })
            | Event::Start(Tag::Paragraph)
            | Event::Start(Tag::Item) => {
                text.clear();
                span = None;
                if matches!(event, Event::Start(Tag::Item)) {
                    in_item = true;
                    task = None;
                }
            }
            Event::Start(Tag::List(start)) => list_counters.push(start),
            Event::End(TagEnd::List(_)) => {
                list_counters.pop();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                in_quote = true;
                text.clear();
                span = None;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                in_quote = false;
                let inline = raw(span);
                if !inline.is_empty() {
                    blocks.push(Block::Quote(join_soft_wraps(&inline, true)));
                }
                text.clear();
                span = None;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code = Some(match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                });
                text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(lang) = in_code.take() {
                    blocks.push(Block::Code(HighlightedCode::new(
                        lang,
                        text.trim_end().to_string(),
                    )));
                }
                text.clear();
            }
            Event::Start(Tag::Table(_)) => table = Some(Vec::new()),
            Event::End(TagEnd::Table) => {
                if let Some(rows) = table.take() {
                    blocks.push(Block::Table(rows));
                }
            }
            Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => table_row.clear(),
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                if let Some(rows) = table.as_mut() {
                    rows.push(std::mem::take(&mut table_row));
                }
            }
            Event::Start(Tag::TableCell) => text.clear(),
            Event::End(TagEnd::TableCell) => table_row.push(breakable(text.trim())),
            Event::End(TagEnd::Heading(level)) => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                blocks.push(Block::Heading(level, raw(span)));
                text.clear();
                span = None;
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_quote && !in_item {
                    let inline = raw(span);
                    if !inline.is_empty() {
                        blocks.push(Block::Paragraph(join_soft_wraps(&inline, false)));
                    }
                    text.clear();
                    span = None;
                }
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                let inline = raw(span);
                let ordinal = list_counters.last_mut().and_then(|counter| {
                    let value = (*counter)?;
                    *counter = Some(value + 1);
                    Some(value)
                });
                if !inline.is_empty() {
                    let depth = list_counters.len().saturating_sub(1);
                    blocks.push(Block::ListItem(
                        depth,
                        join_soft_wraps(&inline, false),
                        task,
                        ordinal,
                    ));
                }
                text.clear();
                span = None;
            }
            Event::Rule => blocks.push(Block::Rule),
            // The checkbox is drawn from `task`, so the `[x]` marker must stay
            // out of the inline slice.
            Event::TaskListMarker(done) => task = Some(done),
            Event::Text(chunk) => {
                text.push_str(&chunk);
                extend(&mut span, &range);
            }
            Event::Code(chunk) => {
                text.push_str(&chunk);
                extend(&mut span, &range);
            }
            Event::SoftBreak => {
                text.push(' ');
                extend(&mut span, &range);
            }
            Event::HardBreak => {
                text.push('\n');
                extend(&mut span, &range);
            }
            // Emphasis, strong, strikethrough and links: the Start range spans
            // the whole run including its markers, so folding it in keeps the
            // syntax the reparse needs.
            Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. }) => {
                extend(&mut span, &range);
            }
            _ => {}
        }
    }

    blocks
}

/// Grows `span` to cover `range`, seeding it on the first inline event.
fn extend(span: &mut Option<(usize, usize)>, range: &std::ops::Range<usize>) {
    *span = Some(match *span {
        Some((start, end)) => (start.min(range.start), end.max(range.end)),
        None => (range.start, range.end),
    });
}

#[cfg(test)]
pub fn parse_summary(source: &str) -> Vec<(&'static str, usize)> {
    parse(source)
        .into_iter()
        .map(|block| match block {
            Block::Heading(level, _) => ("heading", level as usize),
            Block::Paragraph(_) => ("paragraph", 0),
            Block::ListItem(depth, _, _, _) => ("list", depth),
            Block::Code(code) => ("code", code.source.lines().count()),
            Block::Quote(_) => ("quote", 0),
            Block::Rule => ("rule", 0),
            Block::Table(rows) => ("table", rows.len()),
        })
        .collect()
}

/// Inserts zero-width spaces inside very long unbroken tokens.
///
/// GPUI wraps on word boundaries only, so a commit hash or a long path in a
/// narrow table cell would run past its column. `DESIGN.md` asks for those to
/// wrap by character; this is the cheapest way to get that from the text layer
/// without a custom text element.
fn breakable(text: &str) -> String {
    const LIMIT: usize = 18;
    let mut out = String::with_capacity(text.len());
    for (index, token) in text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        if token.chars().count() <= LIMIT {
            out.push_str(token);
            continue;
        }
        for (position, ch) in token.chars().enumerate() {
            if position > 0 && position % LIMIT == 0 {
                out.push('\u{200b}');
            }
            out.push(ch);
        }
    }
    out
}
