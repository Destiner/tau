//! Forbidden-content canaries and redaction helpers shared by every stage's
//! privacy tests, so "does telemetry leak X" is answered against one list
//! instead of being redefined per test file.

/// Distinct sentinel values later tests inject into the content categories
/// telemetry must never record, then scan persisted output for. Kept in one
/// place so a test that forgets a category shows up as a missing entry here,
/// not as a silently-absent assertion in a test body.
// Stage 4+ privacy tests scan persisted/ingested output for these.
#[allow(dead_code)]
pub const FORBIDDEN_CONTENT_CANARIES: &[(&str, &str)] = &[
    ("prompt", "tau-canary-prompt-3f1c9a"),
    ("assistant_response", "tau-canary-response-3f1c9a"),
    ("transcript_entry", "tau-canary-transcript-3f1c9a"),
    ("draft", "tau-canary-draft-3f1c9a"),
    ("tool_arguments", "tau-canary-tool-args-3f1c9a"),
    ("tool_result", "tau-canary-tool-result-3f1c9a"),
    ("extension_message", "tau-canary-extension-3f1c9a"),
    ("clipboard", "tau-canary-clipboard-3f1c9a"),
    ("file_contents", "tau-canary-file-3f1c9a"),
    ("pi_rpc_body", "tau-canary-rpc-body-3f1c9a"),
    ("ssh_command", "tau-canary-ssh-3f1c9a"),
    ("credential", "tau-canary-credential-3f1c9a"),
    ("project_path", "/Users/tau-canary-3f1c9a/project"),
    ("connection_string", "ssh://tau-canary-3f1c9a@build-box"),
];

/// True if `haystack` contains any forbidden-content canary. Later stages'
/// privacy tests assert this is `false` for everything telemetry persists.
#[allow(dead_code)]
pub fn contains_forbidden_content(haystack: &str) -> bool {
    FORBIDDEN_CONTENT_CANARIES
        .iter()
        .any(|(_, canary)| haystack.contains(canary))
}

/// Names of every canary found in `haystack`, so a failing test can report
/// which content categories leaked instead of just that something did.
#[allow(dead_code)]
pub fn matched_canaries(haystack: &str) -> Vec<&'static str> {
    FORBIDDEN_CONTENT_CANARIES
        .iter()
        .filter(|(_, canary)| haystack.contains(canary))
        .map(|(name, _)| *name)
        .collect()
}

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

/// The hard per-attribute size ceiling every catalog entry in `attributes`
/// uses, so nothing accidentally inherits an unbounded length.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_canary_present_in_a_larger_string() {
        let (_, prompt_canary) = FORBIDDEN_CONTENT_CANARIES[0];
        let haystack = format!("unrelated text {prompt_canary} more text");
        assert!(contains_forbidden_content(&haystack));
        assert_eq!(matched_canaries(&haystack), vec!["prompt"]);
    }

    #[test]
    fn ordinary_content_does_not_trip_the_canaries() {
        assert!(!contains_forbidden_content(
            "session started; rpc get_state ok"
        ));
        assert!(matched_canaries("session started").is_empty());
    }

    #[test]
    fn source_locations_drop_directory_components() {
        let (_, path_canary) = FORBIDDEN_CONTENT_CANARIES
            .iter()
            .find(|(name, _)| *name == "project_path")
            .expect("project_path canary");
        let file = format!("{path_canary}/src/index.ts");
        let sanitized = sanitize_source_location(&file, Some(10), Some(4));
        assert_eq!(sanitized, "index.ts:10:4");
        assert!(!contains_forbidden_content(&sanitized));
    }

    #[test]
    fn source_locations_without_a_column_omit_it() {
        assert_eq!(
            sanitize_source_location("lib.rs", Some(42), None),
            "lib.rs:42"
        );
        assert_eq!(sanitize_source_location("lib.rs", None, None), "lib.rs");
        assert_eq!(
            sanitize_source_location("/private/project/src/", None, None),
            "src"
        );
        assert_eq!(sanitize_source_location("///", None, None), "unknown");
    }

    #[test]
    fn truncation_keeps_short_values_untouched() {
        assert_eq!(truncate_to_limit("short", 128), "short");
    }

    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        let value = "a".repeat(127) + "é";
        let truncated = truncate_to_limit(&value, 128);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert_eq!(truncated, "a".repeat(127));
    }
}
