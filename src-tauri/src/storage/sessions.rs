use super::{
    discovery::{
        discover_local_session, parse_session_file, single_line, timestamp_millis, ParsedSession,
    },
    registry::{
        adoption_is_current, default_session_dir, load_project_registry, local_registry_path,
        lock_storage_writes, project_registry_path, read_json_or_default, write_json_atomic,
    },
};
use crate::{
    models::{
        ProjectRegistry, RemoteProjectRecord, RemoteSessionRecord, SessionMutationResult,
        SessionSummary, TauSessionRecord, TauSessionRegistry,
    },
    profile::SESSION_REGISTRY_FILENAME,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn write_projects_if_changed(
    projects: &mut ProjectRegistry,
    project_path: &str,
) -> Result<(), String> {
    if projects.active_project_path != project_path {
        projects.active_project_path = project_path.to_string();
        write_json_atomic(&project_registry_path()?, projects)?;
    }
    Ok(())
}
pub(super) fn mutate_session_archive(
    project_path: &str,
    session_id: &str,
    archived: bool,
) -> Result<SessionMutationResult, String> {
    let _write_guard = lock_storage_writes()?;
    let mut projects = load_project_registry()?;
    let project = projects
        .projects
        .iter_mut()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
    let active_session_id = if let Some(remote) = project.remote.as_mut() {
        let before = serde_json::to_vec(&remote).map_err(|error| error.to_string())?;
        set_remote_session_archived(remote, session_id, archived)?;
        let active = remote.active_session_id.clone();
        if serde_json::to_vec(&remote).map_err(|error| error.to_string())? != before {
            write_json_atomic(&project_registry_path()?, &projects)?;
        }
        active
    } else {
        let path = local_registry_path(project_path)?;
        let mut registry: TauSessionRegistry = read_json_or_default(&path)?;
        let before = serde_json::to_vec(&registry).map_err(|error| error.to_string())?;
        set_local_session_archived(&mut registry, session_id, archived)?;
        if serde_json::to_vec(&registry).map_err(|error| error.to_string())? != before {
            write_json_atomic(&path, &registry)?;
        }
        registry.active_session_id
    };
    Ok(SessionMutationResult {
        session_id: session_id.to_string(),
        archived,
        active_session_id,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(super) fn register_session_inner(
    project_path: String,
    session_id: String,
    session_path: String,
    session_name: String,
    last_user_message_at: Option<u64>,
    adopted: bool,
    revision: u64,
) -> Result<Option<SessionSummary>, String> {
    if session_id.trim().is_empty() || session_id.len() > 256 {
        return Err("Pi returned an invalid session id.".into());
    }
    let projects = load_project_registry()?;
    let project = projects
        .projects
        .iter()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
    if project.remote.is_some() {
        let _write_guard = lock_storage_writes()?;
        let mut projects = load_project_registry()?;
        let remote = projects
            .projects
            .iter_mut()
            .find(|project| project.path == project_path)
            .and_then(|project| project.remote.as_mut())
            .ok_or_else(|| "The remote project is no longer available.".to_string())?;
        let before = serde_json::to_vec(&remote).map_err(|error| error.to_string())?;
        upsert_remote_session(
            remote,
            session_id.clone(),
            session_path,
            session_name,
            last_user_message_at,
            adopted && adoption_is_current(&project_path, &session_id, revision)?,
        );
        let changed = serde_json::to_vec(&remote).map_err(|error| error.to_string())? != before;
        let result = list_remote_sessions(remote)
            .into_iter()
            .find(|session| session.id == session_id);
        if changed {
            write_json_atomic(&project_registry_path()?, &projects)?;
        }
        return Ok(result);
    }

    let directory = default_session_dir(&project_path)?;
    register_local_session_in(
        &directory,
        &session_id,
        &session_path,
        session_name,
        adopted,
        Some(&project_path),
        revision,
    )
}

fn register_local_session_in(
    directory: &Path,
    session_id: &str,
    session_path: &str,
    session_name: String,
    adopted: bool,
    project_path: Option<&str>,
    revision: u64,
) -> Result<Option<SessionSummary>, String> {
    let Some(path) = discover_local_session(directory, session_path, session_id)? else {
        return Ok(None);
    };
    let parsed = parse_session_file(&path)?
        .ok_or_else(|| "The Pi session is no longer available.".to_string())?;
    if parsed.id != session_id {
        return Ok(None);
    }
    let modified = path
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let _write_guard = lock_storage_writes()?;
    if let Some(project_path) = project_path {
        let projects = load_project_registry()?;
        if !projects
            .projects
            .iter()
            .any(|project| project.path == project_path && project.remote.is_none())
        {
            return Err("The project is not imported in Tau.".into());
        }
    }
    let registry_path = directory.join(SESSION_REGISTRY_FILENAME);
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    let before = serde_json::to_vec(&registry).map_err(|error| error.to_string())?;
    let adopt = adopted
        && project_path.map_or(Ok(true), |project| {
            adoption_is_current(project, session_id, revision)
        })?;
    upsert_local_session(
        &mut registry,
        session_id.to_string(),
        path.to_string_lossy().into_owned(),
        session_name,
        adopt,
    );
    let record = registry
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .unwrap();
    let result = local_session_summary(&path, &registry, record, parsed, modified);
    if serde_json::to_vec(&registry).map_err(|error| error.to_string())? != before {
        write_json_atomic(&registry_path, &registry)?;
    }
    Ok(Some(result))
}

fn local_session_summary(
    path: &Path,
    registry: &TauSessionRegistry,
    record: &TauSessionRecord,
    parsed: ParsedSession,
    modified: SystemTime,
) -> SessionSummary {
    let title = session_title(&parsed.name, record.name.as_deref(), &parsed.first_message);
    let sort_at = parsed.sort_at();
    SessionSummary {
        id: parsed.id,
        path: path.to_string_lossy().into_owned(),
        title: single_line(&title),
        title_markdown: Some(markdown_title(&title)),
        model: parsed.model,
        last_active: if sort_at == 0 {
            relative_time(modified)
        } else {
            relative_timestamp(sort_at)
        },
        last_user_message_at: parsed.last_user_message_at,
        sort_at,
        archived: record.archived,
        selected: !record.archived && record.id == registry.active_session_id,
    }
}

fn upsert_remote_session(
    remote: &mut RemoteProjectRecord,
    session_id: String,
    session_path: String,
    name: String,
    last_user_message_at: Option<u64>,
    adopted: bool,
) {
    let sort_at = last_user_message_at.unwrap_or_else(unix_timestamp_millis);
    if let Some(session) = remote
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.path = session_path;
        if let Some(timestamp) = last_user_message_at {
            session.last_active = timestamp;
            session.sort_at = timestamp;
        } else if session.sort_at == 0 {
            session.sort_at = sort_at;
        }
        if !name.is_empty() {
            session.name = Some(name);
        }
        if adopted {
            session.archived = false;
        }
        return;
    }
    remote.sessions.push(RemoteSessionRecord {
        id: session_id,
        path: session_path,
        name: (!name.is_empty()).then_some(name),
        archived: false,
        last_active: last_user_message_at.unwrap_or_default(),
        sort_at,
    });
}

fn upsert_local_session(
    registry: &mut TauSessionRegistry,
    session_id: String,
    session_path: String,
    name: String,
    adopted: bool,
) {
    if let Some(session) = registry
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.path = Some(session_path);
        if !name.is_empty() {
            session.name = Some(name);
        }
        if adopted {
            session.archived = false;
        }
        return;
    }
    registry.sessions.push(TauSessionRecord {
        id: session_id,
        path: Some(session_path),
        name: (!name.is_empty()).then_some(name),
        archived: false,
    });
}

fn select_remote_session(remote: &mut RemoteProjectRecord, session_id: &str) -> Result<(), String> {
    if !remote
        .sessions
        .iter()
        .any(|session| session.id == session_id)
    {
        return Err("The selected session is not available in Tau.".into());
    }
    remote.active_session_id = session_id.to_string();
    Ok(())
}

fn select_local_session(registry: &mut TauSessionRegistry, session_id: &str) -> Result<(), String> {
    if !registry
        .sessions
        .iter()
        .any(|session| session.id == session_id)
    {
        return Err("The selected session is not available in Tau.".into());
    }
    registry.active_session_id = session_id.to_string();
    Ok(())
}

fn set_remote_session_archived(
    remote: &mut RemoteProjectRecord,
    session_id: &str,
    archived: bool,
) -> Result<(), String> {
    let session = remote
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "The session is not registered in Tau.".to_string())?;
    session.archived = archived;
    if archived && remote.active_session_id == session_id {
        remote.active_session_id.clear();
    }
    Ok(())
}

fn set_local_session_archived(
    registry: &mut TauSessionRegistry,
    session_id: &str,
    archived: bool,
) -> Result<(), String> {
    let session = registry
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .ok_or_else(|| "The session is not registered in Tau.".to_string())?;
    session.archived = archived;
    if archived && registry.active_session_id == session_id {
        registry.active_session_id.clear();
    }
    Ok(())
}

pub(super) fn list_remote_sessions(remote: &RemoteProjectRecord) -> Vec<SessionSummary> {
    let mut sessions = remote
        .sessions
        .iter()
        .map(|session| {
            let last_user_message_at = timestamp_millis(session.last_active);
            let sort_at = timestamp_millis(session.sort_at).max(last_user_message_at);
            let title = session
                .name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or("New Session");
            SessionSummary {
                id: session.id.clone(),
                path: session.path.clone(),
                title: single_line(title),
                title_markdown: Some(markdown_title(title)),
                // Remote registries do not carry a model; the archived list
                // omits the model line when it is empty.
                model: String::new(),
                last_active: relative_timestamp(sort_at),
                last_user_message_at,
                sort_at,
                archived: session.archived,
                selected: !session.archived && session.id == remote.active_session_id,
            }
        })
        .collect::<Vec<_>>();
    sort_sessions(&mut sessions);
    sessions
}

pub(super) fn list_project_sessions(project_path: &str) -> Result<Vec<SessionSummary>, String> {
    list_sessions_in(&default_session_dir(project_path)?)
}

pub(crate) fn list_sessions_in(session_dir: &Path) -> Result<Vec<SessionSummary>, String> {
    list_sessions_in_with(session_dir, parse_session_file)
}

fn list_sessions_in_with(
    session_dir: &Path,
    mut parse: impl FnMut(&Path) -> Result<Option<ParsedSession>, String>,
) -> Result<Vec<SessionSummary>, String> {
    let registry: TauSessionRegistry =
        read_json_or_default(&session_dir.join(SESSION_REGISTRY_FILENAME))?;
    let mut legacy = HashMap::new();
    let mut scanned = HashMap::new();
    if registry.sessions.iter().any(|record| record.path.is_none()) && session_dir.is_dir() {
        let mut paths = fs::read_dir(session_dir)
            .map_err(|error| format!("Could not read saved Pi sessions: {error}"))?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| format!("Could not read saved Pi sessions: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort(); // The lexicographically first matching file wins for legacy duplicates.
        let ids: HashSet<&str> = registry
            .sessions
            .iter()
            .filter(|record| record.path.is_none())
            .map(|record| record.id.as_str())
            .collect();
        let explicit_paths: HashSet<PathBuf> = registry
            .sessions
            .iter()
            .filter_map(|record| record.path.as_ref().map(PathBuf::from))
            .collect();
        for path in paths {
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") || !path.is_file()
            {
                continue;
            }
            if let Some(parsed) = parse(&path)? {
                let first_legacy_match =
                    ids.contains(parsed.id.as_str()) && !legacy.contains_key(parsed.id.as_str());
                if first_legacy_match {
                    legacy.insert(parsed.id.clone(), path.clone());
                }
                if first_legacy_match || explicit_paths.contains(&path) {
                    scanned.insert(path, parsed);
                }
            }
        }
    }

    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    for record in &registry.sessions {
        if !seen.insert(record.id.as_str()) {
            continue;
        }
        let path = match &record.path {
            Some(path) if Path::new(path).is_absolute() && path.ends_with(".jsonl") => {
                PathBuf::from(path)
            }
            Some(_) => continue,
            None => match legacy.remove(&record.id) {
                Some(path) => path,
                None => continue,
            },
        };
        let metadata = match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Could not read a saved Pi session: {error}")),
        };
        let Some(parsed) = (match scanned.remove(&path) {
            Some(parsed) => Some(parsed),
            None => parse(&path)?,
        }) else {
            continue;
        };
        if parsed.id != record.id {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let title = session_title(&parsed.name, record.name.as_deref(), &parsed.first_message);
        let sort_at = parsed.sort_at();
        sessions.push(SessionSummary {
            id: parsed.id,
            path: path.to_string_lossy().into_owned(),
            title: single_line(&title),
            title_markdown: Some(markdown_title(&title)),
            model: parsed.model,
            last_active: if sort_at == 0 {
                relative_time(modified)
            } else {
                relative_timestamp(sort_at)
            },
            last_user_message_at: parsed.last_user_message_at,
            sort_at,
            archived: record.archived,
            selected: !record.archived && record.id == registry.active_session_id,
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
    "New Session".into()
}

fn markdown_title(value: &str) -> String {
    value.chars().take(240).collect()
}

fn sort_sessions(sessions: &mut [SessionSummary]) {
    sessions.sort_by(|left, right| {
        right
            .sort_at
            .cmp(&left.sort_at)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
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

pub(super) fn set_active_session_inner(
    project_path: String,
    session_id: String,
) -> Result<(), String> {
    let _write_guard = lock_storage_writes()?;
    let mut projects = load_project_registry()?;
    let project = projects
        .projects
        .iter_mut()
        .find(|project| project.path == project_path)
        .ok_or_else(|| "The project is not imported in Tau.".to_string())?;

    if let Some(remote) = project.remote.as_mut() {
        let changed = remote.active_session_id != session_id;
        select_remote_session(remote, &session_id)?;
        if changed || projects.active_project_path != project_path {
            projects.active_project_path = project_path;
            write_json_atomic(&project_registry_path()?, &projects)?;
        }
        return Ok(());
    }

    let registry_path = local_registry_path(&project_path)?;
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path)?;
    let changed = registry.active_session_id != session_id;
    select_local_session(&mut registry, &session_id)?;
    if changed {
        write_json_atomic(&registry_path, &registry)?;
    }
    write_projects_if_changed(&mut projects, &project_path)
}

#[cfg(test)]
mod tests {
    use super::super::{
        projects::reorder_project_records,
        registry::{
            adoption_is_current, archive_revision, default_session_dir, local_registry_path,
            mark_archive_intent, read_json_or_default, write_json_atomic,
        },
    };
    use super::*;
    use crate::models::ProjectRecord;
    use std::{fs::File, io::Write};
    #[test]
    fn external_registered_transcript_is_discovered_after_reload() {
        let root = tempfile::tempdir().expect("temporary directory");
        let project = root.path().join("project");
        let external = root.path().join("mission");
        fs::create_dir_all(&project).expect("project directory");
        fs::create_dir_all(&external).expect("external directory");
        let path = external.join("session.jsonl");
        fs::write(&path, "{\"type\":\"session\",\"id\":\"external\"}\n").expect("transcript");
        let mut registry = TauSessionRegistry::default();
        upsert_local_session(
            &mut registry,
            "external".into(),
            path.to_string_lossy().into_owned(),
            "Mission".into(),
            false,
        );
        write_json_atomic(&project.join(SESSION_REGISTRY_FILENAME), &registry).expect("registry");
        let sessions = list_sessions_in(&project).expect("reload sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].path, path.to_string_lossy());

        {
            let root = tempfile::tempdir().expect("temporary directory");
            let project = root.path().join("project");
            fs::create_dir(&project).expect("project");
            let first = root.path().join("first.jsonl");
            let second = root.path().join("second.jsonl");
            let default = project.join("default.jsonl");
            let body = "{\"type\":\"session\",\"id\":\"external\"}\n";
            fs::write(&first, body).expect("first");
            fs::write(&default, body).expect("default");
            let mut registry = TauSessionRegistry::default();
            upsert_local_session(
                &mut registry,
                "external".into(),
                first.to_string_lossy().into_owned(),
                "".into(),
                false,
            );
            select_local_session(&mut registry, "external").expect("select");
            set_local_session_archived(&mut registry, "external", true).expect("archive");
            assert!(registry.active_session_id.is_empty());
            upsert_local_session(
                &mut registry,
                "external".into(),
                second.to_string_lossy().into_owned(),
                "".into(),
                false,
            );
            assert_eq!(registry.sessions.len(), 1);
            assert!(registry.sessions[0].archived);
            let save = |registry: &TauSessionRegistry| {
                write_json_atomic(&project.join(SESSION_REGISTRY_FILENAME), registry).expect("save")
            };
            save(&registry);
            assert!(list_sessions_in(&project).expect("missing file").is_empty());
            fs::write(&second, "{\"type\":\"session\",\"id\":\"other\"}\n")
                .expect("wrong identity");
            assert!(list_sessions_in(&project)
                .expect("wrong identity")
                .is_empty());
            fs::write(&second, body).expect("materialize");
            let rows = list_sessions_in(&project).expect("rows");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].path, second.to_string_lossy());
            assert!(rows[0].archived);
            set_local_session_archived(&mut registry, "external", false).expect("unarchive");
            select_local_session(&mut registry, "external").expect("select restored");
            save(&registry);
            let rows = list_sessions_in(&project).expect("restored rows");
            assert!(!rows[0].archived);
            assert!(rows[0].selected);
            assert_eq!(fs::read_to_string(&second).expect("transcript"), body);
        }

        {
            let root = tempfile::tempdir().expect("temporary directory");
            let owner = root.path().join("owner");
            let other = root.path().join("other");
            fs::create_dir(&owner).expect("owner");
            fs::create_dir(&other).expect("other");
            let external = root.path().join("external.jsonl");
            fs::write(
                &external,
                "{\"type\":\"message\"}\n{\"type\":\"session\",\"id\":\"external\"}\n",
            )
            .expect("headerless");
            let mut registry = TauSessionRegistry::default();
            upsert_local_session(
                &mut registry,
                "external".into(),
                external.to_string_lossy().into_owned(),
                "".into(),
                false,
            );
            assert!(list_sessions_in(&other)
                .expect("unregistered project")
                .is_empty());
            write_json_atomic(&owner.join(SESSION_REGISTRY_FILENAME), &registry).expect("registry");
            assert!(list_sessions_in(&owner).expect("headerless").is_empty());
            registry.sessions[0].path = Some("relative.jsonl".into());
            write_json_atomic(&owner.join(SESSION_REGISTRY_FILENAME), &registry).expect("registry");
            assert!(list_sessions_in(&owner).expect("relative").is_empty());
            registry.sessions[0].path =
                Some(owner.join("directory.jsonl").to_string_lossy().into_owned());
            fs::create_dir(owner.join("directory.jsonl")).expect("directory");
            write_json_atomic(&owner.join(SESSION_REGISTRY_FILENAME), &registry).expect("registry");
            assert!(list_sessions_in(&owner).expect("directory").is_empty());
        }

        {
            let root = tempfile::tempdir().expect("temporary directory");
            let project = root.path().join("project");
            fs::create_dir(&project).expect("project");
            let registry: TauSessionRegistry = serde_json::from_str(
                r#"{"sessions":[{"id":"legacy","name":null,"path":null},{"id":"legacy"}]}"#,
            )
            .expect("legacy registry");
            write_json_atomic(&project.join(SESSION_REGISTRY_FILENAME), &registry).expect("save");
            for (file, id) in [
                ("z.jsonl", "legacy"),
                ("a.jsonl", "legacy"),
                ("other.jsonl", "other"),
            ] {
                fs::write(
                    project.join(file),
                    format!("{{\"type\":\"session\",\"id\":\"{id}\"}}\n"),
                )
                .expect("transcript");
            }
            let rows = list_sessions_in(&project).expect("legacy rows");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].path, project.join("a.jsonl").to_string_lossy());
        }

        {
            let root = tempfile::tempdir().expect("temporary directory");
            let project = root.path().join("project");
            fs::create_dir(&project).expect("project");
            let external = root.path().join("external.jsonl");
            let registry = TauSessionRegistry {
                sessions: vec![
                    TauSessionRecord {
                        id: "legacy".into(),
                        path: None,
                        name: None,
                        archived: true,
                    },
                    TauSessionRecord {
                        id: "explicit".into(),
                        path: Some(external.to_string_lossy().into_owned()),
                        name: None,
                        archived: false,
                    },
                    TauSessionRecord {
                        id: "local-explicit".into(),
                        path: Some(project.join("local.jsonl").to_string_lossy().into_owned()),
                        name: None,
                        archived: false,
                    },
                ],
                ..TauSessionRegistry::default()
            };
            write_json_atomic(&project.join(SESSION_REGISTRY_FILENAME), &registry)
                .expect("registry");
            for (path, id, name) in [
                (project.join("a.jsonl"), "legacy", "First legacy"),
                (project.join("z.jsonl"), "legacy", "Second legacy"),
                (project.join("explicit.jsonl"), "explicit", "Wrong location"),
                (project.join("local.jsonl"), "local-explicit", "Local"),
                (external.clone(), "explicit", "External"),
            ] {
                fs::write(
                    path,
                    format!("{{\"type\":\"session\",\"id\":\"{id}\"}}\n{{\"type\":\"session_info\",\"name\":\"{name}\"}}\n"),
                )
                .expect("transcript");
            }
            let mut calls = HashMap::<PathBuf, usize>::new();
            let rows = list_sessions_in_with(&project, |path| {
                *calls.entry(path.to_path_buf()).or_default() += 1;
                parse_session_file(path)
            })
            .expect("sessions");
            assert_eq!(calls.len(), 5);
            assert!(calls.values().all(|count| *count == 1), "{calls:?}");
            assert_eq!(rows.len(), 3);
            let legacy = rows.iter().find(|row| row.id == "legacy").expect("legacy");
            assert_eq!(legacy.path, project.join("a.jsonl").to_string_lossy());
            assert_eq!(legacy.title, "First legacy");
            assert!(legacy.archived);
            let explicit = rows
                .iter()
                .find(|row| row.id == "explicit")
                .expect("explicit");
            assert_eq!(explicit.path, external.to_string_lossy());
            assert_eq!(explicit.title, "External");
            let local = rows
                .iter()
                .find(|row| row.id == "local-explicit")
                .expect("local explicit");
            assert_eq!(local.path, project.join("local.jsonl").to_string_lossy());
            assert_eq!(local.title, "Local");
        }
    }
    #[test]
    fn unarchiving_sessions_preserves_the_record_and_keeps_selection() {
        let mut local = TauSessionRegistry {
            active_session_id: String::new(),
            sessions: vec![TauSessionRecord {
                id: "local-session".into(),
                path: None,
                name: Some("Local session".into()),
                archived: true,
            }],
            ..TauSessionRegistry::default()
        };
        set_local_session_archived(&mut local, "local-session", false)
            .expect("unarchive local session");
        assert!(!local.sessions[0].archived);
        assert!(local.active_session_id.is_empty());

        let mut remote = RemoteProjectRecord {
            connection_string: "ssh build-box".into(),
            working_directory: "/home/timur".into(),
            host: "build-box".into(),
            active_session_id: String::new(),
            sessions: vec![RemoteSessionRecord {
                id: "remote-session".into(),
                path: "/remote/session.jsonl".into(),
                name: Some("Remote session".into()),
                archived: true,
                last_active: 10,
                sort_at: 10,
            }],
        };
        set_remote_session_archived(&mut remote, "remote-session", false)
            .expect("unarchive remote session");
        assert!(!remote.sessions[0].archived);

        let missing = set_local_session_archived(&mut local, "nope", false);
        assert!(missing.is_err());

        {
            let mut local = TauSessionRegistry {
                active_session_id: "local-session".into(),
                sessions: vec![TauSessionRecord {
                    id: "local-session".into(),
                    path: None,
                    name: Some("Local session".into()),
                    archived: false,
                }],
                ..TauSessionRegistry::default()
            };
            set_local_session_archived(&mut local, "local-session", true)
                .expect("archive local session");
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
                    sort_at: 10,
                }],
            };
            set_remote_session_archived(&mut remote, "remote-session", true)
                .expect("archive remote session");
            assert!(remote.sessions[0].archived);
            assert!(remote.active_session_id.is_empty());
        }
    }
    #[test]
    fn archive_intent_fences_older_registration_adoption() {
        let project = "intent-test-project";
        let session = "intent-test-session";
        let before = archive_revision(project, session).unwrap();
        mark_archive_intent(project, session).unwrap();
        assert!(!adoption_is_current(project, session, before).unwrap());
        let after = archive_revision(project, session).unwrap();
        assert!(adoption_is_current(project, session, after).unwrap());

        let mut local = TauSessionRegistry {
            sessions: vec![TauSessionRecord {
                id: session.into(),
                path: None,
                name: None,
                archived: true,
            }],
            ..TauSessionRegistry::default()
        };
        upsert_local_session(
            &mut local,
            session.into(),
            "/tmp/intent-test-session.jsonl".into(),
            String::new(),
            adoption_is_current(project, session, before).unwrap(),
        );
        assert!(local.sessions[0].archived);
        upsert_local_session(
            &mut local,
            session.into(),
            "/tmp/intent-test-session.jsonl".into(),
            String::new(),
            adoption_is_current(project, session, after).unwrap(),
        );
        assert!(!local.sessions[0].archived);

        let mut remote = RemoteProjectRecord {
            connection_string: "ssh build-box".into(),
            working_directory: "/home/timur".into(),
            host: "build-box".into(),
            active_session_id: String::new(),
            sessions: vec![RemoteSessionRecord {
                id: session.into(),
                path: "/remote/session.jsonl".into(),
                name: None,
                archived: true,
                last_active: 0,
                sort_at: 0,
            }],
        };
        upsert_remote_session(
            &mut remote,
            session.into(),
            "/remote/session.jsonl".into(),
            String::new(),
            None,
            adoption_is_current(project, session, before).unwrap(),
        );
        assert!(remote.sessions[0].archived);
        upsert_remote_session(
            &mut remote,
            session.into(),
            "/remote/session.jsonl".into(),
            String::new(),
            None,
            adoption_is_current(project, session, after).unwrap(),
        );
        assert!(!remote.sessions[0].archived);

        {
            let mut remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/home/timur".into(),
                host: "build-box".into(),
                active_session_id: String::new(),
                sessions: vec![RemoteSessionRecord {
                    id: "phase".into(),
                    path: "/remote/phase.jsonl".into(),
                    name: Some("Monitor".into()),
                    archived: true,
                    last_active: 10,
                    sort_at: 10,
                }],
            };

            // A registration Tau did not adopt leaves the archived row alone.
            upsert_remote_session(
                &mut remote,
                "phase".into(),
                "/remote/phase.jsonl".into(),
                "Monitor".into(),
                None,
                false,
            );
            assert!(remote.sessions[0].archived);
            // An archived session stays browsable: opening it from the archived
            // list selects the record without unarchiving it.
            select_remote_session(&mut remote, "phase").expect("select archived session");
            assert_eq!(remote.active_session_id, "phase");

            upsert_remote_session(
                &mut remote,
                "phase".into(),
                "/remote/phase.jsonl".into(),
                "Monitor".into(),
                None,
                true,
            );
            assert!(!remote.sessions[0].archived);
            assert_eq!(remote.sessions[0].last_active, 10);
            select_remote_session(&mut remote, "phase").expect("select the adopted session");
            assert_eq!(remote.active_session_id, "phase");

            let mut local = TauSessionRegistry {
                sessions: vec![TauSessionRecord {
                    id: "phase".into(),
                    path: None,
                    name: Some("Monitor".into()),
                    archived: true,
                }],
                ..TauSessionRegistry::default()
            };
            upsert_local_session(
                &mut local,
                "phase".into(),
                "/tmp/phase.jsonl".into(),
                "Monitor".into(),
                false,
            );
            assert!(local.sessions[0].archived);
            select_local_session(&mut local, "phase").expect("select archived session");
            assert_eq!(local.active_session_id, "phase");
            upsert_local_session(
                &mut local,
                "phase".into(),
                "/tmp/phase.jsonl".into(),
                "Monitor".into(),
                true,
            );
            assert!(!local.sessions[0].archived);
            select_local_session(&mut local, "phase").expect("select the adopted session");
            assert_eq!(local.active_session_id, "phase");
        }
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
        assert_eq!(session_title("", Some(""), ""), "New Session");

        {
            let directory = tempfile::tempdir().expect("temporary directory");
            let registry = TauSessionRegistry {
                sessions: vec![
                    TauSessionRecord {
                        id: "pi".into(),
                        path: None,
                        name: Some("Stale Tau title".into()),
                        archived: false,
                    },
                    TauSessionRecord {
                        id: "registry".into(),
                        path: None,
                        name: Some("Tau *title*\nnext".into()),
                        archived: false,
                    },
                    TauSessionRecord {
                        id: "message".into(),
                        path: None,
                        name: None,
                        archived: false,
                    },
                ],
                ..TauSessionRegistry::default()
            };
            fs::write(
                directory.path().join(SESSION_REGISTRY_FILENAME),
                serde_json::to_vec(&registry).expect("registry JSON"),
            )
            .expect("write registry");
            for (id, body) in [
                (
                    "pi",
                    r#"{"type":"session_info","name":"  Pi **title**\r\n\r\n\t```\nnext"}"#,
                ),
                (
                    "registry",
                    r#"{"type":"message","message":{"role":"assistant"}}"#,
                ),
                (
                    "message",
                    r#"{"type":"message","message":{"role":"user","content":"First **message**\nnext"}}"#,
                ),
            ] {
                let mut file = File::create(directory.path().join(format!("{id}.jsonl")))
                    .expect("session file");
                writeln!(file, r#"{{"type":"session","id":"{id}"}}"#).expect("header");
                writeln!(file, "{body}").expect("body");
            }

            let sessions = list_sessions_in(directory.path()).expect("sessions");
            let titles = sessions
                .iter()
                .map(|session| {
                    (
                        session.id.as_str(),
                        (session.title.as_str(), session.title_markdown.as_deref()),
                    )
                })
                .collect::<HashMap<_, _>>();

            assert_eq!(
                titles["pi"],
                (
                    "  Pi **title**     ``` next",
                    Some("  Pi **title**\r\n\r\n\t```\nnext")
                )
            );
            assert_eq!(
                titles["registry"],
                ("Tau *title* next", Some("Tau *title*\nnext"))
            );
            assert_eq!(
                titles["message"],
                ("First **message** next", Some("First **message**\nnext"))
            );
        }

        {
            let raw_title = format!(" \n\t{}é", "🦀".repeat(239));
            let remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/home/timur".into(),
                host: "build-box".into(),
                active_session_id: String::new(),
                sessions: vec![RemoteSessionRecord {
                    id: "remote-session".into(),
                    path: "/remote/session.jsonl".into(),
                    name: Some(raw_title.clone()),
                    archived: false,
                    last_active: 0,
                    sort_at: 0,
                }],
            };

            let session = list_remote_sessions(&remote).pop().expect("remote session");
            let markdown = session.title_markdown.expect("markdown title");
            assert_eq!(markdown.chars().count(), 240);
            assert_eq!(&markdown[..3], " \n\t");
            assert_eq!(markdown, raw_title.chars().take(240).collect::<String>());
            assert_eq!(session.title.chars().count(), 240);
            assert!(session.title.starts_with("   "));
        }

        {
            let registry: TauSessionRegistry = serde_json::from_str(
            r#"{"version":1,"activeSessionId":"session-1","sessions":[{"id":"session-1","name":null,"archived":false}]}"#,
        )
        .expect("legacy registry");
            assert_eq!(registry.sessions[0].name, None);
        }
    }
    #[test]
    fn local_sessions_register_into_the_registry_the_project_lists() {
        // Pi may open a session under another working directory; the row still
        // belongs to the project, so both sides must use one registry file.
        let registry = local_registry_path("/Users/timur/code/tau").expect("registry path");
        assert_eq!(
            registry,
            default_session_dir("/Users/timur/code/tau")
                .expect("session directory")
                .join(SESSION_REGISTRY_FILENAME)
        );
        assert_ne!(
            registry,
            default_session_dir("/Users/timur/code/other")
                .expect("other session directory")
                .join(SESSION_REGISTRY_FILENAME)
        );

        {
            let project_sessions = tempfile::tempdir().unwrap();
            let elsewhere = tempfile::tempdir().unwrap();
            let path = elsewhere.path().join("other_session.jsonl");
            fs::write(&path, b"{\"type\":\"session\",\"id\":\"other\"}\n").unwrap();
            let row = register_local_session_in(
                project_sessions.path(),
                "other",
                path.to_str().unwrap(),
                String::new(),
                false,
                None,
                0,
            )
            .unwrap()
            .unwrap();
            assert_eq!(row.path, path.to_string_lossy());
            let listed = list_sessions_in(project_sessions.path()).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].path, row.path);
        }

        {
            let mut remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/home/timur".into(),
                host: "build-box".into(),
                active_session_id: String::new(),
                sessions: Vec::new(),
            };
            assert!(select_remote_session(&mut remote, "missing").is_err());
            let mut local = TauSessionRegistry::default();
            assert!(select_local_session(&mut local, "missing").is_err());
        }

        {
            assert_eq!(ProjectRegistry::default().version, 1);
            assert_eq!(TauSessionRegistry::default().version, 1);
        }

        {
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
    }
    #[test]
    fn registration_checks_only_the_candidate_file_and_its_identity() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let unrelated = directory.path().join("unrelated.jsonl");
        fs::write(&unrelated, b"not a transcript").expect("unrelated file");
        let path = directory.path().join("2026_session-1.jsonl");
        assert!(
            discover_local_session(directory.path(), path.to_str().unwrap(), "session-1")
                .expect("missing file")
                .is_none()
        );
        fs::write(&path, b"{\"type\":\"session\",\"id\":\"wrong\"}\n").expect("wrong identity");
        assert!(
            discover_local_session(directory.path(), path.to_str().unwrap(), "session-1")
                .expect("wrong identity")
                .is_none()
        );
        fs::write(&path, b"{\"type\":\"session\",\"id\":\"session-1\"}\n").expect("session file");
        assert_eq!(
            discover_local_session(directory.path(), "", "session-1").expect("filename fallback"),
            Some(path.clone())
        );
        let outside = tempfile::tempdir().expect("other directory");
        let outside_file = outside.path().join("session-1.jsonl");
        fs::write(
            &outside_file,
            b"{\"type\":\"session\",\"id\":\"session-1\"}\n",
        )
        .unwrap();
        assert_eq!(
            discover_local_session(
                directory.path(),
                outside_file.to_str().unwrap(),
                "session-1"
            )
            .expect("outside file is registered by identity"),
            Some(outside_file)
        );

        {
            let session_dir = tempfile::tempdir().expect("session directory");
            let session_dir = session_dir.path();
            let path = session_dir.join("2026_verified.jsonl");
            let path_string = path.to_str().unwrap().to_string();
            assert!(register_local_session_in(
                session_dir,
                "verified",
                &path_string,
                String::new(),
                false,
                None,
                0,
            )
            .expect("unmaterialized registration")
            .is_none());
            let registry_path = session_dir.join(SESSION_REGISTRY_FILENAME);
            assert!(!registry_path.exists());
            fs::write(&path, b"{\"type\":\"session\",\"id\":\"verified\"}\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"Hello\",\"timestamp\":1000}}\n").unwrap();
            let row = register_local_session_in(
                session_dir,
                "verified",
                &path_string,
                "Tau title".into(),
                false,
                None,
                0,
            )
            .expect("verified registration")
            .expect("discoverable session");
            assert_eq!(row.id, "verified");
            assert_eq!(row.title, "Tau title");
            assert_eq!(row.path, path_string);
            assert_eq!(list_sessions_in(session_dir).unwrap().len(), 1);
            let mut registry: TauSessionRegistry = read_json_or_default(&registry_path).unwrap();
            set_local_session_archived(&mut registry, "verified", true).unwrap();
            write_json_atomic(&registry_path, &registry).unwrap();
            let row = register_local_session_in(
                session_dir,
                "verified",
                &path_string,
                String::new(),
                true,
                None,
                0,
            )
            .expect("adoption")
            .expect("adopted row");
            assert!(!row.archived);
            let metadata = fs::metadata(&registry_path).unwrap();
            register_local_session_in(
                session_dir,
                "verified",
                &row.path,
                String::new(),
                false,
                None,
                0,
            )
            .unwrap();
            assert_eq!(
                fs::metadata(&registry_path).unwrap().modified().unwrap(),
                metadata.modified().unwrap()
            );
        }
    }
}
