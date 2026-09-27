mod discovery;
mod projects;
mod registry;
mod sessions;

use projects::{
    import_project_inner, import_remote_project_inner, remove_project_inner,
    reorder_projects_inner, set_active_project_inner, set_project_collapsed_inner, snapshot,
};
use registry::{archive_revision, load_project_registry, mark_archive_intent};
use sessions::{mutate_session_archive, register_session_inner, set_active_session_inner};

pub use registry::pi_agent_dir;
pub(crate) use registry::{default_session_dir, remote_project};
#[allow(unused_imports)] // Retained for crate-internal callers of the storage facade.
pub(crate) use sessions::list_sessions_in;

use crate::{
    models::{ProjectSummary, SessionMutationResult, SessionSummary, WorkspaceSnapshot},
    telemetry::{trace_context::TraceContext, Telemetry},
};
use tauri::State;

/// Every storage command below accepts the same optional, explicit
/// `telemetry_context`: a caller without a span (or a malformed one) still
/// runs normally, since telemetry failure must never fail the command it is
/// attached to. Stage 2 proved the pattern end to end on this one command;
/// Stage 3 applies it to the rest of storage's ordinary invokes.
#[tauri::command]
pub async fn load_workspace(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<WorkspaceSnapshot, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "load_workspace"));
    run_storage_worker(|| snapshot(&load_project_registry()?)).await
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
    run_storage_worker(move || import_project_inner(path)).await
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
        import_remote_project_inner(connection_string, working_directory, host)
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
    run_storage_worker(move || remove_project_inner(path)).await
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
    run_storage_worker(move || set_active_project_inner(path)).await
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
    run_storage_worker(move || set_project_collapsed_inner(path, collapsed)).await
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
    run_storage_worker(move || reorder_projects_inner(project_paths)).await
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
    run_storage_worker(move || set_active_session_inner(project_path, session_id)).await
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
    run_storage_worker(move || mutate_session_archive(&project_path, &session_id, true)).await
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
    run_storage_worker(move || mutate_session_archive(&project_path, &session_id, false)).await
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
        register_session_inner(
            project_path,
            session_id,
            session_path,
            session_name.unwrap_or_default(),
            last_user_message_at,
            adopted.unwrap_or(false),
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
