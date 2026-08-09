use crate::{
    models::{
        ProjectRecord, ProjectRegistry, ProjectSummary, SessionSummary, TauSessionRecord,
        TauSessionRegistry, WorkspaceSnapshot,
    },
    pi::{resolve_pi_binary, sdk_available},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_SESSION_LINE_BYTES: usize = 64 * 1024 * 1024;

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
pub fn register_session(
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: Option<String>,
) -> Result<WorkspaceSnapshot, String> {
    if session_id.trim().is_empty() || session_id.len() > 256 {
        return Err("Pi returned an invalid session id.".into());
    }
    let session_path = PathBuf::from(session_path);
    let session_dir = session_path
        .parent()
        .ok_or_else(|| "Pi returned an invalid session path.".to_string())?;
    let registry_path = session_dir.join(".tau.json");
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    registry.active_session_id = session_id.clone();
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
    set_active_project(project_path)
}

fn mutate_projects(
    mutation: impl FnOnce(&mut ProjectRegistry) -> Result<(), String>,
) -> Result<WorkspaceSnapshot, String> {
    let mut registry = load_project_registry()?;
    mutation(&mut registry)?;
    write_json_atomic(&project_registry_path()?, &registry)?;
    snapshot(&registry)
}

fn snapshot(registry: &ProjectRegistry) -> Result<WorkspaceSnapshot, String> {
    let projects = registry
        .projects
        .iter()
        .map(|project| {
            let selected = project.path == registry.active_project_path;
            Ok(ProjectSummary {
                name: project_name(&project.path),
                path: project.path.clone(),
                collapsed: project.collapsed,
                selected,
                sessions: list_project_sessions(&project.path)?,
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
        .map(|path| path.join("tau/projects.json"))
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

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_string()
}

fn pi_agent_dir() -> Result<PathBuf, String> {
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

fn list_project_sessions(project_path: &str) -> Result<Vec<SessionSummary>, String> {
    let session_dir = default_session_dir(project_path)?;
    let registry: TauSessionRegistry = read_json_or_default(&session_dir.join(".tau.json"))?;
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
        let title = if let Some(name) = record.name.as_ref().filter(|name| !name.is_empty()) {
            name.clone()
        } else if !parsed.name.is_empty() {
            parsed.name
        } else if !parsed.first_message.is_empty() {
            parsed.first_message
        } else {
            "New session".into()
        };
        sessions.push((
            modified,
            SessionSummary {
                id: parsed.id.clone(),
                path: path.to_string_lossy().into_owned(),
                title: single_line(&title),
                last_active: relative_time(modified),
                archived: record.archived,
                selected: parsed.id == registry.active_session_id,
            },
        ));
    }
    sessions.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(sessions.into_iter().map(|(_, session)| session).collect())
}

struct ParsedSession {
    id: String,
    name: String,
    first_message: String,
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
            Some("message") if parsed.first_message.is_empty() => {
                let Some(message) = value.get("message") else {
                    continue;
                };
                if message.get("role").and_then(Value::as_str) == Some("user") {
                    parsed.first_message = content_text(message.get("content"));
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
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"Port Tau\"}}]}}}}"
        )
        .expect("message");
        let parsed = parse_session_file(&path)
            .expect("parsed file")
            .expect("session");
        assert_eq!(parsed.id, "session-1");
        assert_eq!(parsed.first_message, "Port Tau");
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
    fn new_registries_start_at_version_one() {
        assert_eq!(ProjectRegistry::default().version, 1);
        assert_eq!(TauSessionRegistry::default().version, 1);
    }
}
