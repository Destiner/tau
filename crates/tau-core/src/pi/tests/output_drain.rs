use super::*;

const COMPLETED: &str = r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"completed"}]}}"#;

fn start_drain_child(
    state: &PiState,
    runtime: &str,
    script: &str,
    sink: PiSink,
    remote: bool,
) -> u64 {
    let mut command = Command::new("sh");
    command.args(["-c", script]);
    spawn_command(
        state,
        sink,
        "owner-a".into(),
        runtime.into(),
        command,
        remote,
    )
    .unwrap()
}

#[cfg(unix)]
#[test]
fn broken_pipe_reports_input_closure_not_exit_and_keeps_final_stdout() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let directory = tempfile::tempdir().unwrap();
    let gate = directory.path().join("release");
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = sender.send(event);
    }));
    let generation = start_drain_child(
        &state,
        "runtime",
        &format!(
            "exec 0<&-; printf '{{}}\\n'; while [ ! -f '{}' ]; do sleep 0.01; done; printf '%s\\n' '{COMPLETED}'",
            gate.display()
        ),
        sink,
        false,
    );
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().kind, "started");
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().kind, "rpc");
    assert!(state
        .send("wrong-owner", "runtime".into(), json!({"type":"get_state"}))
        .is_err());
    assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));
    assert!(state
        .send("owner-a", "runtime".into(), json!({"type":"get_state"}))
        .is_err());
    let event = events.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert_eq!(event.kind, "input_closed");
    assert_eq!(event.generation, generation);
    assert_eq!(event.code, None);
    assert!(matches!(events.try_recv(), Err(TryRecvError::Empty)));
    std::fs::write(gate, "go").unwrap();
    let final_record = events.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert_eq!(final_record.kind, "rpc");
    assert_eq!(final_record.line.as_deref(), Some(COMPLETED));
    let exit = events.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert_eq!(exit.kind, "exited");
    assert_eq!(exit.code, Some(0));
    assert!(state.inner.lock().unwrap().processes.is_empty());
    assert_eq!(state.stop("owner-a", "runtime").unwrap(), None);
}

#[test]
fn stop_enqueues_completed_output_after_stdin_eof_before_returning() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let (sender, events) = mpsc::sync_channel(1);
    let sink = PiSink::new(Arc::new(move |event: PiEvent| sender.send(event).unwrap()));
    let (started_sender, started) = mpsc::channel();
    // A bounded sink is deliberately drained concurrently with the Stop worker.
    let consumer = thread::spawn(move || {
        let mut output = Vec::new();
        while let Ok(event) = events.recv() {
            if event.kind == "started" {
                started_sender.send(()).unwrap();
            }
            let done = event.kind == "exited";
            output.push(event);
            if done {
                break;
            }
        }
        output
    });
    let generation = start_drain_child(
        &state,
        "runtime",
        &format!("while IFS= read -r line; do :; done; printf '%s\\n' '{COMPLETED}'; exit 0"),
        sink,
        false,
    );
    started.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert_eq!(state.stop("owner-a", "runtime").unwrap(), Some(generation));
    let output = consumer.join().unwrap();
    assert_eq!(
        output.iter().map(|e| e.kind).collect::<Vec<_>>(),
        ["started", "rpc", "exited"]
    );
    assert_eq!(output[1].line.as_deref(), Some(COMPLETED));
    assert!(output.iter().all(|e| e.generation == generation));
    assert!(state.inner.lock().unwrap().processes.is_empty());
}

#[test]
fn stop_waits_for_sink_enqueue_and_fences_concurrent_writes_start_and_claim() {
    let state = Arc::new(PiState::default());
    claim_for_test(&state, "owner-a");
    let (entered_sender, entered) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        if event.kind == "rpc" {
            entered_sender.send(()).unwrap();
            released.lock().unwrap().recv().unwrap();
        }
        sender.send(event).unwrap();
    }));
    let generation = start_drain_child(
        &state,
        "runtime",
        &format!("while IFS= read -r line; do :; done; printf '%s\\n' '{COMPLETED}'; exit 0"),
        sink.clone(),
        false,
    );
    let stopping = Arc::clone(&state);
    let (done_sender, done) = mpsc::channel();
    let worker = thread::spawn(move || {
        done_sender
            .send(stopping.stop("owner-a", "runtime"))
            .unwrap()
    });
    entered.recv_timeout(EVENT_TIMEOUT).unwrap();
    assert!(matches!(done.try_recv(), Err(TryRecvError::Empty)));
    let original = state.inner.lock().unwrap().processes["runtime"].clone();
    assert_eq!(original.generation, generation);
    assert!(!original.usable.load(Ordering::Acquire));
    assert!(state
        .send("owner-a", "runtime".into(), json!({"type":"get_state"}))
        .is_err());
    assert!(state
        .start_command(
            "owner-a".into(),
            "runtime".into(),
            Command::new("must-not-spawn"),
            sink
        )
        .is_err());
    assert!(state.claim("owner-b".into(), 1).is_err());
    // The manager remains available while the producer is blocked in the sink.
    assert_eq!(state.inner.lock().unwrap().generation, generation);
    release.send(()).unwrap();
    assert_eq!(
        done.recv_timeout(EVENT_TIMEOUT).unwrap().unwrap(),
        Some(generation)
    );
    worker.join().unwrap();
    let output = events.try_iter().collect::<Vec<_>>();
    assert_eq!(
        output.iter().map(|e| e.kind).collect::<Vec<_>>(),
        ["started", "rpc", "exited"]
    );
    assert_eq!(output[1].generation, generation);
    assert!(state.inner.lock().unwrap().processes.is_empty());
}

#[test]
fn invalid_buffered_record_after_eof_retains_cleanup_even_if_child_exits_cleanly() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| sender.send(event).unwrap()));
    let generation = start_drain_child(
        &state,
        "runtime",
        &format!(
            "while IFS= read -r line; do :; done; printf 'not-json\\n'; printf '%s\\n' '{COMPLETED}'; exit 0"
        ),
        sink,
        false,
    );
    assert!(state
        .stop("owner-a", "runtime")
        .unwrap_err()
        .contains("invalid record"));
    assert_eq!(
        state.inner.lock().unwrap().processes["runtime"].generation,
        generation
    );
    let output = events.try_iter().collect::<Vec<_>>();
    assert_eq!(
        output.iter().map(|e| e.kind).collect::<Vec<_>>(),
        ["started", "rpc", "exited"]
    );
    assert_eq!(output[1].line.as_deref(), Some(COMPLETED));
    assert!(state.stop("owner-a", "runtime").is_err());
}

#[cfg(unix)]
#[test]
fn inherited_stdout_times_out_without_detaching_cleanup_or_extending_batch_budget() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let sink = PiSink::new(Arc::new(|_: PiEvent| {}));
    for (runtime, remote) in [("local", false), ("remote", true)] {
        start_drain_child(
            &state,
            runtime,
            "while IFS= read -r line; do :; done; sleep 5 & exit 0",
            sink.clone(),
            remote,
        );
    }
    start_drain_child(&state, "remote-uncertain", "exec sleep 60", sink, true);
    let started = Instant::now();
    let failure = state.claim("owner-b".into(), 1).unwrap_err();
    assert!(failure.stopped.is_empty());
    assert!(started.elapsed() >= PI_STOP_TIMEOUT);
    assert!(started.elapsed() < PI_STOP_TIMEOUT + Duration::from_millis(500));
    let processes = state.inner.lock().unwrap().processes.clone();
    assert_eq!(processes.len(), 3);
    for process in processes.values() {
        assert!(!process.usable.load(Ordering::Acquire));
        assert!(process.child.lock().unwrap().try_wait().unwrap().is_some());
    }
    assert!(!processes["remote"].remote_uncertain.load(Ordering::Acquire));
    assert!(processes["remote-uncertain"]
        .remote_uncertain
        .load(Ordering::Acquire));
    // Join via the receipt, not inherited pipe EOF: Unix readers cancel their
    // nonblocking poll when the original Stop deadline expires.
    let outcomes = stop_processes(&processes.values().collect::<Vec<_>>());
    assert!(outcomes
        .iter()
        .zip(processes.values())
        .all(|(o, p)| o.result(p).is_err()));
}

#[cfg(unix)]
#[test]
fn acknowledged_stop_many_shares_budget_preserves_partial_receipts_and_failed_entries() {
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let sink = PiSink::new(Arc::new(|_: PiEvent| {}));
    let mut selected = Vec::new();
    for index in 0..13 {
        let runtime = format!("held-{index}");
        start_drain_child(
            &state,
            &runtime,
            "while IFS= read -r line; do :; done; sleep 6 & exit 0",
            sink.clone(),
            false,
        );
        selected.push(runtime);
    }
    let good = start_drain_child(
        &state,
        "good",
        "while IFS= read -r line; do :; done; exit 0",
        sink,
        false,
    );
    selected.extend(["good".into(), "absent".into(), "good".into()]);
    assert!(state.stop_many("wrong-owner", &selected).is_err());
    assert!(state
        .inner
        .lock()
        .unwrap()
        .processes
        .values()
        .all(|p| p.usable.load(Ordering::Acquire)));
    let started = Instant::now();
    let outcomes = state
        .stop_many("owner-a", &selected)
        .unwrap()
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    assert!(started.elapsed() >= PI_STOP_TIMEOUT);
    assert!(started.elapsed() < PI_STOP_TIMEOUT + Duration::from_millis(500));
    assert_eq!(outcomes.len(), 15);
    assert_eq!(outcomes["good"], Ok(Some(good)));
    assert_eq!(outcomes["absent"], Ok(None));
    let manager = state.inner.lock().unwrap();
    assert_eq!(manager.processes.len(), 13);
    for index in 0..13 {
        let runtime = format!("held-{index}");
        assert!(outcomes[&runtime].is_err());
        assert!(!manager.processes[&runtime].usable.load(Ordering::Acquire));
    }
}

#[test]
fn reader_io_error_is_not_a_successful_stop_receipt() {
    struct BrokenReader;
    impl std::io::Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test reader error"))
        }
    }
    let state = PiState::default();
    claim_for_test(&state, "owner-a");
    let mut process = eof_process(1);
    process.drain = Arc::new(ReaderDrain::default());
    let (sender, events) = mpsc::channel();
    process.sink = PiSink::new(Arc::new(move |event: PiEvent| sender.send(event).unwrap()));
    state
        .inner
        .lock()
        .unwrap()
        .processes
        .insert("runtime".into(), process.clone());
    let context = PiReaderContext {
        sink: process.sink.clone(),
        manager: Arc::clone(&state.inner),
        child: Arc::clone(&process.child),
        usable: Arc::clone(&process.usable),
        output_current: Arc::clone(&process.output_current),
        runtime_id: "runtime".into(),
        generation: 1,
        process,
    };
    let stderr = thread::spawn(|| {});
    spawn_stdout_reader(
        context,
        BrokenReader,
        Arc::new(Mutex::new(Default::default())),
        stderr,
    );
    assert!(state
        .stop("owner-a", "runtime")
        .unwrap_err()
        .contains("reader failed"));
    assert_eq!(
        events.try_iter().map(|e| e.kind).collect::<Vec<_>>(),
        ["error", "exited"]
    );
    assert!(state
        .inner
        .lock()
        .unwrap()
        .processes
        .contains_key("runtime"));
}
