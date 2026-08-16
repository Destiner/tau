//! Stage 0 contract: the shared OpenTelemetry vocabulary later telemetry
//! stages build on. This module only defines data — the attribute catalog,
//! privacy rules, and the trace-context shape — and initializes nothing.
//! Provider setup, resource generation, and persistence land in Stage 1; see
//! `OBSERVABILITY_PLAN.md`.

pub mod attributes;
pub mod privacy;
pub mod trace_context;

/// Directory holding telemetry segments, relative to Tau's per-profile
/// application-data directory (see `profile::APP_DIRECTORY_NAME`).
pub const TELEMETRY_DIR_NAME: &str = "telemetry";

/// One newline-delimited OTLP JSON segment file per signal.
pub const TRACE_SEGMENT_FILE: &str = "traces.jsonl";
pub const LOG_SEGMENT_FILE: &str = "logs.jsonl";
pub const METRIC_SEGMENT_FILE: &str = "metrics.jsonl";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn segment_files_are_distinct_jsonl_files() {
        let files = [TRACE_SEGMENT_FILE, LOG_SEGMENT_FILE, METRIC_SEGMENT_FILE];
        assert!(files.iter().all(|file| file.ends_with(".jsonl")));
        assert_eq!(files.iter().collect::<HashSet<_>>().len(), files.len());
    }
}
