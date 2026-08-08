//! Pure terminal search and plain-text link detection.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridMatch {
    pub line: i32,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlainLink {
    Url(String),
    Path(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlainLinkMatch {
    pub start: usize,
    pub end: usize,
    pub target: PlainLink,
}

/// Finds literal matches. A query containing an uppercase character is case-sensitive.
pub fn find_in_lines(lines: &[(i32, String)], query: &str) -> Vec<GridMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let needle = if sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut matches = Vec::new();

    for (line, text) in lines {
        let haystack = if sensitive {
            text.clone()
        } else {
            text.to_lowercase()
        };
        for (start, found) in haystack.match_indices(&needle) {
            let end = start + found.len();
            matches.push(GridMatch {
                line: *line,
                start: text[..start.min(text.len())].chars().count(),
                end: text[..end.min(text.len())].chars().count(),
            });
        }
    }
    matches
}

/// Finds the URL or path token under one terminal cell.
pub fn plain_link_at(text: &str, column: usize) -> Option<PlainLinkMatch> {
    let chars: Vec<char> = text.chars().collect();
    if column >= chars.len() || chars.get(column)?.is_whitespace() {
        return None;
    }
    let mut start = column;
    while start > 0 && !chars.get(start - 1)?.is_whitespace() {
        start -= 1;
    }
    let mut end = column + 1;
    while end < chars.len() && !chars.get(end)?.is_whitespace() {
        end += 1;
    }

    while start < end && matches!(chars.get(start), Some('(' | '[' | '{' | '<' | '"' | '\'')) {
        start += 1;
    }
    while start < end
        && matches!(
            chars.get(end - 1),
            Some(')' | ']' | '}' | '>' | '"' | '\'' | ',' | '.' | ';')
        )
    {
        end -= 1;
    }
    if column < start || column >= end {
        return None;
    }

    let token: String = chars.get(start..end)?.iter().collect();
    let target = if token.starts_with("https://") || token.starts_with("http://") {
        PlainLink::Url(token)
    } else if token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with('/')
        || token.starts_with('~')
        || token.contains('/')
    {
        PlainLink::Path(token)
    } else {
        return None;
    };
    Some(PlainLinkMatch { start, end, target })
}
