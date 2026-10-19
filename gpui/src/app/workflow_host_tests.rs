use super::tests::{create, rpc, started, with_shell};
use super::*;
use serde_json::{Value, json};

fn reads(
    s: &mut Shell,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
    messages: Value,
) -> Vec<Command> {
    reads_identity(s, cx, requests, messages, "")
}

fn reads_identity(
    s: &mut Shell,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
    messages: Value,
    identity: &str,
) -> Vec<Command> {
    let mut other = vec![];
    for _ in 0..128 {
        let Ok(command) = requests.try_recv() else {
            break;
        };
        if let Command::Send {
            runtime,
            generation,
            request,
        } = command
        {
            assert_ne!(request["type"], "prompt", "must never resend a command");
            let data = match request["type"].as_str().unwrap() {
                "get_state" => {
                    json!({"sessionId":identity, "sessionFile":if identity.is_empty() { String::new() } else { format!("/fixture/{identity}.jsonl") }, "isStreaming":false})
                }
                "get_messages" => json!({"messages":messages}),
                "get_commands" => json!({"commands":[{"name":"implement", "source":"extension"}]}),
                _ => json!({"models":[], "levels":[]}),
            };
            s.ingest(rpc(&runtime, generation, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
        } else {
            other.push(command);
        }
    }
    other
}

fn ready(
    s: &mut Shell,
    w: &mut Window,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
) {
    create(s, "a", "", w, cx);
    started(s, "a", 7, cx);
    reads(s, cx, requests, json!([]));
    assert_eq!(s.sessions["a"].controller.pending_count(), 0);
}

fn submit(
    s: &mut Shell,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
) -> Value {
    let effects = s
        .sessions
        .get_mut("a")
        .unwrap()
        .controller
        .submit("/implement");
    s.effects("a", effects, cx);
    std::iter::from_fn(|| requests.try_recv().ok())
        .find_map(|command| match command {
            Command::Send { request, .. } if request["type"] == "prompt" => Some(request),
            _ => None,
        })
        .expect("actual dispatched prompt")
}

fn failed_notice(s: &mut Shell, cx: &mut Context<Shell>) {
    s.ingest(rpc("a", 7, json!({"type":"extension_ui_request", "id":"failure", "method":"notify", "notifyType":"error", "message":"git is not a repository"})), cx);
    s.ingest(
        rpc(
            "a",
            7,
            json!({"type":"extension_error", "error":"git is not a repository"}),
        ),
        cx,
    );
}

fn handled(s: &mut Shell, prompt: &Value, cx: &mut Context<Shell>) {
    s.ingest(rpc("a", 7, json!({"type":"response", "id":prompt["id"], "command":"prompt", "success":true, "data":{"disposition":"handled"}})), cx);
}

#[test]
fn workflow_host_admission_outlives_deadline_before_delayed_written() {
    with_shell(512, |s, w, cx, requests| {
        ready(s, w, cx, &requests);
        let prompt = submit(s, cx, &requests);
        for now in [20_000, 600_001] {
            let effects = s.sessions.get_mut("a").unwrap().controller.tick(now);
            s.effects("a", effects, cx);
            assert!(
                s.sessions["a"].controller.state().error.is_none(),
                "false confirmation timeout at {now}"
            );
            assert!(s.sessions["a"].controller.submission_pending());
            s.sync(w, cx);
            assert!(s.fallback.is_none());
        }
        s.ingest(
            Event::Written {
                runtime: "a".into(),
                generation: 6,
                request: prompt.clone(),
                success: false,
            },
            cx,
        );
        s.ingest(
            Event::Written {
                runtime: "a".into(),
                generation: 7,
                request: json!({"type":"prompt", "id":"stale-id"}),
                success: true,
            },
            cx,
        );
        assert!(s.sessions["a"].controller.submission_pending());
        assert!(s.sessions["a"].controller.state().error.is_none());
        for _ in 0..2 {
            s.ingest(
                Event::Written {
                    runtime: "a".into(),
                    generation: 7,
                    request: prompt.clone(),
                    success: true,
                },
                cx,
            );
        }
        handled(s, &prompt, cx);
        reads(s, cx, &requests, json!([]));
        assert!(!s.sessions["a"].controller.submission_pending());
        assert!(
            s.sessions["a"]
                .controller
                .state()
                .recovered_drafts
                .is_empty()
        );
    });
}

#[test]
fn workflow_host_error_only_finished_command_retires_after_correlated_reads() {
    with_shell(512, |s, w, cx, requests| {
        ready(s, w, cx, &requests);
        let prompt = submit(s, cx, &requests);
        failed_notice(s, cx);
        handled(s, &prompt, cx);
        assert!(s.sessions["a"].controller.state().error.is_some());
        create(s, "b", "keep", w, cx);
        assert!(s.sessions.contains_key("a"));
        assert!(
            s.empty_candidates.contains("a"),
            "notice must not erase selection intent during reconciliation"
        );
        reads(s, cx, &requests, json!([]));
        assert!(
            s.sessions.contains_key("a"),
            "replacement probes still own the runtime"
        );
        let effects = s.sessions.get_mut("a").unwrap().controller.tick(5_000);
        s.effects("a", effects, cx);
        let cleanup = reads(s, cx, &requests, json!([]));
        assert!(!s.sessions.contains_key("a"));
        assert!(cleanup.iter().any(
            |c| matches!(c, Command::DisposeEmpty { runtime, generation:7, .. } if runtime == "a")
        ));
        assert_eq!(s.active, "b");
    });
}

#[test]
fn workflow_host_cancel_acknowledges_only_displayed_incident_and_reselect_does_not_replay() {
    let mut app = tests::app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let (old_id, old) = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            ready(s, w, cx, &requests);
            let prompt = submit(s, cx, &requests);
            failed_notice(s, cx);
            handled(s, &prompt, cx);
            reads(
                s,
                cx,
                &requests,
                json!([{"role":"user", "content":"meaningful saved history"}]),
            );
            s.sync(w, cx);
            s.fallback.clone().expect("error fallback")
        })
    });
    root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    let current = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            assert!(s.sessions["a"].dirty);
            assert!(s.sessions["a"].projection_dirty);
            assert_ne!(s.sessions["a"].controller.state().status, Status::Error);
            s.sync(w, cx);
            assert!(s.sessions["a"].controller.state().error.is_none());
            assert!(s.fallback.is_none());
            create(s, "b", "keep", w, cx);
            s.select("a", w, cx);
            s.sync(w, cx);
            assert!(
                s.fallback.is_none(),
                "acknowledged error must not replay on selection"
            );
            assert!(!s.sessions["a"].controller.state().transcript.is_empty());
            failed_notice(s, cx);
            s.sync(w, cx);
            let (new_id, current) = s.fallback.clone().expect("identical new incident");
            assert_ne!(old_id, new_id);
            current
        })
    });
    root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    root.update(|_, _, cx| {
        assert!(
            shell.read(cx).sessions["a"]
                .controller
                .state()
                .error
                .is_some()
        )
    });
    // A new identical occurrence can arrive before the next render replaces the dialog.
    root.update(|_, _, cx| shell.update(cx, failed_notice));
    root.update(|_, _, cx| current.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    let latest = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            assert!(
                s.sessions["a"].controller.state().error.is_some(),
                "stale displayed token must not acknowledge a newer same-text incident"
            );
            s.sync(w, cx);
            let latest = s.fallback.as_ref().unwrap().1.clone();
            assert_ne!(latest, current);
            latest
        })
    });
    root.update(|_, _, cx| latest.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    root.update(|_, _, cx| {
        assert!(
            shell.read(cx).sessions["a"]
                .controller
                .state()
                .error
                .is_none()
        )
    });
    root.update(|_, w, cx| w.blur(cx));
    app.run_until_parked();
}

#[test]
fn workflow_host_fallback_refresh_keeps_read_ownership_and_stale_generation_dialog_is_inert() {
    let mut app = tests::app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let old = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            ready(s, w, cx, &requests);
            failed_notice(s, cx);
            s.sync(w, cx);
            s.fallback.as_ref().unwrap().1.clone()
        })
    });
    root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Submitted(String::new()))));
    root.update(|_, _, cx| {
        shell.update(cx, |s, cx| {
            assert!(s.sessions["a"].controller.pending_count() > 0);
            reads(s, cx, &requests, json!([]));
            assert!(
                s.sessions["a"].controller.state().error.is_none(),
                "retain the existing Refresh recovery semantics"
            );
            let effects = s
                .sessions
                .get_mut("a")
                .unwrap()
                .controller
                .begin_generation(8);
            s.effects("a", effects, cx);
            reads(s, cx, &requests, json!([]));
            s.ingest(
                rpc(
                    "a",
                    8,
                    json!({"type":"extension_error", "error":"same failure"}),
                ),
                cx,
            );
        })
    });
    // Deliver before sync, while the old dialog still has presentation ownership.
    root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    let current = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            assert!(s.sessions["a"].controller.state().error.is_some());
            s.sync(w, cx);
            let current = s.fallback.as_ref().unwrap().1.clone();
            assert_ne!(old, current);
            current
        })
    });
    root.update(|_, _, cx| current.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
    root.update(|_, _, cx| {
        assert!(
            shell.read(cx).sessions["a"]
                .controller
                .state()
                .error
                .is_none()
        )
    });
    root.update(|_, w, cx| w.blur(cx));
    app.run_until_parked();
}

#[test]
fn workflow_host_transport_write_failure_and_frame_close_fail_without_resending() {
    for close in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            ready(s, w, cx, &requests);
            let prompt = submit(s, cx, &requests);
            let effects = s.sessions.get_mut("a").unwrap().controller.tick(20_000);
            s.effects("a", effects, cx);
            assert!(s.sessions["a"].controller.state().error.is_none());
            if close {
                let Event::Pi(mut event) = rpc("a", 7, json!({})) else {
                    unreachable!()
                };
                event.kind = "input_closed";
                event.line = None;
                s.ingest(Event::Pi(event.clone()), cx);
                assert!(s.sessions["a"].controller.submission_pending());
                assert!(!s.sessions["a"].controller.state().ready);
                event.kind = "exited";
                s.ingest(Event::Pi(event), cx);
            } else {
                s.ingest(
                    Event::Written {
                        runtime: "a".into(),
                        generation: 7,
                        request: prompt.clone(),
                        success: false,
                    },
                    cx,
                );
            }
            let live = &s.sessions["a"];
            assert!(live.controller.state().error.is_some());
            assert!(live.controller.state().draft.contains("/implement"));
            let token = live.controller.error_token().unwrap();
            assert!(
                s.sessions
                    .get_mut("a")
                    .unwrap()
                    .controller
                    .acknowledge_error(&token)
            );
            if close {
                assert!(!s.sessions["a"].controller.can_release_runtime());
            }
            create(s, "b", "keep", w, cx);
            assert!(s.sessions.contains_key("a"));
            assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(
                |c| matches!(c, Command::Send { request, .. } if request["type"] == "prompt")
            ));
        });
    }
}

#[test]
fn workflow_host_idle_state_read_then_exit_preserves_unanswered_command_uncertainty() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        reads_identity(s, cx, &requests, json!([]), "p");
        let prompt = submit(s, cx, &requests);
        s.ingest(rpc("a", 7, json!({"type":"session_info_changed"})), cx);
        reads_identity(s, cx, &requests, json!([]), "p");
        assert_eq!(s.sessions["a"].controller.state().identity.id, "p");
        assert!(!s.sessions["a"].controller.state().working);
        assert!(s.sessions["a"].controller.submission_pending());
        assert!(!s.sessions["a"].controller.reconciliation_pending());
        let Event::Pi(mut event) = rpc("a", 7, json!({})) else {
            unreachable!()
        };
        event.kind = "input_closed";
        event.line = None;
        s.ingest(Event::Pi(event.clone()), cx);
        assert!(s.sessions["a"].controller.submission_pending());
        event.kind = "exited";
        s.ingest(Event::Pi(event), cx);
        assert!(!s.sessions["a"].controller.submission_pending());
        assert!(s.sessions["a"].controller.reconciliation_pending());
        assert_eq!(s.sessions["a"].controller.state().draft, "/implement");
        let token = s.sessions["a"].controller.error_token().unwrap();
        assert!(
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .acknowledge_error(&token)
        );
        assert!(s.sessions["a"].controller.reconciliation_pending());
        assert!(!s.sessions["a"].controller.can_release_runtime());
        started(s, "a", 8, cx);
        assert!(s.sessions["a"].controller.reconciliation_pending());
        s.ingest(rpc("a", 7, json!({"type":"response", "id":prompt["id"], "command":"prompt", "success":true, "data":{"disposition":"handled"}})), cx);
        assert!(s.sessions["a"].controller.reconciliation_pending());
        reads_identity(s, cx, &requests, json!([]), "p");
        assert!(!s.sessions["a"].controller.reconciliation_pending());
        assert_eq!(s.sessions["a"].controller.state().draft, "/implement");
    });
}

#[test]
fn workflow_host_rejected_channel_and_cleanup_gate_do_not_transfer_write_ownership() {
    for cleanup in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            ready(s, w, cx, &requests);
            if cleanup {
                s.runtime_lifecycle_track_cancel("a", s.sessions["a"].start_attempt);
            } else {
                requests.close();
            }
            let effects = s
                .sessions
                .get_mut("a")
                .unwrap()
                .controller
                .submit("/implement");
            s.effects("a", effects, cx);
            assert!(!s.sessions["a"].controller.submission_pending());
            assert!(s.sessions["a"].controller.state().error.is_some());
            assert_eq!(s.sessions["a"].controller.state().draft, "/implement");
            assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(
                |c| matches!(c, Command::Send { request, .. } if request["type"] == "prompt")
            ));
        });
    }
}

#[test]
fn workflow_host_incomplete_command_and_acknowledged_uncertain_write_remain_owned() {
    for uncertain in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            ready(s, w, cx, &requests);
            if uncertain {
                let effects = s
                    .sessions
                    .get_mut("a")
                    .unwrap()
                    .controller
                    .submit("ordinary prompt");
                s.effects("a", effects, cx);
                while requests.try_recv().is_ok() {}
                let effects = s.sessions.get_mut("a").unwrap().controller.tick(20_000);
                s.effects("a", effects, cx);
                assert!(s.sessions["a"].controller.reconciliation_pending());
            } else {
                submit(s, cx, &requests);
                failed_notice(s, cx);
            }
            let token = s.sessions["a"].controller.error_token().unwrap();
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .acknowledge_error(&token);
            assert!(!s.sessions["a"].controller.can_release_runtime());
            create(s, "b", "keep", w, cx);
            s.reconcile_empty_sessions(cx);
            assert!(s.sessions.contains_key("a"));
            if uncertain {
                assert!(s.sessions["a"].controller.reconciliation_pending());
            } else {
                assert!(s.sessions["a"].controller.submission_pending());
            }
        });
    }
}

#[test]
fn workflow_host_retains_conversation_and_tools_but_not_custom_only_output() {
    for message in [
        json!({"role":"user", "content":"user work"}),
        json!({"role":"assistant", "content":[{"type":"text", "text":"Pi failure"}], "stopReason":"error", "errorMessage":"meaningful failure"}),
        json!({"role":"toolResult", "toolCallId":"tool", "toolName":"bash", "content":[{"type":"text", "text":"git failed"}], "isError":true}),
        json!({"role":"custom", "customType":"error-notice", "content":"Pi-owned custom failure", "display":true}),
    ] {
        let retains_conversation = message["role"] != "custom";
        with_shell(512, |s, w, cx, requests| {
            ready(s, w, cx, &requests);
            let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
            s.effects("a", effects, cx);
            reads(s, cx, &requests, json!([message]));
            assert!(
                s.sessions["a"]
                    .controller
                    .state()
                    .transcript
                    .iter()
                    .any(|r| r.origin == tau_session::transcript::EntryOrigin::Pi)
            );
            create(s, "b", "keep", w, cx);
            assert_eq!(s.sessions.contains_key("a"), retains_conversation);
            let disposals = std::iter::from_fn(|| requests.try_recv().ok())
                .filter(|command| matches!(command, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
                .count();
            assert_eq!(disposals, usize::from(!retains_conversation));
            assert_eq!(s.sessions["b"].controller.state().draft, "keep");
        });
    }
}

#[test]
fn workflow_host_notice_only_without_controller_error_is_disposable() {
    with_shell(512, |s, w, cx, requests| {
        ready(s, w, cx, &requests);
        let prompt = submit(s, cx, &requests);
        s.ingest(rpc("a", 7, json!({"type":"extension_ui_request", "id":"git-failure", "method":"notify", "notifyType":"error", "message":"git is not a repository"})), cx);
        handled(s, &prompt, cx);
        reads(s, cx, &requests, json!([]));
        let effects = s.sessions.get_mut("a").unwrap().controller.tick(5_000);
        s.effects("a", effects, cx);
        reads(s, cx, &requests, json!([]));
        assert!(s.sessions["a"].controller.state().error.is_none());
        s.sync(w, cx);
        assert!(
            s.fallback.is_none(),
            "an error notice alone is not a fallback incident"
        );
        create(s, "b", "keep", w, cx);
        assert!(!s.sessions.contains_key("a"));
        assert!(
            std::iter::from_fn(|| requests.try_recv().ok())
                .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
        );
    });
}

#[test]
fn workflow_host_provisional_replacement_stays_warm_after_error_ack_and_empty_reads() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        reads_identity(s, cx, &requests, json!([]), "p");
        let prompt = submit(s, cx, &requests);
        failed_notice(s, cx);
        handled(s, &prompt, cx);
        reads_identity(s, cx, &requests, json!([]), "q");
        assert!(s.sessions["a"].controller.provisional_replacement());
        let token = s.sessions["a"].controller.error_token().unwrap();
        assert!(
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .acknowledge_error(&token)
        );
        let effects = s.sessions.get_mut("a").unwrap().controller.tick(5_000);
        s.effects("a", effects, cx);
        reads_identity(s, cx, &requests, json!([]), "q");
        create(s, "b", "keep", w, cx);
        assert!(s.sessions.contains_key("a"));
        assert!(s.sessions["a"].controller.provisional_replacement());
        assert!(!s.sessions["a"].controller.can_release_runtime());
    });
}
