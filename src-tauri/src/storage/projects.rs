use super::{
    registry::mutate_projects,
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
) {
    if let Some(project) = registry.projects.iter_mut().find(|project| {
        project.remote.as_ref().is_some_and(|remote| {
            remote.connection_string == connection_string
                && remote.working_directory == working_directory
        })
    }) {
        if let Some(remote) = project.remote.as_mut() {
            remote.host = host;
        }
        return;
    }

    registry.projects.push(ProjectRecord {
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

fn register_local_project(registry: &mut ProjectRegistry, path: &str) {
    if !registry.projects.iter().any(|project| project.path == path) {
        registry.projects.push(ProjectRecord {
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

    let registry = mutate_projects(|registry| {
        register_remote_project(
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
        .find(|project| project.path == new_path)
        .ok_or_else(|| "The remote project is no longer available.".to_string())?;
    project_summary(project, &registry.active_project_path)
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

    #[test]
    fn registering_a_new_local_project_does_not_select_it() {
        let mut registry = ProjectRegistry::default();

        register_local_project(&mut registry, "/projects/new");

        assert!(registry.active_project_path.is_empty());
        assert_eq!(registry.projects.len(), 1);
        assert_eq!(registry.projects[0].path, "/projects/new");
        assert!(registry.projects[0].remote.is_none());
    }

    #[test]
    fn registering_an_existing_local_project_preserves_selection() {
        let mut registry = ProjectRegistry {
            active_project_path: "/projects/active".into(),
            projects: vec![ProjectRecord {
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
    }

    #[test]
    fn registering_an_existing_remote_project_preserves_sessions_and_selection() {
        let mut registry = ProjectRegistry {
            active_project_path: "/projects/active".into(),
            projects: vec![ProjectRecord {
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
