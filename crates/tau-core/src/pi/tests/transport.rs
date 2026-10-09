use super::*;

#[test]
fn fake_pi_round_trips_jsonl_and_cleans_up_after_successful_exit() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = NativePiFixture::new(None);
    let (state, sink, events) = fixture.state_and_events();
    claim_for_test(&state, "owner-a");
    let generation = fixture
        .start(&state, sink, "owner-a", "native-main")
        .expect("start fake Pi");
    assert_eq!(generation, 1);
    let started = expect_outer_event(&events, "started", generation);
    assert_eq!(started["runtimeId"], "native-main");

    for (id, request_type) in [
        ("models", "get_available_models"),
        ("commands", "get_commands"),
        ("state", "get_state"),
        ("messages", "get_messages"),
        ("efforts", "get_available_thinking_levels"),
    ] {
        send_pi_to_state(
            &state,
            "owner-a",
            "native-main".into(),
            json!({
                "id": id,
                "type": request_type,
            }),
        )
        .expect("send bootstrap request");
        if id == "efforts" {
            expect_rpc(&events, generation, "response", Some("efforts"));
            expect_rpc(&events, generation, "response", Some("messages"));
        } else if id != "messages" {
            expect_rpc(&events, generation, "response", Some(id));
        }
    }

    send_pi_to_state(
        &state,
        "owner-a",
        "native-main".into(),
        json!({"id": "prompt", "type": "prompt", "message": "Explain the fixture"}),
    )
    .expect("send prompt");
    expect_rpc(&events, generation, "agent_start", None);

    send_pi_to_state(
        &state,
        "owner-a",
        "native-main".into(),
        json!({"id": "run-state", "type": "get_state"}),
    )
    .expect("send running state request");
    expect_rpc(&events, generation, "response", Some("run-state"));
    expect_rpc(&events, generation, "message_update", None);
    expect_rpc(&events, generation, "message_update", None);
    expect_rpc(&events, generation, "response", Some("prompt"));
    expect_rpc(&events, generation, "agent_settled", None);

    for (id, request_type) in [
        ("settled-state", "get_state"),
        ("settled-messages", "get_messages"),
        ("settled-efforts", "get_available_thinking_levels"),
    ] {
        send_pi_to_state(
            &state,
            "owner-a",
            "native-main".into(),
            json!({
                "id": id,
                "type": request_type,
            }),
        )
        .expect("send settlement request");
        if id == "settled-efforts" {
            expect_rpc(&events, generation, "response", Some(id));
            expect_rpc(&events, generation, "response", Some("settled-messages"));
        } else if id != "settled-messages" {
            expect_rpc(&events, generation, "response", Some(id));
        }
    }

    let exited = expect_outer_event(&events, "exited", generation);
    assert_eq!(exited["runtimeId"], "native-main");
    assert_eq!(exited["code"], 0);
    assert!(exited.get("message").is_none());
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
}

#[test]
fn fake_pi_malformed_stdout_is_dropped_and_lifecycle_is_cleaned_up() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = NativePiFixture::new(Some(OsStr::new("malformed-stdout")));
    let (state, sink, events) = fixture.state_and_events();
    claim_for_test(&state, "owner-a");
    let generation = fixture
        .start(&state, sink, "owner-a", "native-malformed")
        .expect("start fake Pi");
    let started = expect_outer_event(&events, "started", generation);
    assert_eq!(started["runtimeId"], "native-malformed");
    let exited = expect_outer_event(&events, "exited", generation);
    assert_eq!(exited["runtimeId"], "native-malformed");
    assert_eq!(exited["code"], 0);
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
    assert!(matches!(
        events.try_recv(),
        Err(TryRecvError::Empty | TryRecvError::Disconnected)
    ));
}

#[test]
fn fake_pi_nonzero_exit_forwards_bounded_failure_and_cleans_up() {
    let _lock = ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let fixture = NativePiFixture::new(Some(OsStr::new("nonzero-exit")));
    let (state, sink, events) = fixture.state_and_events();
    claim_for_test(&state, "owner-a");
    let generation = fixture
        .start(&state, sink, "owner-a", "native-failure")
        .expect("start fake Pi");
    let started = expect_outer_event(&events, "started", generation);
    assert_eq!(started["runtimeId"], "native-failure");
    let next = events.recv_timeout(EVENT_TIMEOUT).expect("failure event");
    assert_eq!(next["generation"], generation);
    let exited = if next["kind"] == "stderr" {
        assert_eq!(next["message"], "scripted transport failure");
        expect_outer_event(&events, "exited", generation)
    } else {
        next
    };
    assert_eq!(exited["kind"], "exited");
    assert_eq!(exited["runtimeId"], "native-failure");
    assert_eq!(exited["code"], 23);
    // The stderr reader may emit its event after the exit reader snapshots
    // the best-effort tail, even when the event arrives first on this channel.
    assert!(exited["message"]
        .as_str()
        .is_some_and(|message| message.starts_with("Pi exited with status 23.")));
    assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
}

#[test]
fn exit_message_includes_status_and_stderr() {
    assert_eq!(
        pi_exit_message(Some(127), "env: node: No such file or directory"),
        "Pi exited with status 127. env: node: No such file or directory",
    );

    {
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::BrokenPipe),
            "broken_pipe"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::Interrupted),
            "interrupted"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::UnexpectedEof),
            "unexpected_eof"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::PermissionDenied),
            "other"
        );
    }

    {
        assert_eq!(line_drop_reason(br#"{"type":"response"}"#), None);
        assert_eq!(line_drop_reason(b"not json"), Some("malformed"));
        assert_eq!(
            line_drop_reason(br#"["not", "an", "object"]"#),
            Some("malformed")
        );
        assert_eq!(line_drop_reason(&[0xff]), Some("invalid_utf8"));
        assert_eq!(
            line_drop_reason(&vec![b'x'; MAX_RPC_LINE_BYTES + 1]),
            Some("oversized")
        );
    }
}
