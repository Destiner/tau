use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::profile;
use crate::telemetry::{trace_context::TraceContext, Telemetry};

const FEEDBACK_DIRECTORY_NAME: &str = "feedback";
const ISSUE_REPORT_FILE: &str = "issues.jsonl";
const MAX_DESCRIPTION_CHARS: usize = 10_000;
const MAX_SESSION_ID_CHARS: usize = 256;
const WRITE_ERROR: &str = "The report could not be saved. Try again.";

static ISSUE_REPORT_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IssueReport<'a> {
    time_unix_nano: String,
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
}

#[tauri::command]
pub fn submit_issue_report(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    description: String,
    session_id: Option<String>,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "submit_issue_report"));
    let description = description.trim();
    if description.is_empty() {
        return Err("Describe the issue before submitting.".to_string());
    }
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!(
            "Keep the issue description under {MAX_DESCRIPTION_CHARS} characters."
        ));
    }

    let session_id = session_id.as_deref().filter(|value| !value.is_empty());
    if session_id.is_some_and(|value| value.chars().count() > MAX_SESSION_ID_CHARS) {
        return Err("The current session could not be included. Try again without it.".to_string());
    }

    append_issue_report(
        &resolve_feedback_dir(),
        description,
        session_id,
        SystemTime::now(),
    )
}

fn resolve_feedback_dir() -> PathBuf {
    profile::app_data_dir().join(FEEDBACK_DIRECTORY_NAME)
}

fn append_issue_report(
    dir: &Path,
    description: &str,
    session_id: Option<&str>,
    now: SystemTime,
) -> Result<(), String> {
    let report = IssueReport {
        time_unix_nano: now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string(),
        description,
        session_id,
    };
    let mut line = serde_json::to_vec(&report).map_err(|_| WRITE_ERROR.to_string())?;
    line.push(b'\n');

    let _guard = ISSUE_REPORT_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    fs::create_dir_all(dir).map_err(|_| WRITE_ERROR.to_string())?;
    set_owner_only_dir_permissions(dir);
    let path = dir.join(ISSUE_REPORT_FILE);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|_| WRITE_ERROR.to_string())?;
    set_owner_only_file_permissions(&path);
    file.write_all(&line)
        .and_then(|_| file.sync_all())
        .map_err(|_| WRITE_ERROR.to_string())
}

#[cfg(unix)]
fn set_owner_only_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn set_owner_only_dir_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_owner_only_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only_file_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn appends_reports_as_json_lines_with_optional_session_context() {
        let directory = tempfile::tempdir().expect("temporary directory");
        append_issue_report(
            directory.path(),
            "The sidebar stopped responding.",
            None,
            UNIX_EPOCH + std::time::Duration::from_nanos(42),
        )
        .expect("first report");
        append_issue_report(
            directory.path(),
            "The reply appeared in the wrong session.",
            Some("session-1"),
            UNIX_EPOCH + std::time::Duration::from_nanos(84),
        )
        .expect("second report");

        let contents = fs::read_to_string(directory.path().join(ISSUE_REPORT_FILE))
            .expect("issue report file");
        let reports = contents
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid JSON line"))
            .collect::<Vec<_>>();

        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0]["timeUnixNano"], "42");
        assert_eq!(reports[0]["description"], "The sidebar stopped responding.");
        assert!(reports[0].get("sessionId").is_none());
        assert_eq!(reports[1]["timeUnixNano"], "84");
        assert_eq!(reports[1]["sessionId"], "session-1");
    }

    #[cfg(unix)]
    #[test]
    fn report_directory_and_file_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().expect("temporary directory");
        let directory = parent.path().join("feedback");
        append_issue_report(&directory, "A report", None, UNIX_EPOCH).expect("issue report");

        let dir_mode = fs::metadata(&directory)
            .expect("feedback directory")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(directory.join(ISSUE_REPORT_FILE))
            .expect("issue report file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn feedback_path_is_next_to_the_telemetry_directory() {
        assert_eq!(
            resolve_feedback_dir(),
            profile::app_data_dir().join("feedback")
        );
    }
}
