use super::*;

fn registration() -> Command {
    Command::Register {
        runtime: "runtime".into(),
        generation: 7,
        project: "/project".into(),
        id: "p".into(),
        path: "/p".into(),
        name: "P".into(),
    }
}

#[test]
fn only_accepted_registration_produces_correlated_success() {
    let row = tau_core::models::SessionSummary {
        id: "p".into(),
        path: "/p".into(),
        title: "P".into(),
        title_markdown: None,
        model: String::new(),
        last_active: String::new(),
        last_user_message_at: 0,
        sort_at: 0,
        archived: false,
        selected: false,
    };
    assert!(matches!(registration_reply(registration(), Ok(Some(row))),
        Event::RegistrationStored { runtime, generation: 7, id, path }
        if runtime == "runtime" && id == "p" && path == "/p"));
    for result in [Ok(None), Err("not accepted".into())] {
        assert!(matches!(registration_reply(registration(), result),
            Event::RegistrationFailed(Command::Register { runtime, generation: 7, id, path, .. })
            if runtime == "runtime" && id == "p" && path == "/p"));
    }
}

#[test]
fn retirement_revokes_send_and_registry_even_when_stop_needs_retry() {
    let starts = HashMap::from([("empty".into(), 3), ("other".into(), 1)]);
    let mut generations = HashMap::from([("empty".into(), 9), ("other".into(), 10)]);
    let mut registry = generations.clone();
    let mut retired = HashMap::new();
    assert!(retire_empty(
        "empty",
        3,
        9,
        &starts,
        &mut generations,
        &mut registry,
        &mut retired
    ));
    assert!(!generations.contains_key("empty"));
    assert!(!registry.contains_key("empty"));
    assert_eq!(generations["other"], 10);
    assert_eq!(registry["other"], 10);
    // Simulate stop failure: the attempt still belongs to the manager, but not to IO.
    assert!(retire_empty(
        "empty",
        3,
        9,
        &starts,
        &mut generations,
        &mut registry,
        &mut retired
    ));
    assert_eq!(retired["empty"], 3);
}

#[test]
fn disposal_never_revokes_successor_attempt_or_mismatched_generation() {
    let starts = HashMap::from([("session".into(), 4)]);
    let mut generations = HashMap::from([("session".into(), 12)]);
    let mut registry = generations.clone();
    let mut retired = HashMap::from([("session".into(), 3)]);
    for (attempt, generation) in [(3, 9), (4, 9), (3, 0)] {
        assert!(!retire_empty(
            "session",
            attempt,
            generation,
            &starts,
            &mut generations,
            &mut registry,
            &mut retired
        ));
        assert_eq!(generations["session"], 12);
        assert_eq!(registry["session"], 12);
        assert_eq!(retired["session"], 3);
    }
    // A disposal submitted while Start is pending knows only its attempt, not generation.
    assert!(retire_empty(
        "session",
        4,
        0,
        &starts,
        &mut generations,
        &mut registry,
        &mut retired
    ));
}

#[test]
fn never_started_retirement_also_fences_delayed_start_retry() {
    let mut generations = HashMap::new();
    let mut registry = HashMap::new();
    let mut retired = HashMap::new();
    for attempt in [5, 2] {
        assert!(retire_empty(
            "session",
            attempt,
            0,
            &HashMap::new(),
            &mut generations,
            &mut registry,
            &mut retired
        ));
    }
    assert_eq!(retired["session"], 5);
}

#[test]
fn fifo_worker_acknowledges_disposal_and_rejects_post_retirement_send_without_pi() {
    let dir = tempfile::tempdir().unwrap();
    let predecessor = dir.path().join("predecessor-p.jsonl");
    let predecessor_bytes = b"{\"type\":\"session\",\"id\":\"p\"}\n";
    std::fs::write(&predecessor, predecessor_bytes).unwrap();
    let (commands, input) = async_channel::bounded(16);
    let (events, output) = async_channel::bounded(16);
    let data = dir.path().to_path_buf();
    let worker = std::thread::spawn(move || worker_with_import(data, input, events, |_| Err(())));
    commands
        .send_blocking(Command::DisposeEmpty {
            runtime: "never-started".into(),
            attempt: 1,
            generation: 0,
        })
        .unwrap();
    // A delayed retry of the retired attempt must not launch a runtime at all.
    commands
        .send_blocking(Command::Start {
            runtime: "never-started".into(),
            attempt: 1,
            project: crate::app::tests::project(),
            source: None,
        })
        .unwrap();
    commands
        .send_blocking(Command::Send {
            runtime: "never-started".into(),
            generation: 1,
            request: serde_json::json!({"id":"late", "type":"prompt", "message":"do not send"}),
        })
        .unwrap();
    // Exit retries can already be queued when the first cleanup ack reaches UI.
    commands
        .send_blocking(Command::DisposeEmpty {
            runtime: "never-started".into(),
            attempt: 1,
            generation: 0,
        })
        .unwrap();
    commands
        .send_blocking(Command::Save {
            revision: 42,
            preferences: Some(Preferences::default()),
            shutdown: true,
        })
        .unwrap();
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::EmptyDisposed { success: true, .. }
    ));
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::Written { success: false, .. }
    ));
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::EmptyDisposed { success: true, .. }
    ));
    assert!(matches!(
        output.recv_blocking().unwrap(),
        Event::Saved {
            revision: 42,
            success: true,
            ..
        }
    ));
    let (done, completed) = async_channel::bounded(1);
    commands.send_blocking(Command::Shutdown(done)).unwrap();
    completed.recv_blocking().unwrap();
    worker.join().unwrap();
    assert_eq!(std::fs::read(predecessor).unwrap(), predecessor_bytes);
}
