use super::*;

fn remote_script(script: &str, generation: u64) -> PiProcess {
    let mut process = scripted_process(script, generation);
    process.remote = true;
    process
}

#[test]
fn natural_ssh_command_statuses_confirm_exit_except_255() {
    // OpenSSH's documented contract: remote command status or 255 on SSH error.
    // These are deterministic local stand-ins, not authentication/network tests.
    for code in [0, 1, 42, 254, 255] {
        let process = remote_script(
            &format!("while IFS= read -r line; do :; done; exit {code}"),
            code as u64 + 1,
        );
        let outcome = stop_processes(&[&process]).remove(0);
        assert_eq!(outcome.natural_exit_status.unwrap().code(), Some(code));
        assert!(!outcome.kill_sent);
        assert_eq!(outcome.result(&process).is_ok(), code != 255);
        assert_eq!(
            process.remote_uncertain.load(Ordering::Acquire),
            code == 255
        );
        assert_eq!(stop_process(&process).is_ok(), code != 255);
    }
}

#[test]
fn local_nonzero_exit_is_confirmed_once_reaped() {
    let process = scripted_process("while IFS= read -r line; do :; done; exit 255", 1);
    stop_process(&process).unwrap();
    assert!(!process.remote_uncertain.load(Ordering::Acquire));
}

#[cfg(unix)]
#[test]
fn graceful_remote_reader_cleanup_allows_replacement() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let sink = PiSink::new(Arc::new(|_: PiEvent| {}));
    let mut command = Command::new("sh");
    command.args(["-c", "while IFS= read -r line; do :; done; exit 0"]);
    let generation = spawn_command(
        &state,
        sink.clone(),
        "owner-a".into(),
        "runtime".into(),
        command,
        true,
    )
    .unwrap();
    assert_eq!(state.stop("owner-a", "runtime").unwrap(), Some(generation));
    assert!(state.inner.lock().unwrap().processes.is_empty());
    let mut replacement = Command::new("sh");
    replacement.args(["-c", "while IFS= read -r line; do :; done; exit 0"]);
    assert!(
        state
            .start_command("owner-a".into(), "runtime".into(), replacement, sink)
            .unwrap()
            > generation
    );
    state.stop("owner-a", "runtime").unwrap();
}

#[cfg(unix)]
#[test]
fn ssh_signal_exit_is_not_remote_exit_confirmation() {
    let process = remote_script("while IFS= read -r line; do :; done; kill -TERM $$", 1);
    let outcome = stop_processes(&[&process]).remove(0);
    assert_eq!(outcome.natural_exit_status.unwrap().code(), None);
    assert!(!outcome.kill_sent);
    assert!(outcome.result(&process).is_err());
    assert!(stop_process(&process).is_err());
}

#[test]
fn mixed_cleanup_shares_deadline_and_retains_only_uncertain_remote() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let local = sleeping_process(1);
    let remote_ok = remote_script("while IFS= read -r line; do :; done; exit 0", 2);
    let remote_killed = remote_script("exec sleep 60", 3);
    let remote_error = remote_script("while IFS= read -r line; do :; done; exit 255", 4);
    {
        let mut manager = state.inner.lock().unwrap();
        for (id, process) in [
            ("local", &local),
            ("remote-ok", &remote_ok),
            ("remote-killed", &remote_killed),
            ("remote-error", &remote_error),
        ] {
            manager.processes.insert(id.into(), process.clone());
        }
    }
    let started = Instant::now();
    let failure = state.claim("owner-b".into(), 1).unwrap_err();
    assert!(started.elapsed() >= PI_STOP_GRACE_PERIOD);
    assert!(started.elapsed() < PI_STOP_TIMEOUT);
    assert_eq!(failure.stopped.len(), 2);
    assert!(failure
        .stopped
        .iter()
        .any(|item| item.runtime_id == "local"));
    assert!(failure
        .stopped
        .iter()
        .any(|item| item.runtime_id == "remote-ok"));
    assert!(remote_killed.kill_attempted.load(Ordering::Acquire));
    assert!(remote_killed
        .child
        .lock()
        .unwrap()
        .try_wait()
        .unwrap()
        .is_some());
    for process in [&remote_killed, &remote_error] {
        assert!(process.remote_uncertain.load(Ordering::Acquire));
        assert!(!process.usable.load(Ordering::Acquire));
    }
    for _ in 0..2 {
        let failure = state.claim("owner-b".into(), 1).unwrap_err();
        assert!(failure.stopped.is_empty());
        assert!(failure.message.contains("unconfirmed"));
    }
    let manager = state.inner.lock().unwrap();
    assert_eq!(manager.processes.len(), 2);
    assert!(matches!(manager.ownership, PiOwnership::Claiming { .. }));
}

#[test]
fn killed_ssh_transport_cannot_be_laundered_by_stop_start_or_shutdown() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let remote = remote_script("exec sleep 60", 1);
    state
        .inner
        .lock()
        .unwrap()
        .processes
        .insert("runtime".into(), remote.clone());
    assert!(state
        .stop("owner-a", "runtime")
        .unwrap_err()
        .contains("unconfirmed"));
    assert!(state.stop("owner-a", "runtime").is_err());
    assert!(state
        .send("owner-a", "runtime".into(), json!({"type": "get_state"}))
        .unwrap_err()
        .contains("unconfirmed"));
    let generation = state.inner.lock().unwrap().generation;
    let command = Command::new("this-command-must-never-be-spawned");
    assert!(state
        .start_command(
            "owner-a".into(),
            "runtime".into(),
            command,
            PiSink::new(Arc::new(|_: PiEvent| {})),
        )
        .unwrap_err()
        .contains("unconfirmed"));
    assert_eq!(state.inner.lock().unwrap().generation, generation);
    state.shutdown();
    state.shutdown();
    assert!(state
        .inner
        .lock()
        .unwrap()
        .processes
        .contains_key("runtime"));
}

#[test]
fn remote_timeout_with_no_status_remains_uncertain_after_later_natural_exit() {
    let process = remote_script("while IFS= read -r line; do :; done; exit 0", 1);
    let guard = process.child.lock().unwrap();
    let outcome = stop_processes(&[&process]).remove(0);
    assert!(outcome.reaped_status.is_none());
    assert!(!outcome.kill_sent);
    assert!(outcome.result(&process).is_err());
    drop(guard);
    assert_eq!(wait_for_child(&process.child).unwrap().code(), Some(0));
    assert!(stop_process(&process).is_err());
}

#[cfg(unix)]
#[test]
fn unexpected_remote_reader_exit_retains_uncertain_ownership() {
    for code in [0, 1, 255] {
        let state = PiState::default();
        claim_for_test(&state, "owner-a");
        let (sender, events) = mpsc::channel();
        let sink = PiSink::new(Arc::new(move |event: PiEvent| {
            let _ = sender.send(event);
        }));
        let mut command = Command::new("sh");
        command.args(["-c", &format!("exit {code}")]);
        spawn_command(
            &state,
            sink,
            "owner-a".into(),
            "runtime".into(),
            command,
            true,
        )
        .unwrap();
        loop {
            let event = events.recv_timeout(EVENT_TIMEOUT).unwrap();
            if event.kind == "exited" {
                assert_eq!(event.code, Some(code));
                if code == 255 {
                    assert!(event.message.unwrap().contains("unconfirmed"));
                }
                break;
            }
        }
        let process = state
            .inner
            .lock()
            .unwrap()
            .processes
            .get("runtime")
            .cloned();
        if let Some(process) = process {
            process.drain.wait(Instant::now() + EVENT_TIMEOUT).unwrap();
        }
        assert_eq!(
            state
                .inner
                .lock()
                .unwrap()
                .processes
                .contains_key("runtime"),
            code == 255
        );
        assert_eq!(state.stop("owner-a", "runtime").is_err(), code == 255);
    }
}

#[cfg(unix)]
#[test]
fn remote_reader_during_forced_stop_does_not_drop_retained_owner() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = sender.send(event);
    }));
    let mut command = Command::new("sh");
    command.args(["-c", "exec sleep 60"]);
    spawn_command(
        &state,
        sink,
        "owner-a".into(),
        "runtime".into(),
        command,
        true,
    )
    .unwrap();
    assert!(state.stop("owner-a", "runtime").is_err());
    loop {
        if events.recv_timeout(EVENT_TIMEOUT).unwrap().kind == "exited" {
            break;
        }
    }
    assert!(state
        .inner
        .lock()
        .unwrap()
        .processes
        .contains_key("runtime"));
    assert!(state.stop("owner-a", "runtime").is_err());
}
