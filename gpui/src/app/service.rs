use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, io::Write, path::PathBuf, sync::Arc};
use tau_core::{
    models::{ProjectSummary, WorkspaceSnapshot},
    pi::{PiEvent, PiSink, PiState},
    storage,
};

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: String,
    pub drafts: Vec<Draft>,
    pub selected_draft: String,
}
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub key: String,
    pub project: String,
    pub source_id: String,
    pub source_path: String,
    pub text: String,
    pub recovered: Vec<String>,
}

#[derive(Clone)]
pub enum Command {
    Load {
        project: Option<String>,
    },
    Import(String),
    Start {
        runtime: String,
        attempt: u64,
        project: ProjectSummary,
        source: Option<String>,
    },
    CancelStart {
        runtime: String,
        attempt: u64,
    },
    Send {
        runtime: String,
        generation: u64,
        request: Value,
    },
    Register {
        runtime: String,
        generation: u64,
        project: String,
        id: String,
        path: String,
        name: String,
    },
    StoreName {
        runtime: String,
        generation: u64,
        revision: u64,
        project: String,
        id: String,
        path: String,
        name: String,
    },
    Archive {
        project: String,
        id: String,
        revision: u64,
        archived: bool,
    },
    Select {
        project: String,
        session: String,
    },
    Collapse {
        project: String,
        collapsed: bool,
    },
    Reorder(Vec<String>),
    Save {
        revision: u64,
        preferences: Option<Preferences>,
        shutdown: bool,
    },
    Shutdown(Sender<()>),
}
pub enum Event {
    Loaded(Result<(WorkspaceSnapshot, Preferences), ()>),
    Workspace(Result<WorkspaceSnapshot, ()>),
    Started {
        runtime: String,
        attempt: u64,
        result: Result<u64, ()>,
    },
    Written {
        runtime: String,
        generation: u64,
        request: Value,
        success: bool,
    },
    Pi(PiEvent),
    Saved {
        revision: u64,
        success: bool,
        stopped: Vec<(String, u64)>,
    },
    NameStored {
        runtime: String,
        generation: u64,
        revision: u64,
        id: String,
        path: String,
        name: String,
        success: bool,
    },
    Archived {
        project: String,
        id: String,
        revision: u64,
        archived: bool,
        success: bool,
    },
    StorageFailed,
    RegistrationFailed(Command),
}

pub fn launch(data: PathBuf) -> (Sender<Command>, Receiver<Event>) {
    let (commands, receiver) = async_channel::bounded(256);
    let (events, output) = async_channel::bounded(512);
    std::thread::spawn(move || worker(data, receiver, events));
    (commands, output)
}
fn worker(data: PathBuf, commands: Receiver<Command>, events: Sender<Event>) {
    let pi = PiState::default();
    let owner = uuid::Uuid::new_v4().to_string();
    let claimed = pi
        .read_revision()
        .ok()
        .and_then(|r| pi.claim(owner.clone(), r).ok())
        .is_some();
    let mut generations = HashMap::new();
    let mut registry_generations = HashMap::new();
    let mut start_attempts = HashMap::new();
    let mut archive_intents = std::collections::HashSet::new();
    let mut session_names = SessionNames::default();
    let sink_events = events.clone();
    let sink = PiSink::new(Arc::new(move |mut event: PiEvent| {
        // stderr is untrusted content, not a user-visible failure or diagnostic.
        if event.kind != "stderr" {
            event.message = None;
            let _ = sink_events.send_blocking(Event::Pi(event));
        }
    }));
    while let Ok(command) = commands.recv_blocking() {
        if events.is_closed() && matches!(&command, Command::Start { .. }) {
            continue;
        }
        let event = match command {
            Command::Load { project } => {
                let result = if claimed {
                    project
                        .map(storage::import_project)
                        .transpose()
                        .and_then(|_| storage::load_workspace())
                        .and_then(|workspace| {
                            read_preferences(&data).map(|prefs| (workspace, prefs))
                        })
                } else {
                    Err("ownership".into())
                };
                Some(Event::Loaded(result.map_err(|_| ())))
            }
            Command::Import(path) => Some(Event::Workspace(
                storage::import_project(path)
                    .and_then(|_| storage::load_workspace())
                    .map_err(|_| ()),
            )),
            Command::Start {
                runtime,
                attempt,
                project,
                source,
            } => {
                start_attempts.insert(runtime.clone(), attempt);
                generations.remove(&runtime);
                registry_generations.remove(&runtime);
                let result = pi.stop(&owner, &runtime).and_then(|_| {
                    if !claimed {
                        return Err("ownership".into());
                    }
                    match project.connection_string {
                        Some(connection) => pi.start_remote(
                            owner.clone(),
                            runtime.clone(),
                            connection,
                            project.working_directory,
                            source,
                            sink.clone(),
                        ),
                        None => pi.start_local(
                            owner.clone(),
                            runtime.clone(),
                            project.working_directory,
                            source,
                            sink.clone(),
                        ),
                    }
                });
                if let Ok(generation) = result {
                    generations.insert(runtime.clone(), generation);
                    registry_generations.insert(runtime.clone(), generation);
                }
                Some(Event::Started {
                    runtime,
                    attempt,
                    result: result.map_err(|_| ()),
                })
            }
            Command::CancelStart { runtime, attempt } => {
                if start_attempts.get(&runtime) == Some(&attempt)
                    && pi.stop(&owner, &runtime).is_ok()
                {
                    generations.remove(&runtime);
                }
                None
            }
            Command::Send {
                runtime,
                generation,
                request,
            } => {
                let success = generations.get(&runtime) == Some(&generation)
                    && pi.send(&owner, runtime.clone(), request.clone()).is_ok();
                Some(Event::Written {
                    runtime,
                    generation,
                    request,
                    success,
                })
            }
            Command::Register {
                runtime,
                generation,
                project,
                id,
                path,
                name,
            } => {
                if registry_generations.get(&runtime) != Some(&generation) {
                    continue;
                }
                let retry = Command::Register {
                    runtime: runtime.clone(),
                    generation,
                    project: project.clone(),
                    id: id.clone(),
                    path: path.clone(),
                    name: name.clone(),
                };
                let adopted = registration_is_adoption(&archive_intents, &project, &id);
                if session_names
                    .register(project, id, path, name, adopted)
                    .is_err()
                {
                    Some(Event::RegistrationFailed(retry))
                } else {
                    Some(Event::Workspace(storage::load_workspace().map_err(|_| ())))
                }
            }
            Command::StoreName {
                runtime,
                generation,
                revision,
                project,
                id,
                path,
                name,
            } => {
                let success = registry_generations.get(&runtime) == Some(&generation)
                    && session_names
                        .store(project, id.clone(), path.clone(), name.clone())
                        .is_ok_and(|row| row.is_some());
                Some(Event::NameStored {
                    runtime,
                    generation,
                    revision,
                    id,
                    path,
                    name,
                    success,
                })
            }
            Command::Archive {
                project,
                id,
                revision,
                archived,
            } => {
                // Retries and delayed durability callbacks are not new adoption intent.
                archive_intents.insert((project.clone(), id.clone()));
                let success = if archived {
                    storage::archive_session(project.clone(), id.clone())
                } else {
                    storage::unarchive_session(project.clone(), id.clone())
                }
                .is_ok();
                Some(Event::Archived {
                    project,
                    id,
                    revision,
                    archived,
                    success,
                })
            }
            Command::Select { project, session } => {
                let success = storage::set_active_project(project.clone())
                    .and_then(|_| storage::set_active_session(project, session))
                    .is_ok();
                (!success).then_some(Event::StorageFailed)
            }
            Command::Collapse { project, collapsed } => Some(Event::Workspace(
                storage::set_project_collapsed(project, collapsed)
                    .and_then(|_| storage::load_workspace())
                    .map_err(|_| ()),
            )),
            Command::Reorder(projects) => Some(Event::Workspace(
                storage::reorder_projects(projects)
                    .and_then(|_| storage::load_workspace())
                    .map_err(|_| ()),
            )),
            Command::Save {
                revision,
                preferences,
                shutdown,
            } => {
                let mut success = preferences
                    .as_ref()
                    .is_none_or(|preferences| write_preferences(&data, preferences).is_ok());
                let mut stopped = vec![];
                if success && shutdown {
                    // Failed/late starts can still own a manager process before a
                    // successful generation is recorded in the shell's map.
                    let running: Vec<_> = start_attempts.keys().cloned().collect();
                    for runtime in running {
                        match pi.stop(&owner, &runtime) {
                            Ok(generation) => {
                                generations.remove(&runtime);
                                if let Some(generation) = generation {
                                    stopped.push((runtime, generation));
                                }
                            }
                            Err(_) => success = false,
                        }
                    }
                }
                Some(Event::Saved {
                    revision,
                    success,
                    stopped,
                })
            }
            Command::Shutdown(done) => {
                pi.shutdown();
                let _ = done.send_blocking(());
                return;
            }
        };
        if let Some(event) = event {
            let _ = events.send_blocking(event);
        }
    }
    pi.shutdown();
}
#[derive(Default)]
struct SessionNames {
    authoritative: HashMap<(String, String, String), String>,
}
impl SessionNames {
    fn register(
        &self,
        project: String,
        id: String,
        path: String,
        captured_name: String,
        adopted: bool,
    ) -> Result<Option<tau_core::models::SessionSummary>, String> {
        let name = self
            .authoritative
            .get(&(project.clone(), id.clone(), path.clone()))
            .cloned()
            .unwrap_or(captured_name);
        storage::register_session(
            project,
            id,
            path,
            (!name.is_empty()).then_some(name),
            None,
            Some(adopted),
        )
    }
    fn store(
        &mut self,
        project: String,
        id: String,
        path: String,
        name: String,
    ) -> Result<Option<tau_core::models::SessionSummary>, String> {
        // Fence captured registration names even when this write needs retrying.
        // The binding survives runtime replacement; predecessor registration still runs.
        self.authoritative
            .insert((project.clone(), id.clone(), path.clone()), name.clone());
        storage::register_session(project, id, path, Some(name), None, Some(false))
    }
}
fn registration_is_adoption(
    intents: &std::collections::HashSet<(String, String)>,
    project: &str,
    id: &str,
) -> bool {
    !intents.contains(&(project.into(), id.into()))
}
fn read_preferences(data: &std::path::Path) -> Result<Preferences, String> {
    match std::fs::read(data.join("native-ui.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "preferences".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preferences::default()),
        Err(_) => Err("preferences".into()),
    }
}
fn write_preferences(data: &std::path::Path, preferences: &Preferences) -> Result<(), ()> {
    std::fs::create_dir_all(data).map_err(|_| ())?;
    let mut file = tempfile::NamedTempFile::new_in(data).map_err(|_| ())?;
    serde_json::to_writer(file.as_file_mut(), preferences).map_err(|_| ())?;
    file.as_file_mut().flush().map_err(|_| ())?;
    file.as_file().sync_all().map_err(|_| ())?;
    file.persist(data.join("native-ui.json")).map_err(|_| ())?;
    Ok(())
}

#[cfg(test)]
#[path = "service_name_tests.rs"]
mod name_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_registration_does_not_undo_explicit_archive_or_restore_intent() {
        let mut intents = std::collections::HashSet::new();
        assert!(registration_is_adoption(&intents, "project", "session"));
        intents.insert(("project".into(), "session".into()));
        assert!(!registration_is_adoption(&intents, "project", "session"));
        assert!(registration_is_adoption(&intents, "other", "session"));
        assert!(registration_is_adoption(&intents, "project", "replacement"));
    }
    #[test]
    fn preferences_are_distinct_and_recover_drafts() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = Preferences {
            theme: "dark".into(),
            selected_draft: String::new(),
            drafts: vec![Draft {
                text: "unsent".into(),
                ..Draft::default()
            }],
        };
        write_preferences(dir.path(), &prefs).unwrap();
        assert_eq!(
            read_preferences(dir.path()).unwrap().drafts[0].text,
            "unsent"
        );
        assert!(!dir.path().join(".tau.json").exists());
    }
    #[test]
    fn shutdown_ack_survives_saturated_event_channel_without_launching_pi() {
        let dir = tempfile::tempdir().unwrap();
        let (commands, events) = launch(dir.path().to_path_buf());
        for i in 0..600 {
            commands
                .send_blocking(Command::Send {
                    runtime: "never-started".into(),
                    generation: 1,
                    request: serde_json::json!({"id":i.to_string(), "type":"get_state"}),
                })
                .unwrap();
        }
        let (done, completed) = async_channel::bounded(1);
        commands.send_blocking(Command::Shutdown(done)).unwrap();
        events.close();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if completed.try_recv().is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "shutdown did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn malformed_preferences_do_not_silently_erase_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("native-ui.json"), b"invalid").unwrap();
        assert!(read_preferences(dir.path()).is_err());
    }
}
