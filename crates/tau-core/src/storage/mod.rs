mod discovery;
mod projects;
mod registry;
mod sessions;

use crate::models::{ProjectSummary, SessionMutationResult, SessionSummary, WorkspaceSnapshot};
use projects::{
    import_project_inner, import_remote_project_inner, remove_project_inner,
    reorder_projects_inner, set_active_project_inner, set_project_collapsed_inner, snapshot,
};
use registry::load_project_registry;
use sessions::{mutate_session_archive, register_session_inner, set_active_session_inner};

pub use registry::{
    archive_revision, default_session_dir, mark_archive_intent, pi_agent_dir, remote_project,
};
pub use sessions::list_sessions_in;

pub fn load_workspace() -> Result<WorkspaceSnapshot, String> {
    snapshot(&load_project_registry()?)
}

pub fn import_project(path: String) -> Result<ProjectSummary, String> {
    import_project_inner(path)
}

pub fn import_remote_project(
    connection_string: String,
    working_directory: String,
    host: String,
) -> Result<ProjectSummary, String> {
    import_remote_project_inner(connection_string, working_directory, host)
}

pub fn remove_project(path: String) -> Result<(), String> {
    remove_project_inner(path)
}

pub fn set_active_project(path: String) -> Result<(), String> {
    set_active_project_inner(path)
}

pub fn set_project_collapsed(path: String, collapsed: bool) -> Result<(), String> {
    set_project_collapsed_inner(path, collapsed)
}

pub fn reorder_projects(project_paths: Vec<String>) -> Result<(), String> {
    reorder_projects_inner(project_paths)
}

pub fn list_sessions(project_path: &str) -> Result<Vec<SessionSummary>, String> {
    let projects = load_project_registry()?;
    let project = projects
        .projects
        .iter()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
    match &project.remote {
        Some(remote) => Ok(sessions::list_remote_sessions(remote)),
        None => sessions::list_project_sessions(project_path),
    }
}

pub fn set_active_session(project_path: String, session_id: String) -> Result<(), String> {
    set_active_session_inner(project_path, session_id)
}

pub fn archive_session(
    project_path: String,
    session_id: String,
) -> Result<SessionMutationResult, String> {
    mark_archive_intent(&project_path, &session_id)?;
    archive_session_after_intent(&project_path, &session_id, true)
}

pub fn unarchive_session(
    project_path: String,
    session_id: String,
) -> Result<SessionMutationResult, String> {
    mark_archive_intent(&project_path, &session_id)?;
    archive_session_after_intent(&project_path, &session_id, false)
}

/// Async shells mark intent at dispatch, before scheduling this worker operation.
pub fn archive_session_after_intent(
    project_path: &str,
    session_id: &str,
    archived: bool,
) -> Result<SessionMutationResult, String> {
    mutate_session_archive(project_path, session_id, archived)
}

pub fn register_session(
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: Option<String>,
    last_user_message_at: Option<u64>,
    adopted: Option<bool>,
) -> Result<Option<SessionSummary>, String> {
    let revision = archive_revision(&project_path, &session_id)?;
    register_session_at_revision(
        project_path,
        session_id,
        session_path,
        session_name,
        last_user_message_at,
        adopted,
        revision,
    )
}

/// Capture the revision at dispatch to fence adoption against later archive intentions.
#[allow(clippy::too_many_arguments)]
pub fn register_session_at_revision(
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: Option<String>,
    last_user_message_at: Option<u64>,
    adopted: Option<bool>,
    revision: u64,
) -> Result<Option<SessionSummary>, String> {
    register_session_inner(
        project_path,
        session_id,
        session_path,
        session_name.unwrap_or_default(),
        last_user_message_at,
        adopted.unwrap_or(false),
        revision,
    )
}
