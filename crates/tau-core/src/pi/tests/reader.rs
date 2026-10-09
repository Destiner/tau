use super::*;

#[cfg(unix)]
#[test]
fn stderr_events_and_tail_are_bounded_before_process_exit() {
    let state = PiState::default();
    state.claim("owner-a".into(), 0).unwrap();
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = sender.send(event);
    }));
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "printf '%10000s\\n' x >&2; while IFS= read -r line; do :; done",
    ]);
    state
        .start_command("owner-a".into(), "runtime".into(), command, sink)
        .unwrap();
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().kind, "started");
    let event = events.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert_eq!(event.kind, "stderr");
    let message = event.message.unwrap();
    assert_eq!(message.chars().count(), 2049);
    assert!(message.ends_with('…'));
    state.stop("owner-a", "runtime").unwrap();
}

#[cfg(unix)]
#[test]
fn retired_start_cannot_emit_a_late_started_event() {
    let state = Arc::new(PiState::default());
    state.claim("owner-a".into(), 0).unwrap();
    let (sender, events) = mpsc::channel();
    let (starting, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = sender.send(event);
    }))
    .with_diagnostics(Arc::new(
        move |_: &str, _: Option<u64>, diagnostic: PiDiagnostic| {
            if matches!(diagnostic, PiDiagnostic::Started) {
                starting.send(()).unwrap();
                released
                    .lock()
                    .unwrap()
                    .recv_timeout(EVENT_TIMEOUT)
                    .unwrap();
            }
        },
    ));
    let starting_state = Arc::clone(&state);
    let starter = thread::spawn(move || {
        let mut command = Command::new("sh");
        command.args(["-c", "cat >/dev/null"]);
        starting_state.start_command("owner-a".into(), "runtime".into(), command, sink)
    });
    started.recv_timeout(EVENT_TIMEOUT).unwrap();
    state.stop("owner-a", "runtime").unwrap();
    release.send(()).unwrap();
    starter.join().unwrap().unwrap();
    assert!(!events.try_iter().any(|event| event.kind == "started"));
}

#[cfg(unix)]
#[test]
fn failed_stdin_does_not_discard_final_rpc_output() {
    let root = tempfile::tempdir().unwrap();
    let release = root.path().join("release");
    let state = PiState::default();
    state.claim("owner-a".into(), 0).unwrap();
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = sender.send(event);
    }));
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "exec 0<&-; printf '{\"type\":\"ready\"}\\n'; while [ ! -f \"$1\" ]; do sleep 0.01; done; printf '{\"type\":\"final\"}\\n'",
        "tau-test",
    ]);
    command.arg(&release);
    state
        .start_command("owner-a".into(), "runtime".into(), command, sink)
        .unwrap();
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().kind, "started");
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().kind, "rpc");
    assert!(state
        .send(
            "owner-a",
            "runtime".into(),
            serde_json::json!({"type": "test"})
        )
        .is_err());
    std::fs::write(release, b"release").unwrap();
    loop {
        let event = events.recv_timeout(EVENT_TIMEOUT).unwrap();
        if event.kind == "rpc" {
            assert_eq!(event.line.as_deref(), Some("{\"type\":\"final\"}"));
            break;
        }
    }
}

#[cfg(unix)]
#[test]
fn stopped_readers_drain_final_output_without_blocking_graceful_exit() {
    let state = PiState::default();
    state.claim("owner-a".into(), 0).unwrap();
    let sink = PiSink::new(Arc::new(|_: PiEvent| {}));
    let mut command = Command::new("sh");
    // More than a pipe buffer of final output after stdin closes. Cleanup holds
    // the manager lock, so a reader requiring that lock would force a kill.
    command.args(["-c", "while IFS= read -r line; do :; done; i=0; while [ $i -lt 10000 ]; do printf '{\"type\":\"final\"}\\n'; i=$((i+1)); done; exit 0"]);
    state
        .start_command("owner-a".into(), "runtime".into(), command, sink)
        .unwrap();
    let process = state.inner.lock().unwrap().processes["runtime"].clone();
    state.stop("owner-a", "runtime").unwrap();
    assert_clean_exit(&process);
}
