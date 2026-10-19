use super::*;

#[test]
fn public_claim_is_idempotent_and_revisions_fence_delayed_owners() {
    let state = PiState::default();
    assert_eq!(state.read_revision().unwrap(), 0);
    assert_eq!(state.claim("".into(), 0).unwrap_err().kind, "retryable");
    assert!(!state.claim("first".into(), 0).unwrap().replacement);
    assert_eq!(state.read_revision().unwrap(), 1);
    state.claim("first".into(), 0).unwrap();
    assert_eq!(state.read_revision().unwrap(), 1);
    assert!(state.claim("second".into(), 1).unwrap().replacement);
    assert_eq!(state.claim("first".into(), 0).unwrap_err().kind, "conflict");
    state.shutdown();
    assert!(state.read_revision().is_err());
    assert!(state.claim("second".into(), 2).is_err());
}

#[test]
fn dropping_a_claim_handle_does_not_shutdown_the_manager() {
    let state = PiState::default();
    state.clone_handle().claim("owner-a".into(), 0).unwrap();
    assert_eq!(state.read_revision().unwrap(), 1);
    assert!(state.stop("owner-a", "not-running").unwrap().is_none());
}

#[test]
fn local_start_uses_the_current_isolated_profile_session_directory() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    crate::profile::initialize_test_profile();
    let fixture = NativePiFixture::new(Some(OsStr::new("malformed-stdout")));
    let directory = crate::storage::default_session_dir(&fixture.project_string()).unwrap();
    let _environment =
        EnvironmentGuard::set(&[("TAU_PI_TEST_SESSION_DIR", Some(directory.as_os_str()))]);
    let (state, sink, events) = fixture.state_and_events();
    state.claim("owner-a".into(), 0).unwrap();
    let generation = state
        .start_local(
            "owner-a".into(),
            "isolated".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
            sink,
        )
        .unwrap();
    expect_outer_event(&events, "started", generation);
    let exit = expect_outer_event(&events, "exited", generation);
    assert_eq!(exit["code"], 0);
    assert!(directory.starts_with(
        crate::profile::current()
            .unwrap()
            .temporary_root()
            .unwrap()
            .canonicalize()
            .unwrap()
    ));
}

#[test]
fn bounded_line_reader_drains_oversized_lines_and_recovers() {
    let input = b"123456789\n{}\n";
    let mut reader = std::io::BufReader::with_capacity(3, &input[..]);
    let mut bytes = Vec::new();
    assert_eq!(
        read_bounded_line(&mut reader, &mut bytes, 4).unwrap(),
        (true, true)
    );
    assert_eq!(bytes, b"1234");
    assert_eq!(
        read_bounded_line(&mut reader, &mut bytes, 4).unwrap(),
        (true, false)
    );
    assert_eq!(bytes, b"{}\n");
    assert_eq!(
        read_bounded_line(&mut reader, &mut bytes, 4).unwrap(),
        (false, false)
    );
}

#[test]
fn stale_generations_cannot_emit_rpc_or_stderr() {
    let state = PiState::default();
    let (sender, events) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        sender.send(event).unwrap();
    }));
    let retired = eof_process(1);
    retired.close_stdin();
    assert!(!retired.usable.load(Ordering::Acquire));
    assert!(retired.output_current.load(Ordering::Acquire));
    // Closing input is not retirement: already-produced output must drain.
    emit_current_event(
        &retired.output_current,
        &sink,
        PiEvent {
            runtime_id: "runtime".into(),
            generation: 1,
            kind: "rpc",
            line: Some("{}".into()),
            message: None,
            code: None,
        },
    );
    assert_eq!(events.recv_timeout(EVENT_TIMEOUT).unwrap().generation, 1);
    // The reader/confirmed retirement owns this output fence, not close_stdin.
    retired.output_current.store(false, Ordering::Release);
    let process = eof_process(2);
    state
        .inner
        .lock()
        .unwrap()
        .processes
        .insert("runtime".into(), process);
    for kind in ["rpc", "stderr"] {
        emit_current_event(
            &retired.output_current,
            &sink,
            PiEvent {
                runtime_id: "runtime".into(),
                generation: 1,
                kind,
                line: Some("{}".into()),
                message: None,
                code: None,
            },
        );
    }
    assert_eq!(events.try_recv().unwrap_err(), TryRecvError::Empty);
    state.shutdown();
    stop_process(&retired).unwrap();
}

#[cfg(unix)]
#[test]
fn subprocess_diagnostics_report_categories_not_process_content() {
    let state = PiState::default();
    state.claim("owner-a".into(), 0).unwrap();
    let (event_sender, events) = mpsc::channel();
    let (diagnostic_sender, diagnostics) = mpsc::channel();
    let sink = PiSink::new(Arc::new(move |event: PiEvent| {
        let _ = event_sender.send(event);
    }))
    .with_diagnostics(Arc::new(
        move |_: &str, _: Option<u64>, diagnostic: PiDiagnostic| {
            let _ = diagnostic_sender.send(diagnostic);
        },
    ));
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "printf 'secret-invalid-json\\n'; printf 'secret-stderr\\n' >&2; exit 7",
    ]);
    state
        .start_command("owner-a".into(), "runtime".into(), command, sink)
        .unwrap();
    loop {
        let event = events.recv_timeout(EVENT_TIMEOUT).unwrap();
        if event.kind == "exited" {
            assert_eq!(event.code, Some(7));
            break;
        }
    }
    let diagnostics = diagnostics.try_iter().collect::<Vec<_>>();
    assert!(diagnostics.iter().any(|d| matches!(
        d,
        PiDiagnostic::LineDropped {
            reason: "malformed"
        }
    )));
    assert!(diagnostics
        .iter()
        .any(|d| matches!(d, PiDiagnostic::Exited { code: Some(7) })));
    assert!(!format!("{diagnostics:?}").contains("secret"));
}
