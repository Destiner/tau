use crate::telemetry::{trace_context::TraceContext, Telemetry};
use tauri::State;

#[tauri::command]
pub fn read_admin_mode(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> bool {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_admin_mode"));
    telemetry.read_admin_mode()
}

#[tauri::command]
pub fn set_admin_mode(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    enabled: bool,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "set_admin_mode"));
    telemetry.set_admin_mode(enabled)
}

#[tauri::command]
pub fn get_dismissed_update_version() -> Option<String> {
    tau_observability::admin::get_dismissed_update_version(&crate::profile::app_data_dir())
}

#[tauri::command]
pub fn set_dismissed_update_version(version: Option<String>) -> Result<(), String> {
    tau_observability::admin::set_dismissed_update_version(&crate::profile::app_data_dir(), version)
}
