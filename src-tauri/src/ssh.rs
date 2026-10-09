use crate::models::RemoteDirectoryListing;
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use tauri::State;

#[allow(unused_imports)]
pub use tau_core::ssh::{
    remote_login_shell_command, remote_pi_command, run_remote_command, SshConnection,
};

#[tauri::command]
pub async fn probe_remote_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
) -> Result<RemoteDirectoryListing, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "probe_remote_project"));
    tauri::async_runtime::spawn_blocking(move || {
        tau_core::ssh::probe_remote_project(connection_string)
    })
    .await
    .map_err(|error| format!("Could not test the SSH connection: {error}"))?
}

#[tauri::command]
pub async fn list_remote_directories(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
    working_directory: String,
) -> Result<RemoteDirectoryListing, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "list_remote_directories"));
    tauri::async_runtime::spawn_blocking(move || {
        tau_core::ssh::list_remote_directories(connection_string, working_directory)
    })
    .await
    .map_err(|error| format!("Could not read the remote directory: {error}"))?
}
