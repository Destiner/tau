use crate::models::{
    ProjectRecord, ProjectRegistry, RemoteProjectRecord, RemoteSessionRecord, TauSessionRecord,
    TauSessionRegistry,
};
use crate::profile::{StorageProfile, SESSION_REGISTRY_FILENAME};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::io::{BufWriter, Write};
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
    if std::env::var_os("TAU_DEV_FIXTURE").as_deref() == Some(std::ffi::OsStr::new("performance")) {
        return seed_performance(profile);
    }
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
                path: None,
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

// Explicit opt-in only. All files remain inside StorageProfile's disposable root.
// The padding is in a non-context Pi entry, so transcript parsing still walks
// the same number of bytes without sending synthetic content to a model.
const PERFORMANCE_FILES: usize = 589;
const PERFORMANCE_BYTES: usize = 613 * 1024 * 1024 / 2;

fn seed_performance(profile: &StorageProfile) -> Result<(), String> {
    let mut registry = ProjectRegistry::default();
    let mut written = 0usize;
    let long_file_size = (PERFORMANCE_BYTES - 120 * 4096) / (PERFORMANCE_FILES - 120);
    for project_index in 0..7 {
        let directory = profile
            .data_dir()
            .join("projects")
            .join(format!("perf-local-{project_index}"));
        fs::create_dir_all(&directory).map_err(seed_error)?;
        let path = directory.to_string_lossy().into_owned();
        let sessions_dir = profile.session_dir(&path, Path::new(""));
        fs::create_dir_all(&sessions_dir).map_err(seed_error)?;
        let mut sessions = TauSessionRegistry::default();
        for index in (project_index..PERFORMANCE_FILES).step_by(7) {
            let id = format!("00000000-0000-4000-8000-{index:012x}");
            let timestamp = 1_781_519_400_000_u64 - index as u64 * 60_000;
            let date = "2026-06-15T10:30:00.000Z";
            let header = json!({"type":"session","version":3,"id":id,"timestamp":date,"cwd":path});
            let user = json!({"type":"message","id":"00000001","parentId":null,"timestamp":date,
                "message":{"role":"user","content":[{"type":"text","text":format!("Invented task {index}")}],"timestamp":timestamp}});
            let assistant = json!({"type":"message","id":"00000002","parentId":"00000001","timestamp":date,
                "message":{"role":"assistant","content":[{"type":"text","text":"Synthetic response"}],
                    "api":"anthropic-messages","provider":"fixture","model":"dev-seed",
                    "usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,
                        "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},
                    "stopReason":"stop","timestamp":timestamp+1}});
            let mut transcript = format!("{header}\n{user}\n{assistant}\n");
            if index % 5 == 0 {
                // A sibling branch is abandoned; discovery must still parse it.
                transcript.push_str(&format!(
                    "{}\n",
                    json!({"type":"message","id":"00000003",
                    "parentId":"00000001","timestamp":date,"message":{"role":"user",
                    "content":[{"type":"text","text":"Alternate branch"}],"timestamp":timestamp+2}})
                ));
            }
            if index % 11 == 0 {
                transcript.push_str(&format!("{}\n", json!({"type":"compaction","id":"00000004",
                    "parentId":if index % 5 == 0 {"00000003"} else {"00000002"},
                    "timestamp":date,"summary":"Synthetic retained context","firstKeptEntryId":"00000001","tokensBefore":100})));
            }
            let target = if index < 120 {
                4096
            } else if index == PERFORMANCE_FILES - 1 {
                PERFORMANCE_BYTES - 120 * 4096 - (PERFORMANCE_FILES - 121) * long_file_size
            } else {
                long_file_size
            };
            let pad_prefix = "{\"type\":\"custom\",\"id\":\"00000005\",\"parentId\":\"00000002\",\"timestamp\":\"2026-06-15T10:30:00.000Z\",\"customType\":\"tau-perf-padding\",\"data\":\"";
            let pad_suffix = "\"}\n";
            let pad_length = target
                .checked_sub(transcript.len() + pad_prefix.len() + pad_suffix.len())
                .ok_or_else(|| {
                    "The performance fixture's target file size is too small.".to_string()
                })?;
            let session_path = sessions_dir.join(format!("{index:04}_{id}.jsonl"));
            let file = fs::File::create(&session_path).map_err(seed_error)?;
            let mut writer = BufWriter::new(file);
            writer
                .write_all(transcript.as_bytes())
                .map_err(seed_error)?;
            writer
                .write_all(pad_prefix.as_bytes())
                .map_err(seed_error)?;
            let chunk = [b'x'; 8192];
            for _ in 0..pad_length / chunk.len() {
                writer.write_all(&chunk).map_err(seed_error)?;
            }
            writer
                .write_all(&chunk[..pad_length % chunk.len()])
                .map_err(seed_error)?;
            writer
                .write_all(pad_suffix.as_bytes())
                .map_err(seed_error)?;
            writer.flush().map_err(seed_error)?;
            written += target;
            if index < 434 {
                if sessions.active_session_id.is_empty() && index >= 423 {
                    sessions.active_session_id = id.clone();
                }
                sessions.sessions.push(TauSessionRecord {
                    id,
                    path: Some(session_path.to_string_lossy().into_owned()),
                    name: Some(format!("Synthetic session {index}")),
                    archived: index < 423,
                });
            }
        }
        write_json(&sessions_dir.join(SESSION_REGISTRY_FILENAME), &sessions)?;
        if registry.active_project_path.is_empty() {
            registry.active_project_path = path.clone();
        }
        registry.projects.push(ProjectRecord {
            path,
            collapsed: false,
            remote: None,
        });
    }
    debug_assert_eq!(written, PERFORMANCE_BYTES);
    let connection_string = "ssh tau-perf.invalid";
    let working_directory = "/synthetic/performance";
    let remote_path =
        serde_json::to_string(&(connection_string, working_directory)).map_err(seed_error)?;
    registry.projects.push(ProjectRecord {
        path: format!("ssh:{remote_path}"),
        collapsed: false,
        remote: Some(RemoteProjectRecord {
            connection_string: connection_string.into(),
            working_directory: working_directory.into(),
            host: "tau-perf.invalid".into(),
            active_session_id: "remote-2388".into(),
            sessions: (0..2389)
                .map(|index| RemoteSessionRecord {
                    id: format!("remote-{index:04}"),
                    path: format!("/synthetic/performance/sessions/{index:04}.jsonl"),
                    name: Some(format!("Synthetic remote session {index}")),
                    archived: index != 2388,
                    last_active: 1_781_519_400_000 - index * 60_000,
                    sort_at: 1_781_519_400_000 - index * 60_000,
                })
                .collect(),
        }),
    });
    write_json(&profile.data_dir().join("projects.json"), &registry)?;
    eprintln!("Synthetic native fixture: 7 local + 1 simulated remote projects; 589 files, {written} bytes; 434 registered local (423 archived), 2389 remote (2388 archived). Remote metadata does not imply an SSH host.");
    Ok(())
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
    #[ignore = "generates 306.5 MiB; run explicitly for native fixture verification"]
    fn performance_fixture_shape_and_native_scan_comparison() {
        use std::time::Instant;
        let profile = StorageProfile::ephemeral().unwrap();
        seed_performance(&profile).unwrap();
        let registry: ProjectRegistry =
            serde_json::from_slice(&fs::read(profile.data_dir().join("projects.json")).unwrap())
                .unwrap();
        assert_eq!(registry.projects.len(), 8);
        let remote = registry.projects[7].remote.as_ref().unwrap();
        assert_eq!(remote.sessions.len(), 2389);
        assert_eq!(
            remote.sessions.iter().filter(|row| row.archived).count(),
            2388
        );
        let mut bytes = 0;
        let mut files = 0;
        let mut registered = 0;
        let mut archived = 0;
        let mut short = 0;
        for project in &registry.projects[..7] {
            let directory = profile.session_dir(&project.path, Path::new(""));
            let sessions: TauSessionRegistry = serde_json::from_slice(
                &fs::read(directory.join(SESSION_REGISTRY_FILENAME)).unwrap(),
            )
            .unwrap();
            registered += sessions.sessions.len();
            archived += sessions.sessions.iter().filter(|row| row.archived).count();
            for entry in fs::read_dir(directory).unwrap().flatten() {
                if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                {
                    let len = entry.metadata().unwrap().len() as usize;
                    bytes += len;
                    files += 1;
                    short += usize::from(len == 4096);
                }
            }
        }
        assert_eq!(
            (files, bytes, registered, archived, short),
            (589, PERFORMANCE_BYTES, 434, 423, 120)
        );
        // This compares the legacy discovery phase with the narrow registry-only
        // phase on identical native files. It is NOT a native UI paint benchmark.
        let mut scan = Vec::new();
        let mut narrow = Vec::new();
        for iteration in 0..30 {
            let start = Instant::now();
            for project in &registry.projects[..7] {
                let directory = profile.session_dir(&project.path, Path::new(""));
                let _ = list_sessions_in(&directory).unwrap();
            }
            scan.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = Instant::now();
            let mut changed = registry.clone();
            changed.projects[0].collapsed = iteration % 2 == 0;
            write_json(&profile.data_dir().join("projects.json"), &changed).unwrap();
            narrow.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        for (name, samples) in [
            ("baseline-equivalent discovery only", scan),
            ("current-style registry write only", narrow),
        ] {
            let mut sorted = samples.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "{name}: n={}, median_ms={:.3}, p95_ms={:.3}, samples_ms={:?}",
                sorted.len(),
                sorted[sorted.len() / 2],
                sorted[(sorted.len() * 95).div_ceil(100) - 1],
                samples
            );
        }
    }

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
