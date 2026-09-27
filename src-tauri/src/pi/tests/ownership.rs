use super::*;

#[test]
fn frontend_takeover_stops_stale_runtime_before_replacement_starts() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = NativePiFixture::new(None);
    let (app, events) = fixture.app_and_events();
    let handle = app.handle().clone();
    let state = handle.state::<PiState>();
    let telemetry = handle.state::<Telemetry>();
    claim_for_test(&state, "owner-a");

    let first_generation = start_pi_with(
        handle.clone(),
        &state,
        &telemetry,
        None,
        "owner-a".into(),
        "runtime-a".into(),
        fixture.project_string(),
        Some(fixture.session_string()),
    )
    .expect("start first frontend runtime");
    expect_outer_event(&events, "started", first_generation);
    let old_child =
        Arc::clone(&state.inner.lock().expect("Pi manager").processes["runtime-a"].child);

    let outcome =
        claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1).expect("replace frontend owner");
    record_ownership_cleanup(&telemetry, None, &outcome.stopped);
    telemetry.record_ownership_event("replacement", "success", outcome.stopped.len(), None);
    assert!(old_child
        .lock()
        .expect("old child")
        .try_wait()
        .expect("old child status")
        .is_some());
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());

    let second_generation = start_pi_with(
        handle.clone(),
        &state,
        &telemetry,
        None,
        "owner-b".into(),
        "runtime-b".into(),
        fixture.project_string(),
        Some(fixture.session_string()),
    )
    .expect("start replacement frontend runtime");
    assert!(second_generation > first_generation);
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        let event = events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("replacement started event");
        if event["generation"] == first_generation {
            assert_eq!(event["kind"], "stderr");
            continue;
        }
        assert_eq!(event["generation"], second_generation);
        assert_eq!(event["kind"], "started");
        break;
    }
    assert!(send_pi_to_state(
        &state,
        "owner-a",
        "runtime-b".into(),
        json!({"id": "stale", "type": "get_state"}),
    )
    .expect_err("stale owner must be fenced")
    .contains("no longer owns"));
    assert!(state
        .inner
        .lock()
        .expect("Pi manager")
        .processes
        .contains_key("runtime-b"));
    state.shutdown();
}

#[test]
fn frontend_takeover_retries_kill_after_child_lock_contention() {
    let state = PiState::default();
    claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0).expect("first claim");
    let process = sleeping_process(1);
    let child = Arc::clone(&process.child);
    state
        .inner
        .lock()
        .expect("Pi manager")
        .processes
        .insert("runtime-a".into(), process);
    let (locked_sender, locked_receiver) = mpsc::channel();
    let holder = thread::spawn(move || {
        let _guard = child.lock().expect("child lock");
        locked_sender.send(()).expect("report child lock");
        thread::sleep(PI_STOP_GRACE_PERIOD + Duration::from_millis(50));
    });
    locked_receiver.recv().expect("wait for child lock");

    claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1)
        .expect("takeover after child lock contention");

    holder.join().expect("child lock holder");
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
}

#[test]
fn delayed_claim_cannot_roll_back_current_ownership() {
    let state = PiState::default();
    claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0).expect("first claim");
    claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1).expect("second claim");

    let error = claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0)
        .expect_err("old revision must be rejected");
    assert_eq!(error.kind, "conflict");
    assert!(error.message.contains("already owns"));
    let manager = state.inner.lock().expect("Pi manager");
    assert!(manager.require_owner("owner-b").is_ok());
    assert!(manager.require_owner("owner-a").is_err());
}

#[test]
fn frontend_takeover_delivers_eof_to_every_stale_runtime() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let first = eof_process(1);
    let second = eof_process(2);
    {
        let mut manager = state.inner.lock().expect("Pi manager");
        manager.processes.insert("first".into(), first.clone());
        manager.processes.insert("second".into(), second.clone());
    }

    let outcome =
        claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1).expect("take over after EOF");

    assert_eq!(outcome.stopped.len(), 2);
    assert_clean_exit(&first);
    assert_clean_exit(&second);
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
}

#[test]
fn failed_takeover_retains_only_children_that_could_not_be_stopped() {
    let mut manager = PiManager::default();
    let responsive = eof_process(1);
    let locked = sleeping_process(2);
    manager
        .processes
        .insert("responsive".into(), responsive.clone());
    manager.processes.insert("locked".into(), locked.clone());
    let guard = locked.child.lock().expect("hold child lock");

    let (_, stopped) = stop_stale_processes(&mut manager).expect_err("locked child times out");

    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].runtime_id, "responsive");
    assert_clean_exit(&responsive);
    assert_eq!(manager.processes.len(), 1);
    assert!(manager.processes.contains_key("locked"));
    drop(guard);
    stop_stale_processes(&mut manager).expect("retry cleanup");
    assert!(manager.processes.is_empty());
}

#[test]
fn app_shutdown_delivers_eof_to_all_managed_children() {
    let state = PiState::default();
    let processes = [eof_process(1), eof_process(2)];
    {
        let mut manager = state.inner.lock().expect("Pi manager");
        for (index, process) in processes.iter().enumerate() {
            manager.processes.insert(index.to_string(), process.clone());
        }
    }

    state.shutdown();

    for process in &processes {
        assert_clean_exit(process);
    }
    let manager = state.inner.lock().expect("Pi manager");
    assert!(matches!(manager.ownership, PiOwnership::ShuttingDown));
    assert!(manager.processes.is_empty());
}

#[test]
fn app_shutdown_does_not_wait_for_an_in_flight_write_acknowledgement() {
    let state = Arc::new(PiState::default());
    claim_for_test(&state, "owner-a");
    let process = sleeping_process(1);
    let (sender, requests) = mpsc::sync_channel(1);
    *process.writer.lock().expect("writer lock") = Some(sender);
    state
        .inner
        .lock()
        .expect("Pi manager")
        .processes
        .insert("runtime-a".into(), process.clone());
    let sending_state = Arc::clone(&state);
    let sending = thread::spawn(move || {
        send_pi_to_state(
            &sending_state,
            "owner-a",
            "runtime-a".into(),
            json!({"type": "get_state"}),
        )
    });
    let pending = requests
        .recv_timeout(PI_WRITE_TIMEOUT)
        .expect("write awaiting acknowledgement");
    let started = Instant::now();

    state.shutdown();

    assert!(started.elapsed() < PI_STOP_TIMEOUT);
    assert!(process
        .child
        .lock()
        .expect("child lock")
        .try_wait()
        .expect("child status")
        .is_some());
    drop(pending);
    assert!(sending.join().expect("sending thread").is_err());
}

#[test]
fn app_shutdown_uses_one_grace_period_for_unresponsive_children() {
    let state = PiState::default();
    let processes = (0..4).map(sleeping_process).collect::<Vec<_>>();
    {
        let mut manager = state.inner.lock().expect("Pi manager");
        for (index, process) in processes.iter().enumerate() {
            manager.processes.insert(index.to_string(), process.clone());
        }
    }
    let started = Instant::now();

    state.shutdown();

    assert!(started.elapsed() >= PI_STOP_GRACE_PERIOD);
    assert!(started.elapsed() < PI_STOP_TIMEOUT);
    for process in &processes {
        let status = process
            .child
            .lock()
            .expect("child lock")
            .try_wait()
            .expect("child status")
            .expect("child reaped");
        assert!(!status.success());
    }
}

#[test]
fn stopping_one_runtime_keeps_other_processes() {
    let mut manager = PiManager::default();
    let first = eof_process(1);
    manager.processes.insert("first".into(), first.clone());
    manager
        .processes
        .insert("second".into(), sleeping_process(2));

    stop_runtime_process(&mut manager, "first").expect("stop first runtime");

    assert_clean_exit(&first);
    assert!(!manager.processes.contains_key("first"));
    let second = &manager.processes["second"];
    assert!(second.usable.load(Ordering::Acquire));
    assert!(second
        .child
        .lock()
        .expect("second child")
        .try_wait()
        .expect("second status")
        .is_none());
    let processes = manager
        .processes
        .drain()
        .map(|(_, process)| process)
        .collect();
    stop_all_processes(processes);
    assert!(manager.processes.is_empty());
}
