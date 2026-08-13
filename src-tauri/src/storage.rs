use crate::{
    models::{
        ProjectRecord, ProjectRegistry, ProjectSummary, RemoteProjectRecord, RemoteSessionRecord,
        SessionSummary, TauSessionRecord, TauSessionRegistry, WorkspaceSnapshot,
    },
    pi::{resolve_pi_binary, sdk_available},
    profile::{APP_DIRECTORY_NAME, SESSION_REGISTRY_FILENAME},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

const MAX_SESSION_LINE_BYTES: usize = 64 * 1024 * 1024;
static STORAGE_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub fn load_workspace() -> Result<WorkspaceSnapshot, String> {
    snapshot(&load_project_registry()?)
}

#[tauri::command]
pub fn import_project(path: String) -> Result<WorkspaceSnapshot, String> {
    let path = normalized_project_path(&path)?;
    mutate_projects(|registry| {
        if !registry.projects.iter().any(|project| project.path == path) {
            registry.projects.push(ProjectRecord {
                path: path.clone(),
                collapsed: false,
                remote: None,
            });
        }
        if registry.active_project_path.is_empty() {
            registry.active_project_path = path;
        }
        Ok(())
    })
}

#[tauri::command]
pub fn import_remote_project(
    connection_string: String,
    working_directory: String,
    host: String,
) -> Result<WorkspaceSnapshot, String> {
    let connection_string = connection_string.trim().to_string();
    let working_directory = working_directory.trim_end_matches('/').to_string();
    let working_directory = if working_directory.is_empty() {
        "/".to_string()
    } else {
        working_directory
    };
    let host = host.trim().to_string();
    if connection_string.is_empty() || !working_directory.starts_with('/') || host.is_empty() {
        return Err("The selected remote directory is invalid.".into());
    }
    let new_path = remote_project_path(&connection_string, &working_directory)?;

    mutate_projects(|registry| {
        let path = registry
            .projects
            .iter()
            .find(|project| {
                project.remote.as_ref().is_some_and(|remote| {
                    remote.connection_string == connection_string
                        && remote.working_directory == working_directory
                })
            })
            .map(|project| project.path.clone())
            .unwrap_or_else(|| new_path.clone());
        if let Some(project) = registry
            .projects
            .iter_mut()
            .find(|project| project.path == path)
        {
            let previous = project.remote.take();
            project.remote = Some(RemoteProjectRecord {
                connection_string,
                working_directory,
                host,
                active_session_id: previous
                    .as_ref()
                    .map(|remote| remote.active_session_id.clone())
                    .unwrap_or_default(),
                sessions: previous.map(|remote| remote.sessions).unwrap_or_default(),
            });
        } else {
            registry.projects.push(ProjectRecord {
                path: path.clone(),
                collapsed: false,
                remote: Some(RemoteProjectRecord {
                    connection_string,
                    working_directory,
                    host,
                    active_session_id: String::new(),
                    sessions: Vec::new(),
                }),
            });
        }
        if registry.active_project_path.is_empty() {
            registry.active_project_path = path;
        }
        Ok(())
    })
}

#[tauri::command]
pub fn remove_project(path: String) -> Result<WorkspaceSnapshot, String> {
    mutate_projects(|registry| {
        registry.projects.retain(|project| project.path != path);
        if registry.active_project_path == path {
            registry.active_project_path.clear();
        }
        Ok(())
    })
}

#[tauri::command]
pub fn set_active_project(path: String) -> Result<WorkspaceSnapshot, String> {
    mutate_projects(|registry| {
        if !registry.projects.iter().any(|project| project.path == path) {
            return Err("The project is not imported in Tau.".into());
        }
        registry.active_project_path = path;
        Ok(())
    })
}

#[tauri::command]
pub fn set_project_collapsed(path: String, collapsed: bool) -> Result<WorkspaceSnapshot, String> {
    mutate_projects(|registry| {
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.path == path)
            .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
        project.collapsed = collapsed;
        Ok(())
    })
}

#[tauri::command]
pub fn reorder_projects(project_paths: Vec<String>) -> Result<WorkspaceSnapshot, String> {
    mutate_projects(|registry| reorder_project_records(&mut registry.projects, &project_paths))
}

#[tauri::command]
pub fn set_active_session(
    project_path: String,
    session_id: String,
) -> Result<WorkspaceSnapshot, String> {
    let _write_guard = lock_storage_writes()?;
    let mut projects = load_project_registry()?;
    let project = projects
        .projects
        .iter_mut()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;

    if let Some(remote) = project.remote.as_mut() {
        if !remote
            .sessions
            .iter()
            .any(|session| session.id == session_id && !session.archived)
        {
            return Err("The selected session is not available in Tau.".into());
        }
        remote.active_session_id = session_id;
        projects.active_project_path = project_path;
        write_json_atomic(&project_registry_path()?, &projects)?;
        return snapshot(&projects);
    }

    let session_dir = default_session_dir(&project_path)?;
    let registry_path = session_dir.join(SESSION_REGISTRY_FILENAME);
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    if !registry
        .sessions
        .iter()
        .any(|session| session.id == session_id && !session.archived)
    {
        return Err("The selected session is not available in Tau.".into());
    }
    registry.active_session_id = session_id;
    projects.active_project_path = project_path;
    write_json_atomic(&registry_path, &registry)?;
    write_json_atomic(&project_registry_path()?, &projects)?;
    snapshot(&projects)
}

#[tauri::command]
pub fn archive_session(
    project_path: String,
    session_id: String,
) -> Result<WorkspaceSnapshot, String> {
    let _write_guard = lock_storage_writes()?;
    let mut projects = load_project_registry()?;
    let project = projects
        .projects
        .iter_mut()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;

    if let Some(remote) = project.remote.as_mut() {
        archive_remote_session(remote, &session_id)?;
        write_json_atomic(&project_registry_path()?, &projects)?;
        return snapshot(&projects);
    }

    let session_dir = default_session_dir(&project_path)?;
    let registry_path = session_dir.join(SESSION_REGISTRY_FILENAME);
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    archive_local_session(&mut registry, &session_id)?;
    write_json_atomic(&registry_path, &registry)?;
    snapshot(&projects)
}

#[tauri::command]
pub fn register_session(
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: Option<String>,
    last_user_message_at: Option<u64>,
) -> Result<WorkspaceSnapshot, String> {
    if session_id.trim().is_empty() || session_id.len() > 256 {
        return Err("Pi returned an invalid session id.".into());
    }
    let _write_guard = lock_storage_writes()?;
    let mut projects = load_project_registry()?;
    let project = projects
        .projects
        .iter_mut()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
    if let Some(remote) = project.remote.as_mut() {
        let name = session_name.unwrap_or_default();
        if let Some(session) = remote
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.path = session_path;
            if let Some(timestamp) = last_user_message_at {
                session.last_active = timestamp;
            }
            if !name.is_empty() {
                session.name = Some(name);
            }
        } else {
            remote.sessions.push(RemoteSessionRecord {
                id: session_id,
                path: session_path,
                name: (!name.is_empty()).then_some(name),
                archived: false,
                last_active: last_user_message_at.unwrap_or_default(),
            });
        }
        write_json_atomic(&project_registry_path()?, &projects)?;
        return snapshot(&projects);
    }

    let session_path = PathBuf::from(session_path);
    let session_dir = session_path
        .parent()
        .ok_or_else(|| "Pi returned an invalid session path.".to_string())?;
    let registry_path = session_dir.join(SESSION_REGISTRY_FILENAME);
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    let name = session_name.unwrap_or_default();
    if let Some(session) = registry
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        if !name.is_empty() {
            session.name = Some(name);
        }
    } else {
        registry.sessions.push(TauSessionRecord {
            id: session_id,
            name: (!name.is_empty()).then_some(name),
            archived: false,
        });
    }
    write_json_atomic(&registry_path, &registry)?;
    snapshot(&projects)
}

fn archive_remote_session(
    remote: &mut RemoteProjectRecord,
    session_id: &str,
) -> Result<(), String> {
    let session = remote
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "The session is not registered in Tau.".to_string())?;
    session.archived = true;
    if remote.active_session_id == session_id {
        remote.active_session_id.clear();
    }
    Ok(())
}

fn archive_local_session(
    registry: &mut TauSessionRegistry,
    session_id: &str,
) -> Result<(), String> {
    let session = registry
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "The session is not registered in Tau.".to_string())?;
    session.archived = true;
    if registry.active_session_id == session_id {
        registry.active_session_id.clear();
    }
    Ok(())
}

fn reorder_project_records(
    projects: &mut Vec<ProjectRecord>,
    project_paths: &[String],
) -> Result<(), String> {
    const INVALID_ORDER: &str = "Project order must include every imported project exactly once.";
    if projects.len() != project_paths.len() {
        return Err(INVALID_ORDER.into());
    }

    let reordered = {
        let projects_by_path = projects
            .iter()
            .map(|project| (project.path.as_str(), project))
            .collect::<HashMap<_, _>>();
        if projects_by_path.len() != projects.len() {
            return Err(INVALID_ORDER.into());
        }

        let mut seen = HashSet::with_capacity(project_paths.len());
        let mut reordered = Vec::with_capacity(project_paths.len());
        for path in project_paths {
            if !seen.insert(path.as_str()) {
                return Err(INVALID_ORDER.into());
            }
            let project = projects_by_path
                .get(path.as_str())
                .ok_or_else(|| INVALID_ORDER.to_string())?;
            reordered.push((*project).clone());
        }
        reordered
    };

    *projects = reordered;
    Ok(())
}

fn mutate_projects(
    mutation: impl FnOnce(&mut ProjectRegistry) -> Result<(), String>,
) -> Result<WorkspaceSnapshot, String> {
    let _write_guard = lock_storage_writes()?;
    let mut registry = load_project_registry()?;
    mutation(&mut registry)?;
    write_json_atomic(&project_registry_path()?, &registry)?;
    snapshot(&registry)
}

fn lock_storage_writes() -> Result<MutexGuard<'static, ()>, String> {
    STORAGE_WRITE_LOCK
        .lock()
        .map_err(|_| "Tau storage is unavailable.".to_string())
}

fn snapshot(registry: &ProjectRegistry) -> Result<WorkspaceSnapshot, String> {
    let projects = registry
        .projects
        .iter()
        .map(|project| {
            let selected = project.path == registry.active_project_path;
            let (name, working_directory, connection_string, sessions) =
                if let Some(remote) = project.remote.as_ref() {
                    (
                        remote_project_name(remote),
                        remote.working_directory.clone(),
                        Some(remote.connection_string.clone()),
                        list_remote_sessions(remote),
                    )
                } else {
                    (
                        project_name(&project.path),
                        project.path.clone(),
                        None,
                        list_project_sessions(&project.path)?,
                    )
                };
            Ok(ProjectSummary {
                name,
                path: project.path.clone(),
                working_directory,
                connection_string,
                collapsed: project.collapsed,
                selected,
                sessions,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(WorkspaceSnapshot {
        active_project_path: registry.active_project_path.clone(),
        pi_path: resolve_pi_binary().map(|path| path.to_string_lossy().into_owned()),
        sdk_available: sdk_available(),
        projects,
    })
}

fn project_registry_path() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|path| path.join(APP_DIRECTORY_NAME).join("projects.json"))
        .ok_or_else(|| "Could not locate the application data folder.".into())
}

fn load_project_registry() -> Result<ProjectRegistry, String> {
    read_json_or_default(&project_registry_path()?)
}

fn normalized_project_path(path: &str) -> Result<String, String> {
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err("The selected project folder does not exist.".into());
    }
    let path = path
        .canonicalize()
        .map_err(|error| format!("Could not read the selected project folder: {error}"))?;
    Ok(path.to_string_lossy().trim_end_matches('/').to_string())
}

fn remote_project_path(connection_string: &str, working_directory: &str) -> Result<String, String> {
    let identity = serde_json::to_string(&(connection_string, working_directory))
        .map_err(|error| format!("Could not encode the remote project identity: {error}"))?;
    Ok(format!("ssh:{identity}"))
}

fn remote_project_name(remote: &RemoteProjectRecord) -> String {
    Path::new(&remote.working_directory)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&remote.host)
        .to_string()
}

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_string()
}

pub fn pi_agent_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PI_CODING_AGENT_DIR") {
        return Ok(PathBuf::from(path));
    }
    dirs::home_dir()
        .map(|path| path.join(".pi/agent"))
        .ok_or_else(|| "Could not locate the home folder.".into())
}

fn default_session_dir(project_path: &str) -> Result<PathBuf, String> {
    let safe_path = project_path
        .trim_start_matches(['/', '\\'])
        .replace(['/', '\\', ':'], "-");
    Ok(pi_agent_dir()?
        .join("sessions")
        .join(format!("--{safe_path}--")))
}

fn list_remote_sessions(remote: &RemoteProjectRecord) -> Vec<SessionSummary> {
    let mut sessions = remote
        .sessions
        .iter()
        .map(|session| {
            let last_user_message_at = timestamp_millis(session.last_active);
            SessionSummary {
                id: session.id.clone(),
                path: session.path.clone(),
                title: single_line(
                    session
                        .name
                        .as_deref()
                        .filter(|name| !name.is_empty())
                        .unwrap_or("New session"),
                ),
                last_active: relative_timestamp(last_user_message_at),
                last_user_message_at,
                archived: session.archived,
                selected: !session.archived && session.id == remote.active_session_id,
            }
        })
        .collect::<Vec<_>>();
    sort_sessions(&mut sessions);
    sessions
}

fn list_project_sessions(project_path: &str) -> Result<Vec<SessionSummary>, String> {
    let session_dir = default_session_dir(project_path)?;
    let registry: TauSessionRegistry =
        read_json_or_default(&session_dir.join(SESSION_REGISTRY_FILENAME))?;
    if registry.sessions.is_empty() || !session_dir.is_dir() {
        return Ok(Vec::new());
    }
    let records: HashMap<&str, &TauSessionRecord> = registry
        .sessions
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect();
    let mut sessions = Vec::new();
    let entries = fs::read_dir(&session_dir)
        .map_err(|error| format!("Could not read saved Pi sessions: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(parsed) = parse_session_file(&path)? else {
            continue;
        };
        let Some(record) = records.get(parsed.id.as_str()) else {
            continue;
        };
        let modified = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let title = session_title(&parsed.name, record.name.as_deref(), &parsed.first_message);
        sessions.push(SessionSummary {
            id: parsed.id.clone(),
            path: path.to_string_lossy().into_owned(),
            title: single_line(&title),
            last_active: if parsed.last_user_message_at == 0 {
                relative_time(modified)
            } else {
                relative_timestamp(parsed.last_user_message_at)
            },
            last_user_message_at: parsed.last_user_message_at,
            archived: record.archived,
            selected: !record.archived && parsed.id == registry.active_session_id,
        });
    }
    sort_sessions(&mut sessions);
    Ok(sessions)
}

/// Pi owns the session name: it only writes `session_info` when someone names
/// the session, so a name in the session file outranks Tau's own copy, which
/// also holds titles derived from the first user message.
fn session_title(pi_name: &str, tau_name: Option<&str>, first_message: &str) -> String {
    for candidate in [pi_name, tau_name.unwrap_or_default(), first_message] {
        if !candidate.is_empty() {
            return candidate.to_string();
        }
    }
    "New session".into()
}

struct ParsedSession {
    id: String,
    name: String,
    first_message: String,
    last_user_message_at: u64,
}

fn parse_session_file(path: &Path) -> Result<Option<ParsedSession>, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not read a saved Pi session: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    let mut parsed = ParsedSession {
        id: String::new(),
        name: String::new(),
        first_message: String::new(),
        last_user_message_at: 0,
    };
    loop {
        bytes.clear();
        let count = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|error| format!("Could not read a saved Pi session: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() > MAX_SESSION_LINE_BYTES {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session") if parsed.id.is_empty() => {
                parsed.id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            Some("session_info") => {
                parsed.name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            Some("message") => {
                let Some(message) = value.get("message") else {
                    continue;
                };
                if message.get("role").and_then(Value::as_str) == Some("user") {
                    if parsed.first_message.is_empty() {
                        parsed.first_message = content_text(message.get("content"));
                    }
                    if let Some(timestamp) = message.get("timestamp").and_then(Value::as_u64) {
                        parsed.last_user_message_at =
                            parsed.last_user_message_at.max(timestamp_millis(timestamp));
                    }
                }
            }
            _ => {}
        }
    }
    Ok((!parsed.id.is_empty()).then_some(parsed))
}

fn content_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                (part.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| part.get("text").and_then(Value::as_str))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn single_line(value: &str) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .chars()
        .take(240)
        .collect()
}

fn sort_sessions(sessions: &mut [SessionSummary]) {
    sessions.sort_by(|left, right| {
        right
            .last_user_message_at
            .cmp(&left.last_user_message_at)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn timestamp_millis(timestamp: u64) -> u64 {
    if timestamp == 0 {
        0
    } else if timestamp < 10_000_000_000 {
        timestamp.saturating_mul(1_000)
    } else {
        timestamp
    }
}

fn relative_timestamp(timestamp: u64) -> String {
    if timestamp == 0 {
        return String::new();
    }
    relative_time(SystemTime::UNIX_EPOCH + Duration::from_millis(timestamp))
}

fn relative_time(modified: SystemTime) -> String {
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    match age.as_secs() {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m", age.as_secs() / 60),
        3_600..=86_399 => format!("{}h", age.as_secs() / 3_600),
        86_400..=604_799 => format!("{}d", age.as_secs() / 86_400),
        604_800..=31_535_999 => format!("{}w", age.as_secs() / 604_800),
        seconds => format!("{}y", seconds / 31_536_000),
    }
}

fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}

fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The settings path is invalid.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Could not encode Tau settings: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_session_path_matches_pi_encoding() {
        let path = default_session_dir("/Users/timur/code/tau").expect("session path");
        assert!(path.ends_with(".pi/agent/sessions/--Users-timur-code-tau--"));
    }

    #[test]
    fn parses_session_identity_and_title() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("session.jsonl");
        let mut file = File::create(&path).expect("session file");
        writeln!(
            file,
            "{{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/tmp\"}}"
        )
        .expect("header");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"Port Tau\"}}],\"timestamp\":1000}}}}"
        )
        .expect("first message");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"Continue\",\"timestamp\":2000}}}}"
        )
        .expect("second message");
        let parsed = parse_session_file(&path)
            .expect("parsed file")
            .expect("session");
        assert_eq!(parsed.id, "session-1");
        assert_eq!(parsed.first_message, "Port Tau");
        assert_eq!(parsed.last_user_message_at, 2_000_000);
    }

    #[test]
    fn session_titles_prefer_the_name_pi_recorded() {
        assert_eq!(
            session_title("Renamed in Pi", Some("Stale Tau title"), "Port Tau"),
            "Renamed in Pi"
        );
        assert_eq!(
            session_title("", Some("Renamed in Tau"), "Port Tau"),
            "Renamed in Tau"
        );
        assert_eq!(session_title("", None, "Port Tau"), "Port Tau");
        assert_eq!(session_title("", Some(""), ""), "New session");
    }

    #[test]
    fn accepts_legacy_null_session_names() {
        let registry: TauSessionRegistry = serde_json::from_str(
            r#"{"version":1,"activeSessionId":"session-1","sessions":[{"id":"session-1","name":null,"archived":false}]}"#,
        )
        .expect("legacy registry");
        assert_eq!(registry.sessions[0].name, None);
    }

    #[test]
    fn legacy_local_projects_remain_local() {
        let project: ProjectRecord =
            serde_json::from_str(r#"{"path":"/tmp/tau","collapsed":false}"#)
                .expect("legacy project");
        assert!(project.remote.is_none());
    }

    #[test]
    fn remote_project_identity_includes_the_working_directory() {
        let first =
            remote_project_path("ssh build-box", "/home/timur/one").expect("first remote project");
        let second =
            remote_project_path("ssh build-box", "/home/timur/two").expect("second remote project");
        assert_ne!(first, second);
    }

    #[test]
    fn remote_project_name_uses_the_selected_directory() {
        let remote = RemoteProjectRecord {
            connection_string: "ssh build-box".into(),
            working_directory: "/users/agent/rhinestone".into(),
            host: "build-box".into(),
            active_session_id: String::new(),
            sessions: Vec::new(),
        };
        assert_eq!(remote_project_name(&remote), "rhinestone");
    }

    #[test]
    fn remote_sessions_are_sorted_by_last_user_message() {
        let remote = RemoteProjectRecord {
            connection_string: "ssh build-box".into(),
            working_directory: "/home/timur".into(),
            host: "build-box".into(),
            active_session_id: "newer".into(),
            sessions: vec![
                RemoteSessionRecord {
                    id: "older".into(),
                    path: "/remote/older.jsonl".into(),
                    name: Some("Older session".into()),
                    archived: false,
                    last_active: 10,
                },
                RemoteSessionRecord {
                    id: "newer".into(),
                    path: "/remote/newer.jsonl".into(),
                    name: Some("Newer session".into()),
                    archived: false,
                    last_active: 20,
                },
            ],
        };
        let sessions = list_remote_sessions(&remote);
        assert_eq!(sessions[0].id, "newer");
        assert_eq!(sessions[0].last_user_message_at, 20_000);
        assert!(sessions[0].selected);
        assert_eq!(sessions[1].title, "Older session");
    }

    #[test]
    fn archiving_sessions_preserves_the_record_and_clears_selection() {
        let mut local = TauSessionRegistry {
            active_session_id: "local-session".into(),
            sessions: vec![TauSessionRecord {
                id: "local-session".into(),
                name: Some("Local session".into()),
                archived: false,
            }],
            ..TauSessionRegistry::default()
        };
        archive_local_session(&mut local, "local-session").expect("archive local session");
        assert!(local.sessions[0].archived);
        assert!(local.active_session_id.is_empty());

        let mut remote = RemoteProjectRecord {
            connection_string: "ssh build-box".into(),
            working_directory: "/home/timur".into(),
            host: "build-box".into(),
            active_session_id: "remote-session".into(),
            sessions: vec![RemoteSessionRecord {
                id: "remote-session".into(),
                path: "/remote/session.jsonl".into(),
                name: Some("Remote session".into()),
                archived: false,
                last_active: 10,
            }],
        };
        archive_remote_session(&mut remote, "remote-session").expect("archive remote session");
        assert!(remote.sessions[0].archived);
        assert!(remote.active_session_id.is_empty());
    }

    #[test]
    fn project_order_requires_every_project_exactly_once() {
        let mut projects = vec![
            ProjectRecord {
                path: "/tmp/alpha".into(),
                collapsed: false,
                remote: None,
            },
            ProjectRecord {
                path: "/tmp/beta".into(),
                collapsed: true,
                remote: None,
            },
            ProjectRecord {
                path: "/tmp/gamma".into(),
                collapsed: false,
                remote: None,
            },
        ];

        reorder_project_records(
            &mut projects,
            &["/tmp/gamma".into(), "/tmp/alpha".into(), "/tmp/beta".into()],
        )
        .expect("valid project order");
        assert_eq!(
            projects
                .iter()
                .map(|project| project.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/tmp/gamma", "/tmp/alpha", "/tmp/beta"]
        );
        assert!(projects[2].collapsed);

        let previous = projects.clone();
        let error = reorder_project_records(
            &mut projects,
            &["/tmp/gamma".into(), "/tmp/gamma".into(), "/tmp/beta".into()],
        )
        .expect_err("duplicate project order");
        assert!(error.contains("exactly once"));
        assert_eq!(projects[0].path, previous[0].path);
        assert_eq!(projects[1].path, previous[1].path);
        assert_eq!(projects[2].path, previous[2].path);
    }

    #[test]
    fn new_registries_start_at_version_one() {
        assert_eq!(ProjectRegistry::default().version, 1);
        assert_eq!(TauSessionRegistry::default().version, 1);
    }
}
