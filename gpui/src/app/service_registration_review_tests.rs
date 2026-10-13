use super::*;

fn receipt(output: &Receiver<Event>) -> Event {
    loop {
        let event = output.recv_blocking().unwrap();
        if !matches!(event, Event::Pi(_)) {
            return event;
        }
    }
}

#[test]
fn real_worker_storage_failure_restart_retry_and_remove_reimport_incarnation_revocation() {
    const CHILD: &str = "TAU_REGISTER_REVIEW_CHILD";
    if std::env::var_os(CHILD).is_none() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let fake = directory.path().join("fake-pi");
        std::fs::write(&fake, "#!/bin/sh\nwhile IFS= read -r line; do :; done\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app::service::lifecycle::registration_review_tests::real_worker_storage_failure_restart_retry_and_remove_reimport_incarnation_revocation", "--nocapture"])
            .env(CHILD, "1").env("TAU_PI_PATH", &fake)
            .env("PI_CODING_AGENT_DIR", directory.path().join("pi"))
            .status().unwrap();
        assert!(status.success());
        return;
    }
    tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap()).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let project = storage::import_project(directory.path().to_string_lossy().into_owned()).unwrap();
    let sessions = storage::default_session_dir(&project.path).unwrap();
    std::fs::create_dir_all(&sessions).unwrap();
    let transcript = sessions.join("2026-01-01_proven.jsonl");
    let data = tempfile::tempdir().unwrap();
    let (commands, output) = launch(data.path().into());
    let start = |attempt| Command::Start {
        runtime: "r".into(),
        attempt,
        project: project.clone(),
        source: None,
    };
    commands.send_blocking(start(1)).unwrap();
    let Event::Started {
        result: Ok(old), ..
    } = receipt(&output)
    else {
        panic!("fake local Pi must start")
    };
    let captured = Command::Register {
        runtime: "r".into(),
        generation: old,
        project: project.path.clone(),
        id: "proven".into(),
        path: transcript.to_string_lossy().into_owned(),
        name: "Predecessor".into(),
    };
    commands.send_blocking(captured.clone()).unwrap();
    // This is the actual storage's absent-file validation, not a fabricated UI failure.
    assert!(
        matches!(receipt(&output), Event::RegistrationFailed(Command::Register { generation, .. }) if generation == old)
    );
    commands.send_blocking(start(2)).unwrap();
    let Event::Started {
        result: Ok(new), ..
    } = receipt(&output)
    else {
        panic!("transport restart")
    };
    assert_ne!(old, new);
    let bytes = serde_json::to_vec(&serde_json::json!({"type":"session", "version":3, "id":"proven", "timestamp":"2026-01-01T00:00:00Z", "cwd":project.working_directory})).unwrap();
    std::fs::write(&transcript, &bytes).unwrap();
    commands.send_blocking(captured.clone()).unwrap();
    commands
        .send_blocking(Command::ExitFence { revision: 44 })
        .unwrap();
    assert!(
        matches!(receipt(&output), Event::RegisteredRuntimeRow { generation, row, .. } if generation == old && row.id == "proven")
    );
    assert!(
        matches!(receipt(&output), Event::RegistrationStored { generation, id, .. } if generation == old && id == "proven")
    );
    assert!(matches!(
        receipt(&output),
        Event::ExitDrained { revision: 44 }
    ));
    assert!(
        storage::load_workspace().unwrap().projects[0]
            .sessions
            .iter()
            .any(|row| row.id == "proven")
    );
    commands
        .send_blocking(Command::RemoveProject {
            project: project.path.clone(),
            token: 1,
        })
        .unwrap();
    assert!(matches!(
        receipt(&output),
        Event::ProjectRemoved { success: true, .. }
    ));
    commands
        .send_blocking(Command::ReleaseRuntime {
            runtime: "r".into(),
            attempt: 2,
            generation: new,
        })
        .unwrap();
    assert!(matches!(
        receipt(&output),
        Event::RuntimeReleased { success: true, .. }
    ));
    commands
        .send_blocking(Command::ImportProject {
            path: project.path.clone(),
            token: 2,
        })
        .unwrap();
    assert!(matches!(
        receipt(&output),
        Event::ProjectImported { result: Ok(_), .. }
    ));
    commands.send_blocking(start(3)).unwrap();
    assert!(
        matches!(receipt(&output), Event::Started { result: Ok(g), .. } if g != old && g != new)
    );
    // Reimport and a fresh process do not re-authorise an old incarnation's ticket.
    commands.send_blocking(captured).unwrap();
    assert!(
        matches!(receipt(&output), Event::RegistrationFailed(Command::Register { generation, .. }) if generation == old)
    );
    assert_eq!(std::fs::read(&transcript).unwrap(), bytes);
    let (done, completed) = async_channel::bounded(1);
    commands.send_blocking(Command::Shutdown(done)).unwrap();
    completed.recv_blocking().unwrap();
}

#[test]
fn exact_ticket_survives_restart_but_mutated_payload_and_revoked_incarnation_do_not() {
    let mut tickets = RegistrationTickets::default();
    let projects = HashMap::from([("r".into(), "p".into())]);
    let mut current = HashMap::from([("r".into(), 7)]);
    let removed = std::collections::HashSet::new();
    let command = Command::Register {
        runtime: "r".into(),
        generation: 7,
        project: "p".into(),
        id: "a".into(),
        path: "/a".into(),
        name: "A".into(),
    };
    assert!(tickets.admit(&command, &current, &projects, &removed));
    current.insert("r".into(), 8);
    assert!(tickets.admit(&command, &current, &projects, &removed));
    let mut arbitrary = command.clone();
    if let Command::Register { path, .. } = &mut arbitrary {
        *path = "/other".into();
    }
    assert!(!tickets.admit(&arbitrary, &current, &projects, &removed));
    tickets.revoke_project("p");
    tickets.started("r", 8, "p");
    assert!(!tickets.admit(&command, &current, &projects, &removed));
}
