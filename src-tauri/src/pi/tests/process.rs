use super::*;

#[test]
fn replacing_a_runtime_waits_for_eof_before_starting_its_successor() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = NativePiFixture::new(None);
    let (app, _events) = fixture.app_and_events();
    let handle = app.handle().clone();
    let state = handle.state::<PiState>();
    let telemetry = handle.state::<Telemetry>();
    claim_for_test(&state, "owner-a");
    let start = || {
        let mut command = Command::new("sh");
        command.args(["-c", "while IFS= read -r line; do :; done; exit 0"]);
        spawn_command(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-a".into(),
            "runtime-a".into(),
            command,
        )
        .expect("start EOF-aware runtime")
    };
    let first_generation = start();
    let first = state.inner.lock().expect("Pi manager").processes["runtime-a"].clone();

    let second_generation = start();

    assert!(second_generation > first_generation);
    assert_clean_exit(&first);
    let second = state.inner.lock().expect("Pi manager").processes["runtime-a"].clone();
    assert!(second.usable.load(Ordering::Acquire));
    state.shutdown();
    assert_clean_exit(&second);
}

#[test]
fn stopping_closes_stdin_even_with_retained_process_handles() {
    let process = eof_process(1);
    let retained = process.clone();
    send_pi_line(&process, b"request\n".to_vec(), PI_WRITE_TIMEOUT).expect("write before stop");

    stop_process(&process).expect("stop on EOF");

    assert_clean_exit(&retained);
    assert!(send_pi_line(&retained, b"late\n".to_vec(), PI_WRITE_TIMEOUT).is_err());
    stop_process(&retained).expect("repeated stop");
}

#[test]
fn a_blocked_pi_write_returns_within_its_timeout() {
    let process = sleeping_process(1);
    let started = Instant::now();

    let error = send_pi_line(
        &process,
        vec![b'x'; 1024 * 1024],
        Duration::from_millis(100),
    )
    .expect_err("an unread pipe should time out");

    assert_eq!(error, "Pi did not accept the request in time. Try again.");
    assert!(started.elapsed() < PI_STOP_TIMEOUT);
    assert!(process.writer.lock().expect("writer lock").is_none());
    assert_eq!(
        send_pi_line(&process, b"retry\n".to_vec(), Duration::from_millis(100))
            .expect_err("a timed-out runtime stays unavailable"),
        "Pi is no longer accepting requests. Restart Tau and try again.",
    );
    stop_process(&process).expect("timed-out process is stopped");
}

#[test]
fn waiting_for_exit_does_not_block_process_termination() {
    let process = sleeping_process(1);
    let child = Arc::clone(&process.child);
    let waiter = thread::spawn(move || wait_for_child(&child));
    thread::sleep(Duration::from_millis(25));
    let started = Instant::now();

    stop_process(&process).expect("stop process while its exit is watched");

    assert!(started.elapsed() < PI_STOP_TIMEOUT);
    assert!(waiter.join().expect("exit watcher").is_some());
}
