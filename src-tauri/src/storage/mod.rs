#[allow(unused_imports)] // Retained for crate-internal callers of the storage facade.
pub(crate) use tau_core::storage::list_sessions_in;
#[allow(unused_imports)]
pub use tau_core::storage::pi_agent_dir;
use tau_core::storage::{archive_revision, mark_archive_intent};
#[allow(unused_imports)]
pub(crate) use tau_core::storage::{default_session_dir, remote_project};

use crate::{
    models::{ProjectSummary, SessionMutationResult, SessionSummary, WorkspaceSnapshot},
    telemetry::{trace_context::TraceContext, Telemetry},
};
use tauri::State;

/// Missing or malformed telemetry context must never fail a storage command.
#[tauri::command]
pub async fn load_workspace(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<WorkspaceSnapshot, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "load_workspace"));
    run_storage_worker(tau_core::storage::load_workspace).await
}

#[tauri::command]
pub async fn import_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    path: String,
) -> Result<ProjectSummary, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "import_project"));
    run_storage_worker(move || tau_core::storage::import_project(path)).await
}

#[tauri::command]
pub async fn import_remote_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
    working_directory: String,
    host: String,
) -> Result<ProjectSummary, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "import_remote_project"));
    run_storage_worker(move || {
        tau_core::storage::import_remote_project(connection_string, working_directory, host)
    })
    .await
}

#[tauri::command]
pub async fn remove_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    path: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "remove_project"));
    run_storage_worker(move || tau_core::storage::remove_project(path)).await
}

#[tauri::command]
pub async fn set_active_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    path: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "set_active_project"));
    run_storage_worker(move || tau_core::storage::set_active_project(path)).await
}

#[tauri::command]
pub async fn set_project_collapsed(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    path: String,
    collapsed: bool,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "set_project_collapsed"));
    run_storage_worker(move || tau_core::storage::set_project_collapsed(path, collapsed)).await
}

#[tauri::command]
pub async fn reorder_projects(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_paths: Vec<String>,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "reorder_projects"));
    run_storage_worker(move || tau_core::storage::reorder_projects(project_paths)).await
}

#[tauri::command]
pub async fn set_active_session(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: String,
    session_id: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "set_active_session"));
    run_storage_worker(move || tau_core::storage::set_active_session(project_path, session_id))
        .await
}

#[tauri::command]
pub async fn archive_session(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: String,
    session_id: String,
) -> Result<SessionMutationResult, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "archive_session"));
    mark_archive_intent(&project_path, &session_id)?;
    run_storage_worker(move || {
        tau_core::storage::archive_session_after_intent(&project_path, &session_id, true)
    })
    .await
}

#[tauri::command]
pub async fn unarchive_session(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: String,
    session_id: String,
) -> Result<SessionMutationResult, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "unarchive_session"));
    mark_archive_intent(&project_path, &session_id)?;
    run_storage_worker(move || {
        tau_core::storage::archive_session_after_intent(&project_path, &session_id, false)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn register_session(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: Option<String>,
    last_user_message_at: Option<u64>,
    adopted: Option<bool>,
) -> Result<Option<SessionSummary>, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "register_session"));
    let revision = archive_revision(&project_path, &session_id)?;
    run_storage_worker(move || {
        tau_core::storage::register_session_at_revision(
            project_path,
            session_id,
            session_path,
            session_name,
            last_user_message_at,
            adopted,
            revision,
        )
    })
    .await
}

async fn run_storage_worker<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "Tau storage worker is unavailable.".to_string())?
}
