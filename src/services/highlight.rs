//! Viewport-scoped syntax highlighting.
//!
//! The whole file is parsed once, because tree-sitter parsing is cheap and the
//! tree is needed to resolve any range. Queries then run with an explicit byte
//! range covering the visible rows only, so a 146 KB file costs the same as a
//! 2 KB file while scrolling.

use std::collections::HashMap;
use std::ops::Range;

use gpui::Hsla;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator as _, Tree};

use crate::theme::Colors;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Rust,
    Swift,
    None,
}

impl Lang {
    pub fn for_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Self::Rust,
            Some("swift") => Self::Swift,
            _ => Self::None,
        }
    }

    fn language(self) -> Option<Language> {
        match self {
            Self::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Self::Swift => Some(tree_sitter_swift::LANGUAGE.into()),
            Self::None => None,
        }
    }

    fn highlights(self) -> &'static str {
        match self {
            Self::Rust => tree_sitter_rust::HIGHLIGHTS_QUERY,
            Self::Swift => tree_sitter_swift::HIGHLIGHTS_QUERY,
            Self::None => "",
        }
    }
}

/// One coloured byte range inside a line.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: Hsla,
}

pub struct Highlighter {
    lang: Lang,
    parser: Parser,
    query: Option<Query>,
    tree: Option<Tree>,
}

impl Highlighter {
    pub fn new(lang: Lang) -> Self {
        let mut parser = Parser::new();
        let query = lang.language().and_then(|language| {
            parser.set_language(&language).ok()?;
            Query::new(&language, lang.highlights()).ok()
        });
        Self {
            lang,
            parser,
            query,
            tree: None,
        }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// Parses the whole document. Called on load and on edit, never on scroll.
    pub fn parse(&mut self, source: &str) {
        if self.query.is_none() {
            return;
        }
        self.tree = self.parser.parse(source, self.tree.as_ref());
    }

    /// Highlights only `byte_range`. Returns spans grouped by line index.
    pub fn spans_in(
        &self,
        source: &str,
        byte_range: Range<usize>,
        line_starts: &[usize],
        colors: &Colors,
    ) -> HashMap<usize, Vec<Span>> {
        let mut out: HashMap<usize, Vec<Span>> = HashMap::new();
        let (Some(query), Some(tree)) = (self.query.as_ref(), self.tree.as_ref()) else {
            return out;
        };

        let palette = capture_palette(colors);
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(byte_range.clone());

        let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let name = &query.capture_names()[capture.index as usize];
                let Some(color) = palette.get(scope_key(name)).copied() else {
                    continue;
                };
                push_node(&capture.node, color, line_starts, &mut out);
            }
        }

        for spans in out.values_mut() {
            *spans = resolve_overlaps(std::mem::take(spans));
        }
        out
    }
}

/// Flattens overlapping captures into one non-overlapping run per byte.
///
/// A highlight query reports several captures for the same text: a whole
/// identifier as `variable`, and part of it as `type` or `constructor`. Handing
/// both to `StyledText` colours the identifier in pieces, which is what made
/// names read as two-tone.
///
/// The rule is narrowest wins. A short capture is the more specific statement
/// about that text, so it claims its bytes first and a wider capture keeps only
/// what is left. That splits the wider run around the narrow one instead of
/// fighting with it.
fn resolve_overlaps(mut spans: Vec<Span>) -> Vec<Span> {
    if spans.len() < 2 {
        return spans;
    }

    // Narrowest first; a stable tiebreak on start keeps the result deterministic.
    spans.sort_by_key(|span| (span.end - span.start, span.start));

    // Kept sorted by start and free of overlap at every step.
    let mut claimed: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        let mut cursor = span.start;
        let mut pieces = Vec::new();
        for taken in claimed
            .iter()
            .filter(|taken| taken.end > span.start && taken.start < span.end)
        {
            if taken.start > cursor {
                pieces.push(Span {
                    start: cursor,
                    end: taken.start,
                    color: span.color,
                });
            }
            cursor = cursor.max(taken.end);
        }
        if cursor < span.end {
            pieces.push(Span {
                start: cursor,
                end: span.end,
                color: span.color,
            });
        }
        if pieces.is_empty() {
            continue;
        }
        claimed.extend(pieces);
        claimed.sort_by_key(|span| span.start);
    }

    // Neighbouring runs of the same colour render as one; merging keeps the run
    // count down before the text layout sees them.
    let mut merged: Vec<Span> = Vec::with_capacity(claimed.len());
    for span in claimed {
        match merged.last_mut() {
            Some(previous) if previous.end == span.start && previous.color == span.color => {
                previous.end = span.end;
            }
            _ => merged.push(span),
        }
    }
    merged
}

#[cfg(test)]
pub fn resolve_overlaps_for_test(spans: Vec<Span>) -> Vec<Span> {
    resolve_overlaps(spans)
}

fn push_node(node: &Node, color: Hsla, line_starts: &[usize], out: &mut HashMap<usize, Vec<Span>>) {
    let start = node.start_position();
    let end = node.end_position();
    for row in start.row..=end.row {
        let Some(line_start) = line_starts.get(row) else {
            continue;
        };
        let line_end = line_starts
            .get(row + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(usize::MAX);
        let span_start = if row == start.row {
            node.start_byte()
        } else {
            *line_start
        };
        let span_end = if row == end.row {
            node.end_byte()
        } else {
            line_end
        };
        if span_end <= span_start {
            continue;
        }
        out.entry(row).or_default().push(Span {
            start: span_start - line_start,
            end: span_end.saturating_sub(*line_start),
            color,
        });
    }
}

/// Collapses `keyword.function` and friends onto their top-level scope so one
/// small palette covers both grammars.
fn scope_key(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn capture_palette(c: &Colors) -> HashMap<&'static str, Hsla> {
    HashMap::from([
        ("keyword", c.accent),
        ("function", c.git_untracked),
        ("type", c.workflow_todo),
        ("constructor", c.workflow_todo),
        ("string", c.git_added),
        ("number", c.workflow_todo),
        ("comment", c.ink_secondary),
        ("constant", c.workflow_todo),
        ("property", c.git_modified),
        ("variable", c.ink),
        ("operator", c.ink_secondary),
        ("punctuation", c.ink_secondary),
        ("attribute", c.workflow_blocked),
        ("label", c.workflow_blocked),
    ])
}

/// Byte offset of the start of each line. Computed once per load.
pub fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}
