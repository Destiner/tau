use super::Event;
use async_channel::Sender;
use tau_core::{
    models::{ProjectSummary, RemoteDirectoryListing},
    ssh, storage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteOperation {
    Probe,
    List,
    Import,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteCommand {
    pub revision: u64,
    pub operation: RemoteOperation,
    pub connection: String,
    pub path: String,
    pub host: String,
}
pub enum RemoteResult {
    Listing(RemoteDirectoryListing),
    Imported(ProjectSummary),
}
pub struct RemoteEvent {
    pub command: RemoteCommand,
    pub result: Result<RemoteResult, ()>,
}

// Short-lived SSH inspection must never hold the Pi transport/Stop worker.
pub(super) fn launch(events: Sender<Event>) -> Sender<RemoteCommand> {
    let (jobs, receiver) = async_channel::bounded::<RemoteCommand>(8);
    std::thread::spawn(move || {
        while let Ok(command) = receiver.recv_blocking() {
            if events.is_closed() {
                break;
            }
            let result = execute(&command);
            if events
                .send_blocking(Event::Remote(RemoteEvent { command, result }))
                .is_err()
            {
                break;
            }
        }
    });
    jobs
}
fn execute(command: &RemoteCommand) -> Result<RemoteResult, ()> {
    // Core validation remains authoritative; errors can include stderr and are discarded.
    ssh::SshConnection::parse(&command.connection).map_err(|_| ())?;
    match command.operation {
        RemoteOperation::Probe => {
            ssh::probe_remote_project(command.connection.clone()).map(RemoteResult::Listing)
        }
        RemoteOperation::List => {
            ssh::list_remote_directories(command.connection.clone(), command.path.clone())
                .map(RemoteResult::Listing)
        }
        // Imports belong to the serialized service worker, never this SSH worker.
        RemoteOperation::Import => return Err(()),
    }
    .map_err(|_| ())
}

pub(super) fn import(command: &RemoteCommand) -> Result<RemoteResult, ()> {
    if command.operation != RemoteOperation::Import {
        return Err(());
    }
    ssh::SshConnection::parse(&command.connection).map_err(|_| ())?;
    storage::import_remote_project(
        command.connection.clone(),
        command.path.clone(),
        command.host.clone(),
    )
    .map(RemoteResult::Imported)
    .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_connections_fail_before_any_ssh_or_storage_operation() {
        for operation in [
            RemoteOperation::Probe,
            RemoteOperation::List,
            RemoteOperation::Import,
        ] {
            let command = RemoteCommand {
                revision: 1,
                operation,
                connection: String::new(),
                path: "/fixture".into(),
                host: "fixture".into(),
            };
            assert!(execute(&command).is_err());
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::super::{Command, worker_with_import};
    use super::*;

    fn command(revision: u64) -> RemoteCommand {
        RemoteCommand {
            revision,
            operation: RemoteOperation::Import,
            connection: "ssh fixture".into(),
            path: "/fixture".into(),
            host: "fixture".into(),
        }
    }

    fn project() -> ProjectSummary {
        ProjectSummary {
            path: "ssh:fixture".into(),
            name: "fixture".into(),
            working_directory: "/fixture".into(),
            connection_string: Some("ssh fixture".into()),
            collapsed: false,
            selected: false,
            sessions: vec![],
        }
    }

    #[test]
    fn imports_execute_on_main_worker_in_fifo_order_without_starting_pi() {
        let dir = tempfile::tempdir().unwrap();
        let (commands, receiver) = async_channel::bounded(8);
        let (events, output) = async_channel::bounded(8);
        let (entered, started) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_with_import(dir.path().into(), receiver, events, |command| {
                entered.send(command.revision).unwrap();
                if command.revision == 1 {
                    wait.recv().unwrap();
                }
                Ok(RemoteResult::Imported(project()))
            });
        });
        commands.send_blocking(Command::Remote(command(1))).unwrap();
        assert_eq!(
            started
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap(),
            1
        );
        commands
            .send_blocking(Command::Send {
                runtime: "never-started".into(),
                generation: 1,
                request: serde_json::json!({"type":"get_state"}),
            })
            .unwrap();
        commands.send_blocking(Command::Remote(command(2))).unwrap();
        assert!(output.try_recv().is_err());
        assert!(started.try_recv().is_err());
        release.send(()).unwrap();
        for revision in [1, 2] {
            if revision == 2 {
                assert!(matches!(
                    output.recv_blocking().unwrap(),
                    Event::Written { success: false, .. }
                ));
            }
            let Event::Remote(event) = output.recv_blocking().unwrap() else {
                panic!("import event")
            };
            assert_eq!(event.command.revision, revision);
            assert!(
                matches!(event.result, Ok(RemoteResult::Imported(project)) if project.path == "ssh:fixture")
            );
        }
        let (done, completed) = async_channel::bounded(1);
        commands.send_blocking(Command::Shutdown(done)).unwrap();
        completed.recv_blocking().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn closed_output_skips_import_storage_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let (commands, receiver) = async_channel::bounded(8);
        let (events, output) = async_channel::bounded(8);
        output.close();
        commands.send_blocking(Command::Remote(command(1))).unwrap();
        commands.close();
        worker_with_import(dir.path().into(), receiver, events, |_| {
            panic!("late storage write")
        });
    }

    #[test]
    fn ssh_worker_refuses_import_even_with_valid_connection() {
        assert!(execute(&command(1)).is_err());
    }

    #[test]
    fn import_returns_project_despite_unrelated_broken_project() {
        const CHILD: &str = "TAU_REMOTE_IMPORT_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let dir = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::service::remote::review_tests::import_returns_project_despite_unrelated_broken_project", "--nocapture"])
                .env(CHILD, "1")
                .env("PI_CODING_AGENT_DIR", dir.path())
                .status().unwrap();
            assert!(status.success());
            return;
        }
        tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap())
            .unwrap();
        let local = tempfile::tempdir().unwrap();
        let local_project = storage::import_project(local.path().to_string_lossy().into()).unwrap();
        let session_dir = storage::default_session_dir(&local_project.path).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(
            session_dir.join(tau_core::profile::session_registry_filename().unwrap()),
            b"invalid",
        )
        .unwrap();
        assert!(
            storage::load_workspace().is_err(),
            "fixture must break workspace scanning"
        );
        let RemoteResult::Imported(project) = import(&command(1)).unwrap() else {
            panic!("project")
        };
        assert_eq!(project.working_directory, "/fixture");
        assert!(!project.selected);
        assert!(storage::load_workspace().is_err());

        let data = tempfile::tempdir().unwrap();
        let (commands, events) = super::super::launch(data.path().into());
        commands
            .send_blocking(Command::Collapse {
                project: project.path.clone(),
                collapsed: true,
            })
            .unwrap();
        commands.send_blocking(Command::Remote(command(2))).unwrap();
        commands
            .send_blocking(Command::Reorder(vec![project.path]))
            .unwrap();
        assert!(matches!(
            events.recv_blocking().unwrap(),
            Event::Workspace(Err(()))
        ));
        let Event::Remote(event) = events.recv_blocking().unwrap() else {
            panic!("FIFO import")
        };
        assert_eq!(event.command.revision, 2);
        assert!(matches!(event.result, Ok(RemoteResult::Imported(project)) if project.collapsed));
        assert!(matches!(
            events.recv_blocking().unwrap(),
            Event::Workspace(Err(()))
        ));
        let (done, completed) = async_channel::bounded(1);
        commands.send_blocking(Command::Shutdown(done)).unwrap();
        completed.recv_blocking().unwrap();
    }
}
