//! Search All Files.
//!
//! `grep-searcher` over the shared file index, results streamed back in batches
//! and cancellable between files.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};

use super::file_index::IndexedFile;

/// One matching line.
#[derive(Clone, Debug)]
pub struct Hit {
    pub relative: String,
    pub absolute: PathBuf,
    pub line: u64,
    pub text: String,
    pub match_start: usize,
    pub match_end: usize,
}

/// A batch of results for one file, streamed as the search walks.
#[derive(Clone, Debug)]
pub struct Batch {
    pub relative: String,
    pub hits: Vec<Hit>,
}

/// Shared cancel flag. Replacing a query flips the previous search's token.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Hard bound, mirroring the Swift app's 1,000 matching lines.
pub const MAX_HITS: usize = 1_000;

struct Collector<'a> {
    relative: &'a str,
    absolute: &'a PathBuf,
    hits: Vec<Hit>,
    matcher: &'a grep_regex::RegexMatcher,
}

impl Sink for Collector<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let text = String::from_utf8_lossy(mat.bytes());
        let trimmed = text.trim_end_matches(['\n', '\r']).to_string();
        let (start, end) = self
            .matcher
            .find(mat.bytes())
            .ok()
            .flatten()
            .map(|m| (m.start(), m.end()))
            .unwrap_or((0, 0));
        self.hits.push(Hit {
            relative: self.relative.to_string(),
            absolute: self.absolute.clone(),
            line: mat.line_number().unwrap_or(0),
            text: trimmed,
            match_start: start,
            match_end: end,
        });
        Ok(self.hits.len() < 200)
    }
}

/// Runs one search. `on_batch` is called per file that produced hits; returning
/// `false` from the cancel token stops the walk between files.
pub fn run(
    files: &[IndexedFile],
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    cancel: Cancel,
    mut on_batch: impl FnMut(Batch),
) -> usize {
    if query.is_empty() {
        return 0;
    }

    let pattern = if whole_word {
        format!(r"\b{}\b", regex_escape(query))
    } else {
        regex_escape(query)
    };

    let Ok(matcher) = RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .build(&pattern)
    else {
        return 0;
    };

    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut total = 0usize;

    for file in files {
        if cancel.is_cancelled() || total >= MAX_HITS {
            break;
        }
        let mut collector = Collector {
            relative: &file.relative,
            absolute: &file.absolute,
            hits: Vec::new(),
            matcher: &matcher,
        };
        if searcher
            .search_path(&matcher, &file.absolute, &mut collector)
            .is_err()
        {
            continue;
        }
        if collector.hits.is_empty() {
            continue;
        }
        total += collector.hits.len();
        on_batch(Batch {
            relative: file.relative.clone(),
            hits: collector.hits,
        });
    }

    total
}

fn regex_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if r"\.+*?()|[]{}^$".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
