use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, io::Write, path::PathBuf, sync::Arc};
use tau_core::{
    models::{ProjectSummary, WorkspaceSnapshot},
    pi::{PiEvent, PiSink, PiState},
    storage,
};
use tau_session::session::ModelScopeToken;

#[path = "service_admin.rs"]
mod admin_backend;
#[path = "service_lifecycle.rs"]
mod lifecycle;
#[path = "service_remote.rs"]
mod remote;
use lifecycle::{release_runtime, retry_claim, stop_cleanup};
pub use remote::{RemoteCommand, RemoteEvent, RemoteOperation, RemoteResult};

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: String,
    pub drafts: Vec<Draft>,
    pub selected_draft: String,
    pub sidebar_width: Option<f32>,
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
    Admin(super::admin::Command),
    Remote(RemoteCommand),
    ReadModelScope {
        runtime: String,
        token: ModelScopeToken,
        connection: Option<String>,
    },
    Load {
        project: Option<String>,
    },
    Import(String),
    ImportProject {
        path: String,
        token: u64,
    },
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
    ReleaseRuntime {
        runtime: String,
        attempt: u64,
        generation: u64,
    },
    RemoveProject {
        project: String,
        token: u64,
    },
    DisposeEmpty {
        runtime: String,
        attempt: u64,
        generation: u64,
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
    ArchiveSource {
        project: String,
        id: String,
        path: String,
        token: u64,
    },
    Archive {
        project: String,
        id: String,
        revision: u64,
        archived: bool,
    },
    WorkspaceMutation {
        revision: u64,
        mutation: super::workspace_mutations::Mutation,
    },
    Save {
        revision: u64,
        preferences: Option<Preferences>,
        shutdown: bool,
    },
    ExitFence {
        revision: u64,
    },
    Shutdown(Sender<()>),
}
pub enum Event {
    Admin(super::admin::Reply),
    Remote(RemoteEvent),
    ModelScope {
        runtime: String,
        token: ModelScopeToken,
        patterns: Option<Vec<String>>,
    },
    Loaded(Result<(WorkspaceSnapshot, Preferences), ()>),
    #[cfg(test)]
    Workspace(Result<WorkspaceSnapshot, ()>),
    WorkspaceMutation {
        revision: u64,
        success: bool,
    },
    ImportedProject(Result<ProjectSummary, ()>),
    ProjectImported {
        path: String,
        token: u64,
        result: Result<ProjectSummary, ()>,
    },
    RegisteredRuntimeRow {
        runtime: String,
        generation: u64,
        project: String,
        row: tau_core::models::SessionSummary,
    },
    Started {
        runtime: String,
        attempt: u64,
        result: Result<u64, ()>,
    },
    StartCancelled {
        runtime: String,
        attempt: u64,
        success: bool,
    },
    RuntimeReleased {
        runtime: String,
        attempt: u64,
        generation: u64,
        success: bool,
    },
    CleanupBlocked {
        runtime: String,
        attempt: u64,
        generation: Option<u64>,
    },
    ProjectRemoved {
        project: String,
        token: u64,
        success: bool,
    },
    EmptyDisposed {
        runtime: String,
        attempt: u64,
        generation: u64,
        success: bool,
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
    ExitDrained {
        revision: u64,
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
    SourceArchived {
        project: String,
        id: String,
        path: String,
        token: u64,
        success: bool,
    },
    Archived {
        project: String,
        id: String,
        revision: u64,
        archived: bool,
        success: bool,
    },
    RegistrationStored {
        runtime: String,
        generation: u64,
        id: String,
        path: String,
    },
    RegistrationFailed(Command),
}

pub fn launch(data: PathBuf) -> (Sender<Command>, Receiver<Event>) {
    let (commands, receiver) = async_channel::bounded(256);
    let (events, output) = async_channel::bounded(512);
    // Profile selection belongs to app startup, never the worker or isolated tests.
    #[cfg(not(test))]
    let telemetry = tau_observability::Telemetry::init().ok();
    #[cfg(test)]
    let telemetry: Option<Arc<tau_observability::Telemetry>> = None;
    if let Some(telemetry) = &telemetry {
        telemetry.record_lifecycle(tau_observability::telemetry::native::Lifecycle::Started);
    }
    std::thread::spawn(move || {
        worker_with_backend(
            data,
            receiver,
            events,
            remote::import,
            admin_backend::Backend(telemetry),
        );
    });
    (commands, output)
}
#[derive(Debug)]
struct ModelScopeRead {
    runtime: String,
    token: ModelScopeToken,
    connection: Option<String>,
}

fn launch_model_scope_reader(events: Sender<Event>) -> Sender<ModelScopeRead> {
    launch_model_scope_reader_with(events, |connection| {
        match connection {
            Some(connection) => tau_core::settings::read_remote_model_scope(connection),
            None => tau_core::settings::read_model_scope(),
        }
        .ok()
    })
}

fn launch_model_scope_reader_with(
    events: Sender<Event>,
    read: impl Fn(Option<String>) -> Option<Vec<String>> + Send + 'static,
) -> Sender<ModelScopeRead> {
    // SSH reads inherit tau-core's timeout/output limits, off the Pi command worker.
    let (jobs, receiver) = async_channel::bounded::<ModelScopeRead>(8);
    std::thread::spawn(move || {
        while let Ok(job) = receiver.recv_blocking() {
            if events.is_closed() {
                break;
            }
            let patterns = read(job.connection);
            if events
                .send_blocking(Event::ModelScope {
                    runtime: job.runtime,
                    token: job.token,
                    patterns,
                })
                .is_err()
            {
                break;
            }
        }
    });
    jobs
}

#[cfg(test)]
fn worker_with_import(
    data: PathBuf,
    commands: Receiver<Command>,
    events: Sender<Event>,
    import: impl Fn(&RemoteCommand) -> Result<RemoteResult, ()>,
) {
    worker_with_backend(data, commands, events, import, admin_backend::Backend(None));
}

fn worker_with_backend(
    data: PathBuf,
    commands: Receiver<Command>,
    events: Sender<Event>,
    import: impl Fn(&RemoteCommand) -> Result<RemoteResult, ()>,
    mut admin: admin_backend::Backend,
) {
    let remote_jobs = remote::launch(events.clone());
    let model_scope_jobs = launch_model_scope_reader(events.clone());
    let pi = PiState::default();
    let owner = uuid::Uuid::new_v4().to_string();
    let mut claim_revision = pi.read_revision().ok();
    let mut claimed = claim_revision
        .and_then(|r| pi.claim(owner.clone(), r).ok())
        .is_some();
    let mut removed_projects = std::collections::HashSet::new();
    let mut runtime_projects = HashMap::<String, String>::new();
    let mut generations = HashMap::new();
    let mut registry_generations = HashMap::new();
    let mut registration_tickets = lifecycle::RegistrationTickets::default();
    let mut start_attempts = HashMap::new();
    let mut retired_attempts = HashMap::new();
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
        if events.is_closed()
            && matches!(
                &command,
                Command::Start { .. }
                    | Command::Remote(RemoteCommand {
                        operation: RemoteOperation::Import,
                        ..
                    })
            )
        {
            continue;
        }
        let event = match command {
            Command::Admin(command) => Some(Event::Admin(admin.execute(command))),
            Command::Remote(command) if command.operation == RemoteOperation::Import => {
                let result = if runtime_projects
                    .values()
                    .any(|p| removed_projects.contains(p))
                {
                    Err(())
                } else {
                    import(&command)
                };
                if let Ok(RemoteResult::Imported(project)) = &result {
                    removed_projects.remove(&project.path);
                }
                Some(Event::Remote(RemoteEvent { result, command }))
            }
            Command::Remote(command) => match remote_jobs.try_send(command) {
                Ok(()) => None,
                Err(error) => Some(Event::Remote(RemoteEvent {
                    command: error.into_inner(),
                    result: Err(()),
                })),
            },
            Command::ReadModelScope {
                runtime,
                token,
                connection,
            } => match model_scope_jobs.try_send(ModelScopeRead {
                runtime,
                token,
                connection,
            }) {
                Ok(()) => None,
                Err(error) => {
                    let job = error.into_inner();
                    Some(Event::ModelScope {
                        runtime: job.runtime,
                        token: job.token,
                        patterns: None,
                    })
                }
            },
            Command::Load { project } => {
                // A conflict never obtains a fresh revision for forced takeover.
                retry_claim(
                    &mut claimed,
                    &mut claim_revision,
                    || pi.read_revision().ok(),
                    |revision| pi.claim(owner.clone(), revision).is_ok(),
                );
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
            Command::Import(path) => {
                let result = if runtime_projects
                    .values()
                    .any(|p| p == &path && removed_projects.contains(p))
                {
                    Err(())
                } else {
                    storage::import_project(path).map_err(|_| ())
                };
                if let Ok(project) = &result {
                    removed_projects.remove(&project.path);
                }
                Some(Event::ImportedProject(result))
            }
            Command::ImportProject { path, token } => {
                let result = if runtime_projects
                    .values()
                    .any(|p| p == &path && removed_projects.contains(p))
                {
                    Err(())
                } else {
                    storage::import_project(path.clone()).map_err(|_| ())
                };
                if let Ok(project) = &result {
                    removed_projects.remove(&project.path);
                }
                Some(Event::ProjectImported {
                    path,
                    token,
                    result,
                })
            }
            Command::RemoveProject { project, token } => {
                let success = storage::remove_project(project.clone()).is_ok();
                if success {
                    removed_projects.insert(project.clone());
                    registration_tickets.revoke_project(&project);
                    registry_generations
                        .retain(|runtime, _| runtime_projects.get(runtime) != Some(&project));
                    session_names
                        .authoritative
                        .retain(|(path, _, _), _| path != &project);
                    archive_intents.retain(|(path, _)| path != &project);
                }
                Some(Event::ProjectRemoved {
                    project,
                    token,
                    success,
                })
            }
            Command::Start {
                runtime,
                attempt,
                project,
                source,
            } => {
                if removed_projects.contains(&project.path) {
                    let _ = events.send_blocking(Event::Started {
                        runtime,
                        attempt,
                        result: Err(()),
                    });
                    continue;
                }
                if retired_attempts
                    .get(&runtime)
                    .is_some_and(|retired| attempt <= *retired)
                    || start_attempts
                        .get(&runtime)
                        .is_some_and(|started| attempt <= *started)
                {
                    continue;
                }
                runtime_projects.insert(runtime.clone(), project.path.clone());
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
                    registration_tickets.started(&runtime, generation, &project.path);
                }
                Some(Event::Started {
                    runtime,
                    attempt,
                    result: result.map_err(|_| ()),
                })
            }
            Command::CancelStart { runtime, attempt } => {
                let current = start_attempts.get(&runtime) == Some(&attempt);
                let success =
                    !current || stop_cleanup(&pi, &owner, &runtime, attempt, None, &events);
                if current && success {
                    generations.remove(&runtime);
                    registry_generations.remove(&runtime);
                    start_attempts.remove(&runtime);
                    runtime_projects.remove(&runtime);
                }
                Some(Event::StartCancelled {
                    runtime,
                    attempt,
                    success,
                })
            }
            Command::ReleaseRuntime {
                runtime,
                attempt,
                generation,
            } => {
                let success = release_runtime(
                    &runtime,
                    attempt,
                    generation,
                    &mut start_attempts,
                    &mut generations,
                    &mut registry_generations,
                    || {
                        stop_cleanup(&pi, &owner, &runtime, attempt, Some(generation), &events)
                            .then_some(())
                            .ok_or(())
                    },
                );
                if success && !start_attempts.contains_key(&runtime) {
                    runtime_projects.remove(&runtime);
                }
                Some(Event::RuntimeReleased {
                    runtime,
                    attempt,
                    generation,
                    success,
                })
            }
            Command::DisposeEmpty {
                runtime,
                attempt,
                generation,
            } => {
                let matches = retire_empty(
                    &runtime,
                    attempt,
                    generation,
                    &start_attempts,
                    &mut generations,
                    &mut registry_generations,
                    &mut retired_attempts,
                );
                let success = if matches {
                    if stop_cleanup(&pi, &owner, &runtime, attempt, Some(generation), &events) {
                        start_attempts.remove(&runtime);
                        runtime_projects.remove(&runtime);
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }; // A successor owns this key; never stop it.
                Some(Event::EmptyDisposed {
                    runtime,
                    attempt,
                    generation,
                    success,
                })
            }
            Command::Send {
                runtime,
                generation,
                request,
            } => {
                let success = generations.get(&runtime) == Some(&generation)
                    && !runtime_projects
                        .get(&runtime)
                        .is_some_and(|project| removed_projects.contains(project))
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
                let row_runtime = runtime.clone();
                let retry = Command::Register {
                    runtime: runtime.clone(),
                    generation,
                    project: project.clone(),
                    id: id.clone(),
                    path: path.clone(),
                    name: name.clone(),
                };
                let admitted = registration_tickets.admit(
                    &retry,
                    &registry_generations,
                    &runtime_projects,
                    &removed_projects,
                );
                let adopted = registration_is_adoption(&archive_intents, &project, &id);
                let result = if admitted {
                    session_names.register(project.clone(), id, path, name, adopted)
                } else {
                    Err("registration ownership revoked".into())
                };
                let row = result.as_ref().ok().and_then(|row| row.clone());
                let reply = registration_reply(retry, result);
                if matches!(reply, Event::RegistrationStored { .. }) {
                    // The accepted row is both durability acknowledgement and metadata;
                    // do not discover every other session after a single registration.
                    if let Some(row) = row
                        && events
                            .send_blocking(Event::RegisteredRuntimeRow {
                                runtime: row_runtime,
                                generation,
                                project,
                                row,
                            })
                            .is_err()
                    {
                        break;
                    }
                    Some(reply)
                } else {
                    Some(reply)
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
                let success = !removed_projects.contains(&project)
                    && registry_generations.get(&runtime) == Some(&generation)
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
            Command::ArchiveSource {
                project,
                id,
                path,
                token,
            } => {
                archive_intents.insert((project.clone(), id.clone()));
                let success = !removed_projects.contains(&project)
                    && storage::archive_session(project.clone(), id.clone()).is_ok();
                Some(Event::SourceArchived {
                    project,
                    id,
                    path,
                    token,
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
                let success = if removed_projects.contains(&project) {
                    Err("removed".into())
                } else if archived {
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
            Command::WorkspaceMutation { revision, mutation } => Some(Event::WorkspaceMutation {
                revision,
                success: write_workspace_mutation(mutation).is_ok(),
            }),
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
                    match pi.stop_many(&owner, &running) {
                        Ok(outcomes) => {
                            for (runtime, outcome) in outcomes {
                                match outcome {
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
                        Err(_) => success = false,
                    }
                }
                Some(Event::Saved {
                    revision,
                    success,
                    stopped,
                })
            }
            Command::ExitFence { revision } => Some(Event::ExitDrained { revision }),
            Command::Shutdown(done) => {
                pi.shutdown();
                admin.finish();
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
fn write_workspace_mutation(mutation: super::workspace_mutations::Mutation) -> Result<(), String> {
    use super::workspace_mutations::Mutation;
    match mutation {
        Mutation::Collapse { project, collapsed } => {
            storage::set_project_collapsed(project, collapsed)
        }
        Mutation::Order(projects) => storage::reorder_projects(projects),
        Mutation::Select { project, session } if session.is_empty() => {
            storage::set_active_project(project)
        }
        // set_active_session already persists the active project under the storage lock.
        Mutation::Select { project, session } => storage::set_active_session(project, session),
    }
}

#[cfg(test)]
#[path = "service_workspace_tests.rs"]
mod workspace_tests;

fn retire_empty(
    runtime: &str,
    attempt: u64,
    generation: u64,
    starts: &HashMap<String, u64>,
    generations: &mut HashMap<String, u64>,
    registry: &mut HashMap<String, u64>,
    retired: &mut HashMap<String, u64>,
) -> bool {
    if !disposal_matches(runtime, attempt, generation, starts, generations, retired) {
        return false;
    }
    // Revoke sends and durability retries before potentially failing cleanup.
    retired
        .entry(runtime.into())
        .and_modify(|retired| *retired = (*retired).max(attempt))
        .or_insert(attempt);
    generations.remove(runtime);
    registry.remove(runtime);
    true
}

fn disposal_matches(
    runtime: &str,
    attempt: u64,
    generation: u64,
    starts: &HashMap<String, u64>,
    generations: &HashMap<String, u64>,
    retired: &HashMap<String, u64>,
) -> bool {
    (starts.get(runtime) == Some(&attempt)
        && (retired.get(runtime) == Some(&attempt)
            || generation == 0
            || generations.get(runtime) == Some(&generation)))
        || (!starts.contains_key(runtime) && !generations.contains_key(runtime))
}

#[cfg(test)]
#[path = "service_empty_tests.rs"]
mod empty_tests;

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
fn registration_reply(
    command: Command,
    result: Result<Option<tau_core::models::SessionSummary>, String>,
) -> Event {
    if matches!(result, Ok(Some(_))) {
        let Command::Register {
            runtime,
            generation,
            id,
            path,
            ..
        } = command
        else {
            unreachable!("registration reply requires its captured command")
        };
        Event::RegistrationStored {
            runtime,
            generation,
            id,
            path,
        }
    } else {
        Event::RegistrationFailed(command)
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
            sidebar_width: None,
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
    fn settings_reader_is_bounded_and_preserves_result_tokens_without_real_io() {
        let (events, output) = async_channel::bounded(16);
        let (entered, started) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let jobs = launch_model_scope_reader_with(events, move |connection| {
            if connection.as_deref() == Some("blocked") {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                Some(vec!["fixture/beta".into()])
            } else {
                assert!(connection.is_none());
                None
            }
        });
        let mut controller = tau_session::session::Controller::new("scope");
        controller.begin_generation(1);
        let token = controller.begin_model_scope_read();
        jobs.try_send(ModelScopeRead {
            runtime: "remote".into(),
            token: token.clone(),
            connection: Some("blocked".into()),
        })
        .unwrap_or_else(|_| panic!("reader should accept first job"));
        started
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        // A slow remote read cannot create unbounded threads or queued work.
        for _ in 0..8 {
            jobs.try_send(ModelScopeRead {
                runtime: "local".into(),
                token: token.clone(),
                connection: None,
            })
            .unwrap_or_else(|_| panic!("bounded queue should accept eight jobs"));
        }
        let rejected = jobs
            .try_send(ModelScopeRead {
                runtime: "rejected".into(),
                token: token.clone(),
                connection: None,
            })
            .expect_err("full queue must reject without blocking")
            .into_inner();
        assert_eq!(rejected.runtime, "rejected");
        assert_eq!(rejected.token, token);
        release.send(()).unwrap();
        let Event::ModelScope {
            runtime,
            token: returned,
            patterns,
        } = output.recv_blocking().unwrap()
        else {
            panic!("expected settings result");
        };
        assert_eq!(runtime, "remote");
        assert_eq!(returned, token);
        assert_eq!(patterns, Some(vec!["fixture/beta".into()]));
        for _ in 0..8 {
            let Event::ModelScope {
                runtime,
                token: returned,
                patterns,
            } = output.recv_blocking().unwrap()
            else {
                panic!("expected failed settings result");
            };
            assert_eq!(runtime, "local");
            assert_eq!(returned, token);
            assert!(patterns.is_none());
        }
        jobs.close();
    }

    #[test]
    fn malformed_preferences_do_not_silently_erase_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("native-ui.json"), b"invalid").unwrap();
        assert!(read_preferences(dir.path()).is_err());
    }
}
