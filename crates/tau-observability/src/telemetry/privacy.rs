/// Reduces a source location to a basename and line/column, matching the
/// "sanitize stack traces to module or source basenames" redaction rule.
/// Directory components can carry a project path or a username, so they are
/// dropped rather than truncated.
pub fn sanitize_source_location(file: &str, line: Option<u32>, column: Option<u32>) -> String {
    let base = file
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|base| !base.is_empty())
        .unwrap_or("unknown");
    match (line, column) {
        (Some(line), Some(column)) => format!("{base}:{line}:{column}"),
        (Some(line), None) => format!("{base}:{line}"),
        _ => base.to_string(),
    }
}

pub const DEFAULT_MAX_ATTRIBUTE_LEN: usize = 128;

/// Truncates `value` to at most `max_len` bytes without splitting a UTF-8
/// character.
pub fn truncate_to_limit(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let mut end = max_len;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
