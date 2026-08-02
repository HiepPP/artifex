//! Markdown preview.
//!
//! Parses with `pulldown-cmark` and renders headings, prose, lists, tables,
//! code cards and quotes as GPUI elements. `DESIGN.md` also asks for one
//! selectable native document; GPUI has no equivalent of `NSTextStorage`, so
//! selection across blocks is out of scope here and recorded as a gap.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, ParentElement, Pixels, Render, ScrollHandle,
    SharedString, Styled as _, Window, canvas, div, px,
};
use gpui_component::{h_flex, v_flex};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::theme::{ActiveTokens as _, Colors, Radius, Space, Type};

/// `DESIGN.md` > documentMaxWidth / documentBleedMaxWidth.
const PROSE_WIDTH: f32 = 640.;
const BLEED_WIDTH: f32 = 1180.;

#[derive(Clone)]
enum Block {
    Heading(u8, String),
    Paragraph(String),
    ListItem(usize, String, Option<bool>),
    Code(String, String),
    Quote(String),
    Rule,
    Table(Vec<Vec<String>>),
}

/// `DESIGN.md` > markdownOutlineWidth.
const OUTLINE_WIDTH: f32 = 200.;
/// Below this the outline would leave no readable measure, so it hides.
const OUTLINE_MIN_HOST: f32 = 900.;

/// One heading in the "On This Page" rail.
struct Heading {
    block: usize,
    level: u8,
    title: SharedString,
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
    scroll: ScrollHandle,
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
                scroll: ScrollHandle::new(),
                measure: Rc::new(Cell::new(px(PROSE_WIDTH))),
            };
            view.reload();
            view
        })
    }

    pub fn reload(&mut self) {
        let source = std::fs::read_to_string(&self.path).unwrap_or_default();
        self.blocks = parse(&source);
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
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        // DESIGN.md hides the rail when the column is too narrow to keep a
        // readable measure, and needs at least two headings to be worth it.
        let shows_outline =
            self.headings.len() >= 2 && f32::from(window.viewport_size().width) >= OUTLINE_MIN_HOST;
        let measure = self.measure.clone();
        let column_width = self.measure.get();
        let top = self.scroll.top_item();
        let active = self
            .headings
            .iter()
            .rposition(|heading| heading.block <= top)
            .unwrap_or(0);

        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(c.editor)
            // A scrolling box is sized by its content on the cross axis, so a
            // plain `flex_1` column grows to the widest table instead of
            // wrapping it. Giving the viewport an absolute frame inside a
            // flex-sized box hands it a definite width to lay out against.
            .child(
                div().flex_1().min_w(px(0.)).relative().child(
                    v_flex()
                        .id("markdown-scroll")
                        .track_scroll(&self.scroll)
                        .absolute()
                        .inset_0()
                        .overflow_y_scroll()
                        .py(Space::XL)
                        .px(Space::XL)
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
                        .children(
                            self.blocks.iter().map(|block| {
                                render_block(block, &c, column_width).into_any_element()
                            }),
                        ),
                ),
            )
            .when(shows_outline, |this| {
                this.child(
                    v_flex()
                        .id("markdown-outline")
                        .w(px(OUTLINE_WIDTH))
                        .flex_none()
                        .overflow_y_scroll()
                        .py(Space::XL)
                        .pr(Space::L)
                        .gap(px(2.))
                        .child(
                            div()
                                .pb(Space::S)
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .child("ON THIS PAGE"),
                        )
                        .children(self.headings.iter().enumerate().map(|(index, heading)| {
                            let is_active = index == active;
                            let block = heading.block;
                            let scroll = self.scroll.clone();
                            h_flex()
                                .id(("outline", index))
                                .cursor_pointer()
                                .items_start()
                                .gap(Space::XS)
                                .py(px(2.))
                                .child(
                                    // A short accent indicator marks the active
                                    // row on the leading edge.
                                    div().w(px(2.)).h(px(14.)).flex_none().rounded(px(1.)).bg(
                                        if is_active {
                                            c.accent
                                        } else {
                                            gpui::transparent_black()
                                        },
                                    ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .pl(px((heading.level.saturating_sub(1) as f32) * 10.))
                                        .text_size(if heading.level <= 2 {
                                            Type::CAPTION
                                        } else {
                                            Type::MICRO
                                        })
                                        .text_color(if is_active {
                                            c.accent
                                        } else if heading.level <= 2 {
                                            c.ink_secondary
                                        } else {
                                            c.ink_secondary.opacity(0.7)
                                        })
                                        .hover(|this| this.text_color(c.ink))
                                        .child(heading.title.clone()),
                                )
                                .on_click(move |_, _, _| scroll.scroll_to_top_of_item(block))
                        })),
                )
            })
    }
}

fn render_block(block: &Block, c: &Colors, measure: Pixels) -> AnyElement {
    // Definite widths, not `w_full` plus `max_w`. Text is measured against the
    // width the parent proposes, which is the full column, and the maximum is
    // applied afterwards. A paragraph that wraps to two lines at 640 points
    // still reports the height of one line at 900, so the next block draws on
    // top of it and a long line is cut off at the column edge.
    let prose = px(f32::from(measure).min(PROSE_WIDTH));
    let bleed = px(f32::from(measure).min(BLEED_WIDTH));

    match block.clone() {
        Block::Heading(level, text) => {
            let ratio = match level {
                1 => 1.85,
                2 => 1.45,
                3 => 1.18,
                4 => 1.00,
                _ => 0.92,
            };
            v_flex()
                .w(prose)
                .mx_auto()
                .mt((Type::EDITOR * 1.4))
                .mb((Type::EDITOR * 0.5))
                .child(
                    div()
                        .text_size((Type::EDITOR * ratio))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .when(level == 3, |this| this.text_color(c.accent))
                        .child(SharedString::from(text)),
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
            .my((Type::EDITOR * 0.5))
            .text_size(Type::EDITOR)
            .line_height((Type::EDITOR * 1.62))
            .child(SharedString::from(text))
            .into_any_element(),
        Block::ListItem(depth, text, checked) => h_flex()
            .w(prose)
            .mx_auto()
            .items_start()
            .pl(px(8. + depth as f32 * 16.))
            .gap(Space::S)
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .text_color(c.ink_secondary)
                    .child(match checked {
                        Some(true) => "[x]",
                        Some(false) => "[ ]",
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
                    .text_size(Type::EDITOR)
                    .line_height((Type::EDITOR * 1.55))
                    .when(checked == Some(true), |this| {
                        this.text_color(c.ink_secondary).line_through()
                    })
                    .child(SharedString::from(text)),
            )
            .into_any_element(),
        // No `overflow_hidden` on either card: clipping a card whose rows are
        // sized by wrapped text collapses it to nothing inside the scrolling
        // document. The rounded corners lose their clip; the content stays.
        Block::Code(language, text) => v_flex()
            .w(bleed)
            .mx_auto()
            .my((Type::EDITOR * 1.25))
            .rounded(Radius::CONTROL)
            .border_1()
            .border_color(c.border)
            .child(
                h_flex()
                    .w_full()
                    .px(Space::M)
                    .py(Space::XS)
                    .bg(c.raised)
                    .text_size(Type::MICRO)
                    .text_color(c.ink_secondary)
                    .child(SharedString::from(if language.is_empty() {
                        "code".to_string()
                    } else {
                        language
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .p(Space::M)
                    .bg(c.panel)
                    .font_family("JetBrains Mono")
                    .text_size((Type::EDITOR * 0.92))
                    .line_height((Type::EDITOR * 1.35))
                    .children(text.lines().enumerate().map(|(index, line)| {
                        h_flex()
                            .child(
                                div()
                                    .w(px(36.))
                                    .flex_none()
                                    .pr(Space::S)
                                    .text_right()
                                    .text_color(c.ink_secondary.opacity(0.55))
                                    .child(SharedString::from((index + 1).to_string())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(SharedString::from(line.to_string())),
                            )
                    })),
            )
            .into_any_element(),
        Block::Quote(text) => h_flex()
            .w(prose)
            .mx_auto()
            .my((Type::EDITOR * 1.25))
            .items_stretch()
            .child(div().w(px(3.)).flex_none().bg(c.accent))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(Space::M)
                    .italic()
                    .text_color(c.ink_secondary)
                    .text_size(Type::EDITOR)
                    .child(SharedString::from(text)),
            )
            .into_any_element(),
        Block::Rule => h_flex()
            .w(prose)
            .mx_auto()
            .my((Type::EDITOR * 1.75))
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
            let width = move |index: usize| {
                px(if index + 1 == columns { last } else { base })
            };

            v_flex()
                .w(px(card))
                .flex_none()
                .mx_auto()
                .my(Type::EDITOR * 1.25)
                .rounded(Radius::CONTROL)
                .border_1()
                .border_color(c.border)
                .child(h_flex().w_full().items_start().bg(c.raised).children(
                    header.into_iter().enumerate().map(|(column, cell)| {
                        div()
                            .w(width(column))
                            .flex_none()
                            .px(Space::M)
                            .py(Space::S)
                            .text_size(Type::EDITOR * 0.9)
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
                                .px(Space::M)
                                .py(Space::S)
                                .text_size(Type::EDITOR * 0.9)
                                .line_height(Type::EDITOR * 1.45)
                                .child(SharedString::from(cell))
                        }))
                }))
                .into_any_element()
        }
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
    let mut text = String::new();
    let mut list_depth = 0usize;
    let mut task: Option<bool> = None;
    let mut in_code: Option<String> = None;
    let mut in_quote = false;
    let mut in_item = false;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut table_row: Vec<String> = Vec::new();

    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::Heading { .. })
            | Event::Start(Tag::Paragraph)
            | Event::Start(Tag::Item) => {
                text.clear();
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
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                in_quote = false;
                if !text.trim().is_empty() {
                    blocks.push(Block::Quote(text.trim().to_string()));
                }
                text.clear();
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
                    blocks.push(Block::Code(lang, text.trim_end().to_string()));
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
                blocks.push(Block::Heading(level, text.trim().to_string()));
                text.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_quote && !in_item && !text.trim().is_empty() {
                    blocks.push(Block::Paragraph(text.trim().to_string()));
                    text.clear();
                }
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                if !text.trim().is_empty() {
                    blocks.push(Block::ListItem(
                        list_depth.saturating_sub(1),
                        text.trim().to_string(),
                        task,
                    ));
                }
                text.clear();
            }
            Event::Rule => blocks.push(Block::Rule),
            Event::TaskListMarker(done) => task = Some(done),
            Event::Text(chunk) => text.push_str(&chunk),
            Event::Code(chunk) => text.push_str(&chunk),
            Event::SoftBreak => text.push(' '),
            Event::HardBreak => text.push('\n'),
            _ => {}
        }
    }

    blocks
}

#[cfg(test)]
pub fn parse_summary(source: &str) -> Vec<(&'static str, usize)> {
    parse(source)
        .into_iter()
        .map(|block| match block {
            Block::Heading(level, _) => ("heading", level as usize),
            Block::Paragraph(_) => ("paragraph", 0),
            Block::ListItem(depth, _, _) => ("list", depth),
            Block::Code(_, text) => ("code", text.lines().count()),
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
