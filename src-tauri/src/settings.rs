use crate::telemetry::{trace_context::TraceContext, Telemetry};
use tauri::State;

#[tauri::command]
pub fn read_model_scope(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<Vec<String>, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_model_scope"));
    tau_core::settings::read_model_scope()
}

#[tauri::command]
pub async fn read_remote_model_scope(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
) -> Result<Vec<String>, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_remote_model_scope"));
    tauri::async_runtime::spawn_blocking(move || {
        tau_core::settings::read_remote_model_scope(connection_string)
    })
    .await
    .map_err(|error| format!("Could not read the remote Pi settings: {error}"))?
}
