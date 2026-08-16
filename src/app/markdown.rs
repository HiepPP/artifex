//! Markdown preview.
//!
//! Parses with `pulldown-cmark` and renders headings, prose, lists, tables,
//! code cards and quotes as GPUI elements. Prose blocks (heading, paragraph,
//! list item, quote) render through `gpui_component::text::TextView`, which
//! registers with the window-level selection system, so a drag across them
//! selects text and `cmd-c` copies it. That reparse also restores inline
//! emphasis, which the plain-text render used to drop. Code cards and tables
//! keep their bespoke layout and stay non-selectable for now.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, HighlightStyle, IntoElement, ListAlignment,
    ListOffset, ListState, ParentElement, Pixels, Render, SharedString, Styled as _, StyledText,
    Window, canvas, div, list, px, rems,
};
use gpui_component::clipboard::Clipboard;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{h_flex, v_flex};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::services::highlight::{Highlighter, Lang, line_starts};
use crate::theme::{
    ActiveTokens as _, Colors, EditorZoom, LayoutMode, Metrics, Radius, Space, Type,
};

/// `DESIGN.md` > documentMaxWidth / documentBleedMaxWidth.
const PROSE_WIDTH: f32 = 720.;
const BLEED_WIDTH: f32 = 880.;
const WIDE_VIEWPORT_PADDING: Pixels = px(48.);

#[derive(Clone)]
struct HighlightedCode {
    language: String,
    source: String,
    highlighter: Rc<Highlighter>,
    line_starts: Rc<Vec<usize>>,
}

impl HighlightedCode {
    fn new(language: String, source: String) -> Self {
        let mut highlighter = Highlighter::new(fence_language(&language));
        highlighter.parse(&source);
        Self {
            language,
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
    ListItem(usize, String, Option<bool>),
    Code(HighlightedCode),
    Quote(String),
    Rule,
    Table(Vec<Vec<String>>),
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
    /// Virtualised block list. Each prose block is now a `TextView`, far heavier
    /// than the plain-text `div` it replaced: laying every block out on each
    /// scroll frame dropped frames on long documents. `list` measures and paints
    /// only the blocks in the viewport (plus overdraw), so the scroll cost
    /// tracks the viewport, not the document length.
    list: ListState,
    /// Measured width of the document column.
    ///
    /// A table needs definite column widths: a flex or percentage cell inside a
    /// scrolling box has nothing to resolve against, so the row reports no
    /// height and the table disappears. The measure arrives one frame late,
    /// which is why it starts at the prose measure instead of zero.
    measure: Rc<Cell<Pixels>>,
}

impl MarkdownView {
    pub fn open(path: PathBuf, cx: &mut App) -> Entity<Self> {
        cx.new(|_| {
            let mut view = Self {
                path,
                blocks: Vec::new(),
                headings: Vec::new(),
                active_heading: None,
                links: Vec::new(),
                list: ListState::new(0, ListAlignment::Top, px(400.)),
                measure: Rc::new(Cell::new(px(PROSE_WIDTH))),
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
}

impl EventEmitter<ActiveHeadingChanged> for MarkdownView {}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let zoom = cx.global::<EditorZoom>().0;
        let window_width = f32::from(window.viewport_size().width) - f32::from(Metrics::RAIL_WIDTH);
        let viewport_padding = match LayoutMode::for_width(window_width) {
            LayoutMode::Wide => WIDE_VIEWPORT_PADDING,
            LayoutMode::Standard => Space::XXL,
            LayoutMode::Compact => Space::XL,
        };
        let measure = self.measure.clone();
        let scroll_view = cx.entity();

        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(c.editor)
            .on_scroll_wheel(move |_, window, cx| {
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
                                let view = view.read(cx);
                                match view.blocks.get(index) {
                                    Some(block) => render_block(
                                        index,
                                        block,
                                        &colors,
                                        view.measure.get(),
                                        zoom,
                                    ),
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

/// A selectable, formatted rendering of one prose block's inline Markdown.
///
/// The surrounding block sets the type token, color and measure; the body text
/// inherits those through the window text style. `paragraph_gap` is zeroed
/// because each block holds exactly one paragraph, so the kit's default trailing
/// gap would only add stray space under the block.
fn prose_text(index: usize, text: String) -> impl IntoElement {
    TextView::markdown(("md-prose", index), SharedString::from(text))
        .selectable(true)
        .style(TextViewStyle::default().paragraph_gap(rems(0.)))
}

fn render_block(index: usize, block: &Block, c: &Colors, measure: Pixels, zoom: f32) -> AnyElement {
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
    let micro = Type::MICRO * zoom;

    match block.clone() {
        Block::Heading(level, text) => {
            let (ratio, space_before, space_after) = heading_spec(level);
            v_flex()
                .w(prose)
                .mx_auto()
                .mt(if level == 1 && index == 0 {
                    px(0.)
                } else {
                    ed * space_before
                })
                .mb(ed * space_after)
                .child(
                    div()
                        .text_size(ed * ratio)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .when(level <= 2, |this| this.font_family("Times New Roman"))
                        .child(prose_text(index, text)),
                )
                .when(level <= 2, |this| {
                    this.child(
                        h_flex()
                            .mt(px(6.))
                            .h(px(1.))
                            .w_full()
                            .child(
                                div()
                                    .h_full()
                                    .w(px(if level == 1 { 64. } else { 32. }))
                                    .bg(c.accent),
                            )
                            .child(div().h_full().flex_1().bg(c.border)),
                    )
                })
                .into_any_element()
        }
        Block::Paragraph(text) => div()
            .w(prose)
            .mx_auto()
            .my(ed * 0.625)
            .text_size(ed)
            .line_height(ed * 1.62)
            .child(prose_text(index, text))
            .into_any_element(),
        Block::ListItem(depth, text, checked) => h_flex()
            .w(prose)
            .mx_auto()
            .items_start()
            .pl(px(depth as f32 * 24.))
            .gap(Space::S)
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .text_color(if checked == Some(true) {
                        c.workflow_done
                    } else {
                        c.ink_secondary
                    })
                    .child(match checked {
                        Some(true) => "\u{2611}",
                        Some(false) => "\u{2610}",
                        None => match depth {
                            0 => "\u{2022}",
                            1 => "\u{25e6}",
                            _ => "-",
                        },
                    }),
            )
            .child(
                // `min_w(0)` beats the automatic minimum, which is the width of
                // the text on one line. Without it the item never wraps and the
                // tail runs past the column.
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(ed)
                    .line_height(ed * 1.55)
                    .when(checked == Some(true), |this| {
                        this.text_color(c.ink_secondary).line_through()
                    })
                    .child(prose_text(index, text)),
            )
            .into_any_element(),
        // No `overflow_hidden` on either card: clipping a card whose rows are
        // sized by wrapped text collapses it to nothing inside the scrolling
        // document. The rounded corners lose their clip; the content stays.
        Block::Code(code) => render_code_block(index, code, c, bleed, ed, micro),
        Block::Quote(text) => h_flex()
            .w(prose)
            .mx_auto()
            .my(ed * 1.25)
            .items_stretch()
            .child(div().w(px(3.)).flex_none().bg(c.accent))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(Space::L)
                    .italic()
                    .text_color(c.ink_secondary)
                    .text_size(ed)
                    .line_height(ed * 1.62)
                    .child(prose_text(index, text)),
            )
            .into_any_element(),
        Block::Rule => h_flex()
            .w(prose)
            .mx_auto()
            .my(ed * 1.75)
            .justify_center()
            .gap(Space::S)
            .child(dot(c.border))
            .child(dot(c.accent))
            .child(dot(c.border))
            .into_any_element(),
        Block::Table(rows) => {
            let mut iter = rows.into_iter();
            let header = iter.next().unwrap_or_default();
            let body: Vec<Vec<String>> = iter.collect();
            // Each column takes a definite share of the card. A flex-sized cell
            // has no intrinsic width to grow from, so the row collapses and the
            // whole table disappears.

            let columns = header
                .len()
                .max(body.iter().map(Vec::len).max().unwrap_or(0))
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

            v_flex()
                .w(px(card))
                .flex_none()
                .mx_auto()
                .my(ed * 1.25)
                .rounded(Radius::CONTROL)
                .border_1()
                .border_color(c.border)
                .child(h_flex().w_full().items_start().bg(c.raised).children(
                    header.into_iter().enumerate().map(|(column, cell)| {
                        div()
                            .w(width(column))
                            .flex_none()
                            .p(Space::M)
                            .text_size(ed * 0.90625)
                            .line_height(ed * 0.90625 * 1.45)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(SharedString::from(cell))
                    }),
                ))
                .children(body.into_iter().enumerate().map(|(index, row)| {
                    h_flex()
                        .w_full()
                        .items_start()
                        .when(index % 2 == 1, |this| this.bg(c.panel))
                        .children(row.into_iter().enumerate().map(|(column, cell)| {
                            div()
                                .w(width(column))
                                .flex_none()
                                .p(Space::M)
                                .text_size(ed * 0.90625)
                                .line_height(ed * 0.90625 * 1.45)
                                .child(SharedString::from(cell))
                        }))
                }))
                .into_any_element()
        }
    }
}

fn heading_spec(level: u8) -> (f32, f32, f32) {
    match level {
        1 => (2.875, 3.5, 1.25),
        2 => (1.75, 3.5, 1.25),
        3 => (1.1875, 2., 0.75),
        4 => (1., 2., 0.75),
        _ => (0.9375, 2., 0.75),
    }
}

fn render_code_block(
    index: usize,
    code: HighlightedCode,
    c: &Colors,
    width: Pixels,
    editor_size: Pixels,
    label_size: Pixels,
) -> AnyElement {
    let label = if code.language.is_empty() {
        SharedString::from("code")
    } else {
        SharedString::from(code.language.clone())
    };
    let copy_value = SharedString::from(code.source.clone());
    let spans = code
        .highlighter
        .spans_in(&code.source, 0..code.source.len(), &code.line_starts, c);

    v_flex()
        .w(width)
        .mx_auto()
        .my(editor_size * 1.25)
        .rounded(Radius::CONTROL)
        .border_1()
        .border_color(c.border)
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .px(Space::M)
                .py(Space::XS)
                .bg(c.raised)
                .text_size(label_size)
                .text_color(c.ink_secondary)
                .child(label)
                .child(
                    Clipboard::new(format!("markdown-code-copy-{index}"))
                        .value(copy_value)
                        .tooltip("Copy code"),
                ),
        )
        .child(
            v_flex()
                .id(format!("markdown-code-scroll-{index}"))
                .w_full()
                .p(Space::M)
                .bg(c.panel)
                .font_family("JetBrains Mono")
                .text_size(editor_size * 0.90625)
                .line_height(editor_size * 0.90625 * 1.35)
                .overflow_x_scrollbar()
                .children(code.source.lines().enumerate().map(|(line_index, line)| {
                    let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = spans
                        .get(&line_index)
                        .map(|line_spans| {
                            line_spans
                                .iter()
                                .filter(|span| span.start < span.end && span.end <= line.len())
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

                    h_flex()
                        .flex_none()
                        .child(
                            div()
                                .w(px(36.))
                                .flex_none()
                                .pr(Space::S)
                                .text_right()
                                .text_color(c.ink_secondary.opacity(0.55))
                                .child(SharedString::from((line_index + 1).to_string())),
                        )
                        .child(
                            div().flex_none().whitespace_nowrap().child(
                                StyledText::new(SharedString::from(line.to_string()))
                                    .with_highlights(highlights),
                            ),
                        )
                })),
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
        assert_eq!(heading_spec(1), (2.875, 3.5, 1.25));
        assert_eq!(heading_spec(2), (1.75, 3.5, 1.25));
        assert_eq!(heading_spec(3), (1.1875, 2., 0.75));
        assert_eq!(heading_spec(6), (0.9375, 2., 0.75));
    }

    #[test]
    fn fenced_language_aliases_use_the_shared_highlighter() {
        assert_eq!(fence_language("rust"), Lang::Rust);
        assert_eq!(fence_language("js title=demo"), Lang::JavaScript);
        assert_eq!(fence_language("zsh"), Lang::Bash);
        assert_eq!(fence_language("unknown"), Lang::None);
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

fn dot(color: gpui::Hsla) -> impl IntoElement {
    div().size(px(4.)).rounded_full().bg(color)
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
    let mut list_depth = 0usize;
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
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => {
                in_quote = true;
                text.clear();
                span = None;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                in_quote = false;
                let inline = raw(span);
                if !inline.is_empty() {
                    blocks.push(Block::Quote(inline));
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
                        blocks.push(Block::Paragraph(inline));
                    }
                    text.clear();
                    span = None;
                }
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                let inline = raw(span);
                if !inline.is_empty() {
                    blocks.push(Block::ListItem(list_depth.saturating_sub(1), inline, task));
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
            Block::ListItem(depth, _, _) => ("list", depth),
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
