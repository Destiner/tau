use super::super::workspace_mutations::Mutation;
use super::*;

#[test]
fn metadata_worker_acks_without_workspace_discovery() {
    const CHILD: &str = "TAU_WORKSPACE_METADATA_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::service::workspace_tests::metadata_worker_acks_without_workspace_discovery",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PI_CODING_AGENT_DIR", directory.path())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap()).unwrap();
    let local = tempfile::tempdir().unwrap();
    let broken = storage::import_project(local.path().to_string_lossy().into_owned()).unwrap();
    let directory = storage::default_session_dir(&broken.path).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(tau_core::profile::session_registry_filename().unwrap()),
        b"invalid",
    )
    .unwrap();
    let healthy =
        storage::import_remote_project("fixture".into(), "/healthy".into(), "fixture".into())
            .unwrap();
    assert!(
        storage::load_workspace().is_err(),
        "a full discovery must fail"
    );

    // Registration already returns the only row needed by the UI; no global scan.
    let row = SessionNames::default()
        .register(
            healthy.path.clone(),
            "saved".into(),
            "/saved.jsonl".into(),
            "Saved".into(),
            true,
        )
        .unwrap()
        .unwrap();
    assert_eq!(row.id, "saved");
    assert!(storage::load_workspace().is_err());

    let data = tempfile::tempdir().unwrap();
    let (commands, output) = launch(data.path().into());
    for (index, mutation) in [
        Mutation::Collapse {
            project: broken.path.clone(),
            collapsed: true,
        },
        Mutation::Collapse {
            project: broken.path.clone(),
            collapsed: false,
        },
        Mutation::Order(vec![healthy.path.clone(), broken.path.clone()]),
        Mutation::Select {
            project: healthy.path.clone(),
            session: "saved".into(),
        },
        Mutation::Select {
            project: healthy.path.clone(),
            session: String::new(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        commands
            .send_blocking(Command::WorkspaceMutation {
                revision: index as u64 + 1,
                mutation,
            })
            .unwrap();
    }
    commands
        .send_blocking(Command::Send {
            runtime: "not-started".into(),
            generation: 1,
            request: serde_json::json!({"type":"get_state"}),
        })
        .unwrap();
    for revision in 1..=5 {
        assert!(
            matches!(output.recv_blocking().unwrap(), Event::WorkspaceMutation { revision: returned, success: true } if returned == revision)
        );
    }
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::Written { success: false, .. }
    ));
    commands
        .send_blocking(Command::WorkspaceMutation {
            revision: 6,
            mutation: Mutation::Order(vec![healthy.path.clone(), healthy.path.clone()]),
        })
        .unwrap();
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::WorkspaceMutation {
            revision: 6,
            success: false
        }
    ));
    let added = tempfile::tempdir().unwrap();
    commands
        .send_blocking(Command::Import(added.path().to_string_lossy().into_owned()))
        .unwrap();
    assert!(
        matches!(output.recv_blocking().unwrap(), Event::ImportedProject(Ok(project)) if project.path == added.path().canonicalize().unwrap().to_string_lossy())
    );
    assert!(
        storage::load_workspace().is_err(),
        "cheap writes must not repair or rescan the broken history registry"
    );
    let (done, completed) = async_channel::bounded(1);
    commands.send_blocking(Command::Shutdown(done)).unwrap();
    completed.recv_blocking().unwrap();
}
