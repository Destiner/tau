//! Production markdown.ts recognition, independent of shell/filesystem ownership.
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReference {
    pub text: String,
    pub path: String,
}

fn control(value: &str) -> bool {
    value.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}
// ECMAScript WhiteSpace + LineTerminator, not Rust's Unicode White_Space.
fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn rooted(value: &str) -> bool {
    ["/", "./", "../", "~/"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}
fn route(c: char) -> bool {
    "{}*[]:".contains(c)
}
fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit())
}

pub fn strip_line_suffix(value: &str) -> &str {
    if value.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return value;
    }
    for (index, _) in value.match_indices(':') {
        if index == 0 {
            continue;
        }
        let suffix = &value[index + 1..];
        if digits(suffix)
            || [':', '-', '–'].iter().any(|separator| {
                suffix
                    .split_once(*separator)
                    .is_some_and(|(line, end)| digits(line) && digits(end))
            })
        {
            return &value[..index];
        }
    }
    value
}

pub fn parse_file_reference(candidate: &str) -> Option<FileReference> {
    let text = candidate.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let path = strip_line_suffix(text);
    if !path.contains('/') || path.contains("//") || control(path) {
        return None;
    }
    let is_rooted = rooted(path);
    let explicit = path.starts_with("./") || path.starts_with("../");
    let body = if let Some(body) = path.strip_prefix("../") {
        body
    } else if let Some(body) = path.strip_prefix("./").or_else(|| path.strip_prefix("~/")) {
        body
    } else if let Some(body) = path.strip_prefix('/') {
        body
    } else {
        path
    };
    let segments: Vec<_> = body.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty()
        || segments
            .iter()
            .any(|s| s.starts_with(js_whitespace) || s.ends_with(js_whitespace))
    {
        return None;
    }
    let directory = path.ends_with('/');
    let named = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(_, ext)| {
            (1..=10).contains(&ext.len()) && ext.bytes().all(|c| c.is_ascii_alphanumeric())
        });
    if (!is_rooted && !directory && !named)
        || (!is_rooted && directory && segments.len() < 2)
        || (is_rooted && !explicit && segments.len() == 1 && !directory && !named)
        || path.chars().any(route)
    {
        return None;
    }
    Some(FileReference {
        text: text.into(),
        path: path.into(),
    })
}

fn decode_uri(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let a = (input.next()? as char).to_digit(16)?;
            let b = (input.next()? as char).to_digit(16)?;
            bytes.push((a * 16 + b) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok()
}

fn windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && matches!(bytes[1], b':' | b'|')
}

// WHATWG file URLs are special: backslashes are separators, drive authorities
// become paths, localhost disappears, and encoded dot segments normalize BEFORE
// decodeURIComponent. No URL crate is a direct dependency of this consumer.
fn file_url_path(value: &str) -> Option<String> {
    let rest = value
        .strip_prefix("file:")?
        .trim_end_matches(|c: char| c <= ' ');
    let rest = rest
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .replace('\\', "/");
    let path = if let Some(authority) = rest.strip_prefix("//") {
        let slash = authority.find('/').unwrap_or(authority.len());
        let host = &authority[..slash];
        if windows_drive(host) {
            format!("/{host}{}", &authority[slash..])
        } else {
            let host = decode_uri(host)?;
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return None;
            }
            authority[slash..].into()
        }
    } else {
        rest
    };
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    let mut parts: Vec<String> = Vec::new();
    let segments: Vec<_> = path[1..].split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        let dot = segment.to_ascii_lowercase().replace("%2e", ".");
        if dot == "." || dot == ".." {
            if dot == ".."
                && parts
                    .last()
                    .is_some_and(|last| !(parts.len() == 1 && windows_drive(last)))
            {
                parts.pop();
            }
            if index == segments.len() - 1 {
                parts.push(String::new());
            }
        } else if parts.is_empty() && windows_drive(segment) {
            parts.push(format!("{}:", &segment[..1]));
        } else {
            parts.push((*segment).into());
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// Explicit destinations do not use prose heuristics. Decode once only.
pub fn parse_markdown_file_destination(value: &str) -> Option<String> {
    if value.is_empty() || value.starts_with('#') || control(value) {
        return None;
    }
    let scheme = value.split_once(':').filter(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    let pathname = if scheme.is_some() {
        return file_url_path(value).and_then(|pathname| {
            let decoded = decode_uri(&pathname)?;
            (!decoded.is_empty() && !control(&decoded)).then(|| strip_line_suffix(&decoded).into())
        });
    } else {
        value
    };
    let decoded = decode_uri(pathname)?;
    if decoded.is_empty() || control(&decoded) {
        return None;
    }
    Some(strip_line_suffix(&decoded).into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathSpan {
    /// UTF-8 offsets in the rendered text run, never synthesized Markdown.
    pub range: Range<usize>,
    pub path: String,
}

fn entity(value: &str) -> Option<char> {
    let name = value
        .strip_prefix('&')?
        .strip_suffix(';')?
        .to_ascii_lowercase();
    match name.as_str() {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        _ => {
            let (digits, radix) = if let Some(digits) = name.strip_prefix("#x") {
                (digits, 16)
            } else {
                (name.strip_prefix('#')?, 10)
            };
            char::from_u32(u32::from_str_radix(digits, radix).ok()?)
        }
    }
}
fn decode_html(value: &str) -> String {
    let mut result = String::new();
    let mut cursor = 0;
    while cursor < value.len() {
        if value[cursor..].starts_with('&')
            && let Some(end) = value[cursor..].find(';')
            && let Some(c) = entity(&value[cursor..cursor + end + 1])
        {
            result.push(c);
            cursor += end + 1;
            continue;
        }
        let c = value[cursor..].chars().next().unwrap();
        result.push(c);
        cursor += c.len_utf8();
    }
    result
}
fn touches_entity(text: &str, start: usize, end: usize) -> bool {
    let before = &text[..start];
    let after = &text[end..];
    (before.ends_with(';')
        && before
            .rfind('&')
            .is_some_and(|at| entity(&before[at..]).is_some()))
        || (after.starts_with('&')
            && after
                .find(';')
                .is_some_and(|at| entity(&after[..=at]).is_some()))
}

fn first(c: char) -> bool {
    c.is_ascii_alphanumeric() || "~._/".contains(c)
}
fn continuation(c: char) -> bool {
    first(c) || "+@:-".contains(c)
}

/// Call only for decoded prose/inline-code leaves, never anchors, pre, SVG or attributes.
/// Splitting at Markdown AST leaf boundaries preserves Vue's HTML text-run behavior.
pub fn references_in_text(text: &str) -> Vec<PathSpan> {
    references_in_run(text, false)
}

/// Vue's post-sanitization HTML text-run oracle. Ranges address encoded input.
pub fn references_in_encoded_text(text: &str) -> Vec<PathSpan> {
    references_in_run(text, true)
}
fn references_in_run(text: &str, encoded: bool) -> Vec<PathSpan> {
    let mut spans = Vec::new();
    let mut base = 0;
    for line in text.split_inclusive('\n') {
        let written = line.trim_matches(js_whitespace);
        let decoded = if encoded {
            decode_html(written)
        } else {
            written.into()
        };
        let candidate = decoded.as_str();
        if rooted(candidate)
            && let Some(reference) = parse_file_reference(candidate)
            && reference.text == candidate
        {
            let start = base + line.len() - line.trim_start_matches(js_whitespace).len();
            let marker = reference.text.len() - reference.path.len();
            spans.push(PathSpan {
                range: start..start + written.len() - marker,
                path: reference.path,
            });
            base += line.len();
            continue;
        }
        let mut cursor = 0;
        while cursor < line.len() {
            let c = line[cursor..].chars().next().unwrap();
            if !first(c) {
                cursor += c.len_utf8();
                continue;
            }
            let start = cursor;
            cursor += c.len_utf8();
            while cursor < line.len() {
                let c = line[cursor..].chars().next().unwrap();
                if !continuation(c) {
                    break;
                }
                cursor += c.len_utf8();
            }
            if line[cursor..].starts_with('–') {
                let end = cursor + '–'.len_utf8();
                let count = line[end..].bytes().take_while(u8::is_ascii_digit).count();
                if count > 0 {
                    cursor = end + count;
                }
            }
            let before = line[..start].chars().next_back();
            let after = line[cursor..].chars().next();
            // Decoded HTML delimiters have the same protection as &lt; and &gt;.
            if before.is_some_and(route)
                || after.is_some_and(route)
                || (!encoded
                    && (before.is_some_and(|c| c == '<' || c == '>')
                        || after.is_some_and(|c| c == '<' || c == '>')))
                || (encoded && touches_entity(line, start, cursor))
            {
                continue;
            }
            if let Some(reference) = parse_file_reference(&line[start..cursor]) {
                spans.push(PathSpan {
                    range: base + start..base + start + reference.path.len(),
                    path: reference.path,
                });
            }
        }
        base += line.len();
    }
    spans
}

/// Reconstruct the HTML-equivalent run without discarding entity provenance.
/// Only the decoration's byte ranges are translated; source segments remain untouched.
pub fn inline_links(
    leaf: &gpui_kit::base::text::MarkdownInlineLeaf<'_>,
) -> Vec<gpui_kit::base::text::InlineLinkDecoration> {
    use gpui_kit::base::text::InlineLinkDecoration;
    let mut encoded = String::new();
    let mut boundaries = vec![(0, 0)];
    let mut cursor = 0;
    let mut segments = leaf.segments.iter().peekable();
    while cursor < leaf.text.len() {
        // Source segments are ordered by rendered offset. Never restart at the
        // first segment for each character in entity-heavy leaves.
        while segments.peek().is_some_and(|segment| {
            mapping_step();
            segment.rendered.start < cursor
        }) {
            segments.next();
        }
        let mapped_entity = (!leaf.inline_code)
            .then(|| {
                segments.peek().copied().filter(|segment| {
                    segment.rendered.start == cursor
                        && leaf
                            .source
                            .get(segment.source.clone())
                            .is_some_and(|raw| raw.starts_with('&') && raw.ends_with(';'))
                })
            })
            .flatten();
        if let Some(segment) = mapped_entity {
            encoded.push_str(&leaf.source[segment.source.clone()]);
            cursor = segment.rendered.end;
            boundaries.push((encoded.len(), cursor));
            continue;
        }
        let c = leaf.text[cursor..].chars().next().unwrap();
        match c {
            '&' => encoded.push_str("&amp;"),
            '<' => encoded.push_str("&lt;"),
            '>' => encoded.push_str("&gt;"),
            '"' => encoded.push_str("&quot;"),
            '\'' => encoded.push_str("&#39;"),
            _ => encoded.push(c),
        }
        cursor += c.len_utf8();
        boundaries.push((encoded.len(), cursor));
    }
    references_in_encoded_text(&encoded)
        .into_iter()
        .filter_map(|span| {
            let endpoint = |offset| {
                boundaries
                    .binary_search_by(|(encoded, _)| {
                        mapping_step();
                        encoded.cmp(&offset)
                    })
                    .ok()
                    .map(|index| boundaries[index].1)
            };
            let start = endpoint(span.range.start)?;
            let end = endpoint(span.range.end)?;
            Some(InlineLinkDecoration {
                range: start..end,
                path: span.path.into(),
                button: false,
            })
        })
        .collect()
}

#[inline]
fn mapping_step() {
    #[cfg(test)]
    MAPPING_STEPS.with(|steps| steps.set(steps.get() + 1));
}

#[cfg(test)]
thread_local! {
    pub(super) static MAPPING_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Transcript path resolution is POSIX-only, unlike preview directory normalization.
pub fn resolve_file_path(base: &str, path: &str, home: Option<&str>) -> String {
    if let Some(relative) = path.strip_prefix("~/") {
        return home.map_or_else(
            || path.into(),
            |home| normalize_posix(&format!("{home}/{relative}")),
        );
    }
    normalize_posix(&if path.starts_with('/') {
        path.into()
    } else {
        format!("{base}/{path}")
    })
}
fn normalize_posix(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            _ => parts.push(part),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}

#[cfg(test)]
#[path = "file_reference_tests.rs"]
mod tests;
