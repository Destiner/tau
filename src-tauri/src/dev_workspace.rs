//! The native playground uses real Pi files, not a second session runtime.

use crate::models::{ProjectRecord, ProjectRegistry, TauSessionRecord, TauSessionRegistry};
use crate::profile::{StorageProfile, SESSION_REGISTRY_FILENAME};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct SeedProject {
    name: String,
    sessions: Vec<SeedSession>,
}

#[derive(Deserialize)]
struct SeedSession {
    id: String,
    title: String,
    user: String,
    assistant: String,
}

pub fn seed(profile: &StorageProfile) -> Result<(), String> {
    let projects: Vec<SeedProject> =
        serde_json::from_str(include_str!("../../src/dev/workspace-seed.json"))
            .map_err(|error| format!("Could not read the development workspace seed: {error}"))?;
    let mut registry = ProjectRegistry::default();
    let mut index = 0;
    for project in projects {
        let directory = profile.data_dir().join("projects").join(&project.name);
        fs::create_dir_all(&directory).map_err(seed_error)?;
        fs::write(
            directory.join("README.md"),
            format!(
                "# {}\n\nDisposable Tau development fixture. Changes are discarded when this app exits.\n\n## Tasks\n{}\n",
                project.name,
                project.sessions.iter().map(|session| format!("- {}", session.title)).collect::<Vec<_>>().join("\n"),
            ),
        )
        .map_err(seed_error)?;
        let project_path = directory.to_string_lossy().into_owned();
        let sessions_dir = profile.session_dir(&project_path, Path::new(""));
        fs::create_dir_all(&sessions_dir).map_err(seed_error)?;
        let mut sessions = TauSessionRegistry::default();
        for session in project.sessions {
            let id = uuid::Uuid::new_v4().to_string();
            let timestamp = 1_781_519_400_000_u64 - index * 60_000;
            let date = "2026-06-15T10:30:00.000Z";
            // Omit model_change: the seed must not select a provider/model the
            // developer cannot use. Pi chooses from their normal configuration.
            let entries = [
                json!({"type": "session", "version": 3, "id": id, "timestamp": date, "cwd": project_path}),
                json!({"type": "session_info", "id": "00000001", "parentId": null, "timestamp": date, "name": session.title}),
                json!({"type": "message", "id": "00000002", "parentId": "00000001", "timestamp": date,
                    "message": {"role": "user", "content": [{"type": "text", "text": session.user}], "timestamp": timestamp}}),
                json!({"type": "message", "id": "00000003", "parentId": "00000002", "timestamp": date,
                    "message": {"role": "assistant", "content": [{"type": "text", "text": session.assistant}],
                        "api": "anthropic-messages", "provider": "fixture", "model": "dev-seed",
                        "usage": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0,
                            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "total": 0}},
                        "stopReason": "stop", "timestamp": timestamp + 1_000}}),
            ];
            let mut transcript = String::new();
            for entry in entries {
                transcript.push_str(&entry.to_string());
                transcript.push('\n');
            }
            fs::write(
                sessions_dir.join(format!("{}_{id}.jsonl", session.id)),
                transcript,
            )
            .map_err(seed_error)?;
            if sessions.active_session_id.is_empty() {
                sessions.active_session_id = id.clone();
            }
            sessions.sessions.push(TauSessionRecord {
                id,
                name: Some(session.title),
                archived: false,
            });
            index += 1;
        }
        write_json(&sessions_dir.join(SESSION_REGISTRY_FILENAME), &sessions)?;
        if registry.active_project_path.is_empty() {
            registry.active_project_path = project_path.clone();
        }
        registry.projects.push(ProjectRecord {
            path: project_path,
            collapsed: false,
            remote: None,
        });
    }
    write_json(&profile.data_dir().join("projects.json"), &registry)
}

fn seed_error(error: impl std::fmt::Display) -> String {
    format!("Could not seed the development workspace: {error}")
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    fs::write(path, serde_json::to_vec_pretty(value).map_err(seed_error)?).map_err(seed_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::list_sessions_in;

    #[test]
    fn seeded_profiles_load_through_real_storage_and_never_share_mutations() {
        let first = StorageProfile::ephemeral().unwrap();
        let second = StorageProfile::ephemeral().unwrap();
        seed(&first).unwrap();
        seed(&second).unwrap();
        for profile in [&first, &second] {
            let registry: ProjectRegistry = serde_json::from_slice(
                &fs::read(profile.data_dir().join("projects.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(registry.projects.len(), 2);
            assert_eq!(registry.active_project_path, registry.projects[0].path);
            for (project, expected_count) in registry.projects.iter().zip([3, 2]) {
                assert!(Path::new(&project.path).join("README.md").is_file());
                let directory = profile.session_dir(&project.path, Path::new("/real/pi"));
                let sessions = list_sessions_in(&directory).unwrap();
                assert_eq!(sessions.len(), expected_count);
                assert_eq!(
                    sessions.iter().filter(|session| session.selected).count(),
                    1
                );
                assert!(sessions
                    .iter()
                    .all(|session| !session.archived && !session.title.is_empty()));
                for session in &sessions {
                    let entries = fs::read_to_string(&session.path)
                        .unwrap()
                        .lines()
                        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                        .collect::<Vec<_>>();
                    assert_eq!(entries[0]["cwd"], project.path);
                    assert_eq!(entries[0]["id"], session.id);
                    assert_eq!(entries[0]["version"], 3);
                    assert_eq!(entries[2]["message"]["role"], "user");
                    assert_eq!(entries[3]["message"]["role"], "assistant");
                    assert_eq!(entries[3]["parentId"], entries[2]["id"]);
                    assert!(Path::new(&session.path).starts_with(&directory));
                }
            }
        }
        fs::write(first.data_dir().join("projects.json"), "{}").unwrap();
        let untouched = fs::read(second.data_dir().join("projects.json")).unwrap();
        let registry: ProjectRegistry = serde_json::from_slice(&untouched).unwrap();
        assert_eq!(registry.projects.len(), 2);
        drop(first);
        assert_eq!(
            fs::read(second.data_dir().join("projects.json")).unwrap(),
            untouched
        );
    }
}
