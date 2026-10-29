//! Issue reports go directly to Linear. There is no native outbox or report tracking.
use crate::cloud::{CloudState, IssueReceipt, Result};
use crate::telemetry::trace_context::TraceContext;
use tauri::State;

#[tauri::command]
pub async fn submit_issue_report(
    cloud: State<'_, CloudState>,
    telemetry_context: Option<TraceContext>,
    description: String,
    session_id: Option<String>,
    report_id: String,
    time_unix_nano: String,
) -> Result<IssueReceipt> {
    let _ = telemetry_context;
    cloud
        .submit(description, session_id, report_id, time_unix_nano)
        .await
}
