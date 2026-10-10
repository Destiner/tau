use crate::telemetry::{trace_context::TraceContext, Telemetry};
use tauri::State;

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
    telemetry.submit_issue_report(description, session_id)
}
