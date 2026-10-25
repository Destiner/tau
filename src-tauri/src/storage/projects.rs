use super::{
    registry::{mutate_projects, mutate_projects_at, project_registry_path},
    sessions::{list_project_sessions, list_remote_sessions},
};
use crate::{
    models::{
        ProjectRecord, ProjectRegistry, ProjectSummary, RemoteProjectRecord, WorkspaceSnapshot,
    },
    pi::resolve_pi_binary,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) fn reorder_project_records(
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

pub(super) fn snapshot(registry: &ProjectRegistry) -> Result<WorkspaceSnapshot, String> {
    let projects = registry
        .projects
        .iter()
        .map(|project| project_summary(project, &registry.active_project_path))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(WorkspaceSnapshot {
        active_project_path: registry.active_project_path.clone(),
        pi_path: resolve_pi_binary().map(|path| path.to_string_lossy().into_owned()),
        projects,
    })
}

fn project_summary(
    project: &ProjectRecord,
    active_project_path: &str,
) -> Result<ProjectSummary, String> {
    let (working_directory, connection_string, sessions) =
        if let Some(remote) = project.remote.as_ref() {
            (
                remote.working_directory.clone(),
                Some(remote.connection_string.clone()),
                list_remote_sessions(remote),
            )
        } else {
            (
                project.path.clone(),
                None,
                list_project_sessions(&project.path)?,
            )
        };
    Ok(ProjectSummary {
        name: effective_project_name(project),
        path: project.path.clone(),
        working_directory,
        connection_string,
        collapsed: project.collapsed,
        selected: project.path == active_project_path,
        sessions,
    })
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

fn register_remote_project(
    registry: &mut ProjectRegistry,
    path: &str,
    connection_string: String,
    working_directory: String,
    host: String,
) -> String {
    if let Some(project) = registry.projects.iter_mut().find(|project| {
        project.remote.as_ref().is_some_and(|remote| {
            remote.connection_string == connection_string
                && remote.working_directory == working_directory
        })
    }) {
        if let Some(remote) = project.remote.as_mut() {
            remote.host = host;
        }
        return project.path.clone();
    }

    registry.projects.push(ProjectRecord {
        name: Some(remote_default_name(&working_directory, &host)),
        path: path.to_string(),
        collapsed: false,
        remote: Some(RemoteProjectRecord {
            connection_string,
            working_directory,
            host,
            active_session_id: String::new(),
            sessions: Vec::new(),
        }),
    });
    path.to_string()
}

fn effective_project_name(project: &ProjectRecord) -> String {
    project.name.clone().unwrap_or_else(|| {
        project
            .remote
            .as_ref()
            .map_or_else(|| project_name(&project.path), remote_project_name)
    })
}

fn remote_project_name(remote: &RemoteProjectRecord) -> String {
    remote_default_name(&remote.working_directory, &remote.host)
}

fn remote_default_name(working_directory: &str, host: &str) -> String {
    Path::new(working_directory)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(host)
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

fn register_local_project(registry: &mut ProjectRegistry, path: &str) {
    if !registry.projects.iter().any(|project| project.path == path) {
        registry.projects.push(ProjectRecord {
            name: Some(project_name(path)),
            path: path.to_string(),
            collapsed: false,
            remote: None,
        });
    }
}

pub(super) fn import_project_inner(path: String) -> Result<ProjectSummary, String> {
    let path = normalized_project_path(&path)?;
    let registry = mutate_projects(|registry| {
        register_local_project(registry, &path);
        Ok(())
    })?;
    project_summary(
        registry
            .projects
            .iter()
            .find(|project| project.path == path)
            .unwrap(),
        &registry.active_project_path,
    )
}

pub(super) fn import_remote_project_inner(
    connection_string: String,
    working_directory: String,
    host: String,
) -> Result<ProjectSummary, String> {
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

    let mut imported_path = new_path.clone();
    let registry = mutate_projects(|registry| {
        imported_path = register_remote_project(
            registry,
            &new_path,
            connection_string,
            working_directory,
            host,
        );
        Ok(())
    })?;
    let project = registry
        .projects
        .iter()
        .find(|project| project.path == imported_path)
        .ok_or_else(|| "The remote project is no longer available.".to_string())?;
    project_summary(project, &registry.active_project_path)
}

fn normalized_project_name(name: &str) -> Result<String, String> {
    // Match JavaScript String.trim rather than Rust's broader whitespace set.
    let name = name.trim_matches(|character: char| {
        matches!(character, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
    });
    if name.is_empty() {
        return Err("The project name must not be blank.".into());
    }
    if name.encode_utf16().count() > 240 {
        return Err("The project name must not exceed 240 UTF-16 units.".into());
    }
    Ok(name.to_string())
}

pub(super) fn rename_project_inner(path: String, name: String) -> Result<String, String> {
    rename_project_at(&project_registry_path()?, &path, &name)
}

fn rename_project_at(registry_path: &Path, path: &str, name: &str) -> Result<String, String> {
    let name = normalized_project_name(name)?;
    mutate_projects_at(registry_path, |registry| {
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.path == path)
            .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
        if effective_project_name(project) != name {
            project.name = Some(name.clone());
        }
        Ok(())
    })?;
    Ok(name)
}

pub(super) fn remove_project_inner(path: String) -> Result<(), String> {
    mutate_projects(|registry| {
        registry.projects.retain(|project| project.path != path);
        if registry.active_project_path == path {
            registry.active_project_path.clear();
        }
        Ok(())
    })
    .map(|_| ())
}

pub(super) fn set_active_project_inner(path: String) -> Result<(), String> {
    mutate_projects(|registry| {
        if !registry.projects.iter().any(|project| project.path == path) {
            return Err("The project is not imported in Tau.".into());
        }
        registry.active_project_path = path;
        Ok(())
    })
    .map(|_| ())
}

pub(super) fn set_project_collapsed_inner(path: String, collapsed: bool) -> Result<(), String> {
    mutate_projects(|registry| {
        let project = registry
            .projects
            .iter_mut()
            .find(|project| project.path == path)
            .ok_or_else(|| "The project is not imported in Tau.".to_string())?;
        project.collapsed = collapsed;
        Ok(())
    })
    .map(|_| ())
}

pub(super) fn reorder_projects_inner(project_paths: Vec<String>) -> Result<(), String> {
    mutate_projects(|registry| reorder_project_records(&mut registry.projects, &project_paths))
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::super::registry::{configured_pi_agent_dir, default_session_dir};
    use super::*;
    use crate::models::RemoteSessionRecord;
    use std::ffi::{OsStr, OsString};

    fn legacy_registry() -> ProjectRegistry {
        serde_json::from_str(r#"{
            "version":1,"activeProjectPath":"/unavailable/project",
            "projects":[
                {"path":"/unavailable/project","collapsed":true},
                {"path":"ssh:legacy-identity","collapsed":false,"remote":{
                    "connectionString":"ssh offline.invalid","workingDirectory":"/remote/work",
                    "host":"offline.invalid","activeSessionId":"active",
                    "sessions":[
                        {"id":"active","path":"/remote/active.jsonl","name":"Active","archived":false,"lastActive":42,"sortAt":43},
                        {"id":"archived","path":"/remote/archived.jsonl","archived":true,"lastActive":12,"sortAt":13}
                    ]
                }}
            ]
        }"#).unwrap()
    }

    #[test]
    fn legacy_project_names_resolve_without_migration() {
        let registry = legacy_registry();
        assert_eq!(effective_project_name(&registry.projects[0]), "project");
        assert_eq!(effective_project_name(&registry.projects[1]), "work");
        for project in &registry.projects {
            assert!(project.name.is_none());
            assert!(serde_json::to_value(project).unwrap().get("name").is_none());
        }
        let mut local = registry.projects[0].clone();
        local.path = "/".into();
        assert_eq!(effective_project_name(&local), "Project");
        let mut remote = registry.projects[1].clone();
        remote.remote.as_mut().unwrap().working_directory = "/".into();
        assert_eq!(effective_project_name(&remote), "offline.invalid");
        let summary = project_summary(&registry.projects[1], "ssh:legacy-identity").unwrap();
        assert_eq!(summary.name, "work");
        assert!(summary.selected);
        assert_eq!(summary.sessions.len(), 2);
    }

    #[test]
    fn rename_persists_only_names_and_preserves_session_files() {
        use super::super::registry::{read_json_or_default, write_json_atomic};
        use std::fs;
        let profile = crate::profile::StorageProfile::ephemeral().unwrap();
        let path = profile.data_dir().join("projects.json");
        let registry = legacy_registry();
        write_json_atomic(&path, &registry).unwrap();
        let session_dir = profile.session_dir(&registry.projects[0].path, Path::new(""));
        fs::create_dir_all(&session_dir).unwrap();
        let session_registry = session_dir.join(crate::profile::SESSION_REGISTRY_FILENAME);
        let transcript = session_dir.join("pi.jsonl");
        let session_bytes = b"{\"activeSessionId\":\"active\",\"sessions\":[]}\n";
        let transcript_bytes = b"{\"type\":\"session\",\"id\":\"active\"}\n";
        fs::write(&session_registry, session_bytes).unwrap();
        fs::write(&transcript, transcript_bytes).unwrap();
        let session_identity = session_dir.clone();
        for project in &registry.projects {
            assert_eq!(
                rename_project_at(&path, &project.path, "  Shared 🦀 / name  ").unwrap(),
                "Shared 🦀 / name"
            );
        }
        let reloaded: ProjectRegistry = read_json_or_default(&path).unwrap();
        let mut expected = serde_json::to_value(&registry).unwrap();
        for project in expected["projects"].as_array_mut().unwrap() {
            project["name"] = serde_json::json!("Shared 🦀 / name");
        }
        assert_eq!(serde_json::to_value(&reloaded).unwrap(), expected);
        assert_eq!(fs::read(session_registry).unwrap(), session_bytes);
        assert_eq!(fs::read(transcript).unwrap(), transcript_bytes);
        assert_eq!(
            profile.session_dir(&reloaded.projects[0].path, Path::new("")),
            session_identity
        );
        for project in &reloaded.projects {
            let summary = project_summary(project, &reloaded.active_project_path).unwrap();
            assert_eq!(summary.name, "Shared 🦀 / name");
            assert_eq!(summary.path, project.path);
            assert_eq!(
                summary.selected,
                project.path == reloaded.active_project_path
            );
        }
        let reimported = mutate_projects_at(&path, |registry| {
            register_local_project(registry, "/unavailable/project");
            let identity = register_remote_project(
                registry,
                "ssh:newly-encoded-identity",
                "ssh offline.invalid".into(),
                "/remote/work".into(),
                "refreshed-host".into(),
            );
            assert_eq!(identity, "ssh:legacy-identity");
            Ok(())
        })
        .unwrap();
        expected["projects"][1]["remote"]["host"] = serde_json::json!("refreshed-host");
        assert_eq!(serde_json::to_value(&reimported).unwrap(), expected);
        let fresh: ProjectRegistry = read_json_or_default(&path).unwrap();
        assert_eq!(serde_json::to_value(&fresh).unwrap(), expected);
    }

    #[test]
    fn rename_validation_and_exact_identity_never_write_on_error_or_noop() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        // Preserve legacy formatting: even a no-op must not rewrite it.
        let bytes = b"{ \"projects\": [{\"path\":\"/missing/project\"}] }\n";
        fs::write(&path, bytes).unwrap();
        fs::create_dir(path.with_extension("tmp")).unwrap();
        assert_eq!(
            rename_project_at(&path, "/missing/project", " project ").unwrap(),
            "project"
        );
        for name in ["", " \t\n\u{2003}", &"a".repeat(241), &"🦀".repeat(121)] {
            assert!(rename_project_at(&path, "/missing/project", name).is_err());
        }
        for identity in ["/missing/project/", "/missing/./project", "unknown"] {
            assert!(rename_project_at(&path, identity, "Valid").is_err());
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
        // A directory at the atomic writer's temporary path deterministically fails.
        assert!(rename_project_at(&path, "/missing/project", "Changed").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir(path.with_extension("tmp")).unwrap();
        let limit = "🦀".repeat(120);
        assert_eq!(
            rename_project_at(&path, "/missing/project", &limit).unwrap(),
            limit
        );
        let saved = fs::read(&path).unwrap();
        assert_eq!(
            rename_project_at(&path, "/missing/project", &format!(" {limit} ")).unwrap(),
            limit
        );
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(
            normalized_project_name(&format!(" {} ", "a".repeat(240)))
                .unwrap()
                .len(),
            240
        );
        assert_eq!(
            normalized_project_name("  東京 / : .  ").unwrap(),
            "東京 / : ."
        );
        assert_eq!(
            normalized_project_name("\u{feff}Name\u{feff}").unwrap(),
            "Name"
        );
        assert!(normalized_project_name("\u{feff}").is_err());
        assert_eq!(normalized_project_name("\u{0085}").unwrap(), "\u{0085}");
    }

    #[test]
    fn name_validation_matches_ecmascript_trim_and_utf16_boundaries() {
        let whitespace = "\u{0009}\u{000a}\u{000b}\u{000c}\u{000d}\u{0020}\u{00a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";
        assert!(normalized_project_name(whitespace).is_err());
        for character in whitespace.chars() {
            assert_eq!(
                normalized_project_name(&format!("{character}Name{character}")).unwrap(),
                "Name"
            );
            let internal = format!("A{character}B");
            assert_eq!(normalized_project_name(&internal).unwrap(), internal);
        }
        for character in ['\u{0085}', '\u{180e}', '\u{200b}', '\u{001c}'] {
            let name = format!("{character}Name{character}");
            assert_eq!(normalized_project_name(&name).unwrap(), name);
        }
        let boundary = format!("{}🦀", "a".repeat(238));
        assert_eq!(
            normalized_project_name(&format!("\u{feff}{boundary}\u{3000}")).unwrap(),
            boundary
        );
        assert!(normalized_project_name(&format!("{boundary}a")).is_err());
    }

    #[test]
    fn legacy_registry_load_and_remote_noop_preserve_bytes() {
        use super::super::registry::read_json_or_default;
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        let bytes = serde_json::to_vec(&legacy_registry()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let fresh: ProjectRegistry = read_json_or_default(&path).unwrap();
        assert_eq!(effective_project_name(&fresh.projects[0]), "project");
        assert_eq!(effective_project_name(&fresh.projects[1]), "work");
        assert_eq!(
            rename_project_at(&path, "ssh:legacy-identity", "\u{feff}work ").unwrap(),
            "work"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let fresh: ProjectRegistry = read_json_or_default(&path).unwrap();
        assert!(fresh.projects.iter().all(|project| project.name.is_none()));
    }

    #[test]
    fn concurrent_renames_preserve_other_registry_mutations() {
        use super::super::registry::{read_json_or_default, write_json_atomic};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        write_json_atomic(&path, &legacy_registry()).unwrap();
        let barrier = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                rename_project_at(&path, "/unavailable/project", "Local").unwrap();
            });
            scope.spawn(|| {
                barrier.wait();
                rename_project_at(&path, "ssh:legacy-identity", "Remote").unwrap();
            });
            barrier.wait();
            mutate_projects_at(&path, |registry| {
                registry.active_project_path = "ssh:legacy-identity".into();
                registry.projects[1].collapsed = true;
                Ok(())
            })
            .unwrap();
        });
        let registry: ProjectRegistry = read_json_or_default(&path).unwrap();
        assert_eq!(registry.projects[0].name.as_deref(), Some("Local"));
        assert_eq!(registry.projects[1].name.as_deref(), Some("Remote"));
        assert_eq!(registry.active_project_path, "ssh:legacy-identity");
        assert!(registry.projects[1].collapsed);
    }

    #[test]
    fn rename_missing_registry_does_not_import_and_corruption_is_preserved() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("projects.json");
        assert!(rename_project_at(&path, "/absent", "Name").is_err());
        assert!(!path.exists());
        fs::write(&path, b"invalid json").unwrap();
        assert!(rename_project_at(&path, "/absent", "Name").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid json");
    }

    #[test]
    fn registering_a_new_local_project_does_not_select_it() {
        let mut registry = ProjectRegistry::default();

        register_local_project(&mut registry, "/projects/new");

        assert!(registry.active_project_path.is_empty());
        assert_eq!(registry.projects.len(), 1);
        assert_eq!(registry.projects[0].path, "/projects/new");
        assert!(registry.projects[0].remote.is_none());
        assert_eq!(registry.projects[0].name.as_deref(), Some("new"));
    }

    #[test]
    fn registering_an_existing_local_project_preserves_selection() {
        let mut registry = ProjectRegistry {
            active_project_path: "/projects/active".into(),
            projects: vec![ProjectRecord {
                name: Some("Custom local".into()),
                path: "/projects/existing".into(),
                collapsed: true,
                remote: None,
            }],
            ..ProjectRegistry::default()
        };

        register_local_project(&mut registry, "/projects/existing");

        assert_eq!(registry.active_project_path, "/projects/active");
        assert_eq!(registry.projects.len(), 1);
        assert!(registry.projects[0].collapsed);
        assert_eq!(registry.projects[0].name.as_deref(), Some("Custom local"));
    }

    #[test]
    fn registering_a_new_remote_project_does_not_select_it() {
        let mut registry = ProjectRegistry::default();

        register_remote_project(
            &mut registry,
            "ssh:[\"ssh build-box\",\"/workspace\"]",
            "ssh build-box".into(),
            "/workspace".into(),
            "build-box".into(),
        );

        assert!(registry.active_project_path.is_empty());
        assert_eq!(registry.projects.len(), 1);
        assert_eq!(
            registry.projects[0].path,
            "ssh:[\"ssh build-box\",\"/workspace\"]"
        );
        assert!(registry.projects[0].remote.is_some());
        assert_eq!(registry.projects[0].name.as_deref(), Some("workspace"));
    }

    #[test]
    fn registering_an_existing_remote_project_preserves_sessions_and_selection() {
        let mut registry = ProjectRegistry {
            active_project_path: "/projects/active".into(),
            projects: vec![ProjectRecord {
                name: Some("Custom remote".into()),
                path: "ssh:[\"ssh build-box\",\"/workspace\"]".into(),
                collapsed: true,
                remote: Some(RemoteProjectRecord {
                    connection_string: "ssh build-box".into(),
                    working_directory: "/workspace".into(),
                    host: "old-host".into(),
                    active_session_id: "session-1".into(),
                    sessions: vec![RemoteSessionRecord {
                        id: "session-1".into(),
                        path: "/workspace/.pi/sessions/session-1.jsonl".into(),
                        name: Some("Preserved session".into()),
                        archived: false,
                        last_active: 42,
                        sort_at: 42,
                    }],
                }),
            }],
            ..ProjectRegistry::default()
        };

        register_remote_project(
            &mut registry,
            "ssh:[\"ssh build-box\",\"/workspace\"]",
            "ssh build-box".into(),
            "/workspace".into(),
            "new-host".into(),
        );

        let remote = registry.projects[0]
            .remote
            .as_ref()
            .expect("remote project");
        assert_eq!(registry.active_project_path, "/projects/active");
        assert_eq!(registry.projects.len(), 1);
        assert!(registry.projects[0].collapsed);
        assert_eq!(registry.projects[0].name.as_deref(), Some("Custom remote"));
        assert_eq!(remote.host, "new-host");
        assert_eq!(remote.active_session_id, "session-1");
        assert_eq!(remote.sessions.len(), 1);
        assert_eq!(remote.sessions[0].id, "session-1");
    }
    #[test]
    fn default_session_path_matches_pi_encoding() {
        let path = default_session_dir("/Users/timur/code/tau").expect("session path");
        assert!(path.ends_with("sessions/--Users-timur-code-tau--"));

        {
            assert_eq!(
                configured_pi_agent_dir(
                    Some(OsString::from("/inherited/pi")),
                    Some(OsStr::new("/login/pi")),
                ),
                Some(PathBuf::from("/inherited/pi")),
            );
            assert_eq!(
                configured_pi_agent_dir(None, Some(OsStr::new("/login/pi"))),
                Some(PathBuf::from("/login/pi")),
            );
        }
    }
    #[test]
    fn legacy_local_projects_remain_local() {
        let project: ProjectRecord =
            serde_json::from_str(r#"{"path":"/tmp/tau","collapsed":false}"#)
                .expect("legacy project");
        assert!(project.remote.is_none());

        {
            let first = remote_project_path("ssh build-box", "/home/timur/one")
                .expect("first remote project");
            let second = remote_project_path("ssh build-box", "/home/timur/two")
                .expect("second remote project");
            assert_ne!(first, second);
        }

        {
            let remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/users/agent/rhinestone".into(),
                host: "build-box".into(),
                active_session_id: String::new(),
                sessions: Vec::new(),
            };
            assert_eq!(remote_project_name(&remote), "rhinestone");
        }
    }
}
