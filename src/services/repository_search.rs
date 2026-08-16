//! Unified repository search for Quick Open.
//!
//! File paths, syntax-tree declarations, and live Git commits share one ranked
//! result list. The caller runs this module on a background executor because
//! declaration discovery reads and parses the indexed text files.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use tree_sitter::{Language, Node, Parser};

use super::file_index::{IndexedFile, MAX_TEXT_BYTES};
use super::git::Commit;
use super::search::Cancel;

const MAX_RESULTS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositoryResult {
    File {
        relative: String,
        absolute: PathBuf,
    },
    Symbol {
        name: String,
        declaration: String,
        relative: String,
        absolute: PathBuf,
        line: usize,
    },
    Commit {
        short_hash: String,
        subject: String,
        author: String,
    },
}

struct ScoredResult {
    score: u32,
    kind_order: u8,
    result: RepositoryResult,
}

/// Searches the shared file index and the current Git snapshot.
///
/// File paths are cheap. Symbol results are syntax-tree declarations, never
/// regex matches. Files over the shared 2 MB bound are rejected again here so
/// manually constructed indexes cannot bypass the repository limit.
pub fn run(
    files: &[IndexedFile],
    commits: &[Commit],
    query: &str,
    cancel: Cancel,
) -> Vec<RepositoryResult> {
    if cancel.is_cancelled() {
        return Vec::new();
    }

    let query = query.trim();
    if query.is_empty() {
        return files
            .iter()
            .filter(|file| within_text_limit(file))
            .take(MAX_RESULTS)
            .map(|file| RepositoryResult::File {
                relative: file.relative.clone(),
                absolute: file.absolute.clone(),
            })
            .collect();
    }

    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut path_matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut text_matcher = Matcher::new(Config::DEFAULT);
    let mut results = Vec::new();

    for file in files {
        if !within_text_limit(file) {
            continue;
        }
        if cancel.is_cancelled() {
            return Vec::new();
        }
        if let Some(score) = fuzzy_score(&pattern, &mut path_matcher, &file.relative) {
            results.push(ScoredResult {
                score,
                kind_order: 1,
                result: RepositoryResult::File {
                    relative: file.relative.clone(),
                    absolute: file.absolute.clone(),
                },
            });
        }
    }

    for commit in commits {
        if cancel.is_cancelled() {
            return Vec::new();
        }
        let candidate = format!("{} {} {}", commit.short_hash, commit.subject, commit.author);
        if let Some(score) = fuzzy_score(&pattern, &mut text_matcher, &candidate) {
            results.push(ScoredResult {
                score,
                kind_order: 2,
                result: RepositoryResult::Commit {
                    short_hash: commit.short_hash.clone(),
                    subject: commit.subject.clone(),
                    author: commit.author.clone(),
                },
            });
        }
    }

    for file in files {
        if cancel.is_cancelled() {
            return Vec::new();
        }
        if !within_text_limit(file) {
            continue;
        }
        let Some((language, kinds)) = language_and_kinds(&file.absolute) else {
            continue;
        };
        let Some(source) = read_text_with_limit(&file.absolute) else {
            continue;
        };
        let mut parser = Parser::new();
        if parser.set_language(&language).is_err() {
            continue;
        }
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        if cancel.is_cancelled() {
            return Vec::new();
        }
        let mut declarations = Vec::new();
        if !collect_declarations(
            tree.root_node(),
            source.as_bytes(),
            kinds,
            &cancel,
            &mut declarations,
        ) {
            return Vec::new();
        }
        for declaration in declarations {
            if cancel.is_cancelled() {
                return Vec::new();
            }
            let candidate = format!(
                "{} {} {}",
                declaration.name, declaration.kind, file.relative
            );
            let Some(score) = fuzzy_score(&pattern, &mut text_matcher, &candidate) else {
                continue;
            };
            results.push(ScoredResult {
                score: score.saturating_add(8),
                kind_order: 0,
                result: RepositoryResult::Symbol {
                    name: declaration.name,
                    declaration: declaration.kind.to_string(),
                    relative: file.relative.clone(),
                    absolute: file.absolute.clone(),
                    line: declaration.line,
                },
            });
        }
    }

    if cancel.is_cancelled() {
        return Vec::new();
    }
    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.kind_order.cmp(&b.kind_order))
            .then_with(|| result_sort_key(&a.result).cmp(&result_sort_key(&b.result)))
    });
    results.truncate(MAX_RESULTS);
    results.into_iter().map(|entry| entry.result).collect()
}

fn read_text_with_limit(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn within_text_limit(file: &IndexedFile) -> bool {
    file.absolute
        .metadata()
        .map(|metadata| metadata.len() <= MAX_TEXT_BYTES)
        .unwrap_or(false)
}

fn fuzzy_score(pattern: &Pattern, matcher: &mut Matcher, candidate: &str) -> Option<u32> {
    let mut buffer = Vec::new();
    let haystack = Utf32Str::new(candidate, &mut buffer);
    pattern.score(haystack, matcher)
}

fn result_sort_key(result: &RepositoryResult) -> String {
    match result {
        RepositoryResult::File { relative, .. } => relative.clone(),
        RepositoryResult::Symbol { name, relative, .. } => format!("{relative}:{name}"),
        RepositoryResult::Commit {
            short_hash,
            subject,
            ..
        } => format!("{short_hash}:{subject}"),
    }
}

struct Declaration {
    name: String,
    kind: &'static str,
    line: usize,
}

fn collect_declarations(
    node: Node<'_>,
    source: &[u8],
    kinds: &[(&'static str, &'static str)],
    cancel: &Cancel,
    output: &mut Vec<Declaration>,
) -> bool {
    if cancel.is_cancelled() {
        return false;
    }
    if let Some((_, label)) = kinds.iter().find(|(kind, _)| *kind == node.kind())
        && let Some(name) = declaration_name(node, source)
    {
        output.push(Declaration {
            name,
            kind: declaration_label(node, label, source),
            line: node.start_position().row + 1,
        });
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if !collect_declarations(child, source, kinds, cancel, output) {
            return false;
        }
    }
    true
}

fn declaration_label(node: Node<'_>, fallback: &'static str, source: &[u8]) -> &'static str {
    let Some(kind) = node.child_by_field_name("declaration_kind") else {
        return fallback;
    };
    match kind.utf8_text(source).ok() {
        Some("actor") => "Actor",
        Some("class") => "Class",
        Some("enum") => "Enum",
        Some("extension") => "Extension",
        Some("protocol") => "Protocol",
        Some("struct") => "Struct",
        _ => fallback,
    }
}

fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let name = node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find(|child| {
            matches!(
                child.kind(),
                "identifier" | "type_identifier" | "simple_identifier" | "property_identifier"
            )
        })
    })?;
    name.utf8_text(source).ok().map(str::to_string)
}

fn language_and_kinds(path: &Path) -> Option<(Language, &'static [(&'static str, &'static str)])> {
    const RUST: &[(&str, &str)] = &[
        ("function_item", "Function"),
        ("struct_item", "Struct"),
        ("enum_item", "Enum"),
        ("trait_item", "Trait"),
        ("type_item", "Type"),
        ("const_item", "Constant"),
        ("static_item", "Static"),
        ("mod_item", "Module"),
        ("macro_definition", "Macro"),
    ];
    const SWIFT: &[(&str, &str)] = &[
        ("function_declaration", "Function"),
        ("class_declaration", "Class"),
        ("protocol_declaration", "Protocol"),
        ("typealias_declaration", "Type"),
    ];
    const PYTHON: &[(&str, &str)] = &[
        ("function_definition", "Function"),
        ("class_definition", "Class"),
    ];
    const JAVASCRIPT: &[(&str, &str)] = &[
        ("function_declaration", "Function"),
        ("generator_function_declaration", "Function"),
        ("class_declaration", "Class"),
        ("method_definition", "Method"),
    ];
    const TYPESCRIPT: &[(&str, &str)] = &[
        ("function_declaration", "Function"),
        ("generator_function_declaration", "Function"),
        ("class_declaration", "Class"),
        ("method_definition", "Method"),
        ("interface_declaration", "Interface"),
        ("type_alias_declaration", "Type"),
        ("enum_declaration", "Enum"),
    ];
    const BASH: &[(&str, &str)] = &[("function_definition", "Function")];

    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => Some((tree_sitter_rust::LANGUAGE.into(), RUST)),
        Some("swift") => Some((tree_sitter_swift::LANGUAGE.into(), SWIFT)),
        Some("py" | "pyi" | "pyw") => Some((tree_sitter_python::LANGUAGE.into(), PYTHON)),
        Some("js" | "jsx" | "mjs" | "cjs") => {
            Some((tree_sitter_javascript::LANGUAGE.into(), JAVASCRIPT))
        }
        Some("ts" | "mts" | "cts") => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            TYPESCRIPT,
        )),
        Some("tsx") => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), TYPESCRIPT)),
        Some("sh" | "bash" | "zsh") => Some((tree_sitter_bash::LANGUAGE.into(), BASH)),
        _ => None,
    }
}
