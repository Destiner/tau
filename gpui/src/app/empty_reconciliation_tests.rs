use super::*;
use crate::app::tests::{create, project, rpc, started, with_shell};
use serde_json::json;

// Complete only this runtime's reads; responding can enqueue further bootstrap reads.
fn complete_reads(
    s: &mut Shell,
    key: &str,
    generation: u64,
    messages: serde_json::Value,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
) {
    for _ in 0..128 {
        let Ok(command) = requests.try_recv() else {
            break;
        };
        if let Command::Send {
            runtime, request, ..
        } = command
        {
            if runtime != key {
                continue;
            }
            let data = match request["type"].as_str().unwrap_or_default() {
                "get_state" => json!({"sessionId":"", "sessionFile":"", "isStreaming":false}),
                "get_messages" => json!({"messages":messages}),
                "get_available_models" | "get_scoped_models" => json!({"models":[]}),
                "get_commands" => json!({"commands":[]}),
                "get_available_thinking_levels" => json!({"levels":[]}),
                _ => json!({}),
            };
            s.ingest(rpc(key, generation, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
        }
    }
}

#[test]
fn pending_empty_a_is_retired_after_reads_settle_without_another_selection() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        assert!(s.sessions["a"].controller.pending_count() > 0);
        create(s, "b", "target draft", w, cx);
        assert!(s.sessions.contains_key("a"));
        assert!(s.empty_candidates.contains("a"));
        complete_reads(s, "a", 7, json!([]), cx, &requests);
        assert!(!s.sessions.contains_key("a"));
        assert!(!s.empty_candidates.contains("a"));
        assert_eq!(s.active, "b");
        assert_eq!(s.sessions["b"].controller.state().draft, "target draft");
    });
}

#[test]
fn unread_history_and_catalogue_remain_barriers_until_their_correlated_reply() {
    for held_kind in ["get_messages", "get_available_models"] {
        with_shell(512, |s, w, cx, requests| {
            create(s, "a", "", w, cx);
            started(s, "a", 7, cx);
            create(s, "b", "target draft", w, cx);
            let mut held = Vec::new();
            for _ in 0..128 {
                let Ok(command) = requests.try_recv() else {
                    break;
                };
                if let Command::Send {
                    runtime, request, ..
                } = command
                {
                    if runtime != "a" {
                        continue;
                    }
                    if request["type"] == held_kind {
                        held.push(request);
                        continue;
                    }
                    let data = match request["type"].as_str().unwrap_or_default() {
                        "get_state" => {
                            json!({"sessionId":"", "sessionFile":"", "isStreaming":false})
                        }
                        "get_messages" => json!({"messages":[]}),
                        "get_available_models" | "get_scoped_models" => json!({"models":[]}),
                        "get_commands" => json!({"commands":[]}),
                        "get_available_thinking_levels" => json!({"levels":[]}),
                        _ => json!({}),
                    };
                    s.ingest(rpc("a", 7, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
                }
            }
            assert!(!held.is_empty());
            assert!(
                s.sessions.contains_key("a"),
                "retired with unread {held_kind}"
            );
            for request in held {
                let data = if held_kind == "get_messages" {
                    json!({"messages":[]})
                } else {
                    json!({"models":[]})
                };
                s.ingest(rpc("a", 7, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
            }
            assert!(!s.sessions.contains_key("a"));
            assert_eq!(s.active, "b");
        });
    }
}

#[test]
fn work_acquired_while_reconciling_survives() {
    for kind in [
        "draft",
        "history",
        "notice",
        "extension",
        "recovered",
        "queue",
        "error",
    ] {
        with_shell(512, |s, w, cx, requests| {
            create(s, "a", "", w, cx);
            started(s, "a", 7, cx);
            create(s, "b", "target draft", w, cx);
            let messages = if kind == "history" {
                json!([{"role":"user", "content":"retained history"}])
            } else {
                json!([])
            };
            match kind {
                "draft" => s.sessions.get_mut("a").unwrap().controller.set_draft("late draft"),
                "notice" => s.ingest(rpc("a", 7, json!({"type":"extension_ui_request", "id":"notice", "method":"notify", "message":"late output"})), cx),
                "extension" => s.ingest(rpc("a", 7, json!({"type":"extension_ui_request", "id":"input", "method":"input", "title":"Input"})), cx),
                "recovered" => s.sessions.get_mut("a").unwrap().restored_recovered.push("late recovered work".into()),
                "queue" => {
                    let live = s.sessions.get_mut("a").unwrap();
                    live.controller.handle(json!({"type":"agent_start"}));
                    let effects = live.controller.submit_queued("late queued work", QueueKind::FollowUp);
                    s.effects("a", effects, cx);
                }
                "error" => {
                    let effects = s.sessions.get_mut("a").unwrap().controller.transport_failed(7);
                    s.effects("a", effects, cx);
                }
                "history" => {}
                _ => unreachable!(),
            }
            complete_reads(s, "a", 7, messages, cx, &requests);
            s.reconcile_empty_sessions(cx);
            assert_eq!(
                s.sessions.contains_key("a"),
                !matches!(kind, "notice" | "error"),
                "wrong retention for {kind}"
            );
            assert!(
                !s.empty_candidates.contains("a"),
                "retained retirement intent for {kind}"
            );
            assert_eq!(s.active, "b");
        });
    }
}

#[test]
fn reselecting_cancels_deferred_retirement_and_no_background_blank_scan_occurs() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        create(s, "b", "target draft", w, cx);
        assert!(s.empty_candidates.contains("a"));
        s.select("a", w, cx);
        assert!(!s.empty_candidates.contains("a"));
        complete_reads(s, "a", 7, json!([]), cx, &requests);
        assert_eq!(s.active, "a");
        assert!(s.sessions.contains_key("a"));
        // Restoration creates background blanks without a selection retirement intent.
        s.create_retained(
            "restored".into(),
            project(),
            String::new(),
            String::new(),
            String::new(),
            w,
            cx,
        );
        s.reconcile_empty_sessions(cx);
        assert!(s.sessions.contains_key("restored"));
    });
}

#[test]
fn predecessor_registration_success_releases_only_its_captured_identity() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        complete_reads(s, "a", 7, json!([]), cx, &requests);
        let command = Command::Register {
            runtime: "a".into(),
            generation: 7,
            project: project().path,
            id: "p".into(),
            path: "/predecessor-p".into(),
            name: String::new(),
        };
        s.dispatch(command.clone(), cx);
        // A generation replacement legitimately begins Q while P's accepted write is
        // still in flight. This must not erase the captured P durability operation.
        let effects = s
            .sessions
            .get_mut("a")
            .unwrap()
            .controller
            .begin_generation(8);
        s.effects("a", effects, cx);
        complete_reads(s, "a", 8, json!([]), cx, &requests);
        create(s, "b", "target draft", w, cx);
        assert!(s.sessions.contains_key("a"));
        assert!(s.empty_candidates.contains("a"));
        for (generation, id, path) in [
            (8, "p", "/predecessor-p"),
            (7, "q", "/predecessor-p"),
            (7, "p", "/wrong"),
        ] {
            s.ingest(
                Event::RegistrationStored {
                    runtime: "a".into(),
                    generation,
                    id: id.into(),
                    path: path.into(),
                },
                cx,
            );
            assert!(s.sessions.contains_key("a"));
        }
        let mut predecessor_project = project();
        predecessor_project
            .sessions
            .push(tau_core::models::SessionSummary {
                id: "p".into(),
                path: "/predecessor-p".into(),
                title: "Predecessor".into(),
                title_markdown: None,
                model: String::new(),
                last_active: String::new(),
                last_user_message_at: 0,
                sort_at: 0,
                archived: false,
                selected: false,
            });
        s.ingest(
            Event::Workspace(Ok(WorkspaceSnapshot {
                projects: vec![predecessor_project],
                active_project_path: project().path,
                pi_path: None,
            })),
            cx,
        );
        assert!(
            s.sessions.contains_key("a"),
            "snapshot is not a correlated ack"
        );
        s.ingest(
            Event::RegistrationStored {
                runtime: "a".into(),
                generation: 7,
                id: "p".into(),
                path: "/predecessor-p".into(),
            },
            cx,
        );
        assert!(!s.sessions.contains_key("a"));
        assert_eq!(s.active, "b");
        let predecessor = &s.workspace.as_ref().unwrap().projects[0].sessions[0];
        assert_eq!(predecessor.id, "p");
        assert_eq!(predecessor.path, "/predecessor-p");
    });
}

#[test]
fn registration_failure_and_retry_remain_protected_until_matching_success() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        complete_reads(s, "a", 7, json!([]), cx, &requests);
        let command = Command::Register {
            runtime: "a".into(),
            generation: 7,
            project: project().path,
            id: "p".into(),
            path: "/p".into(),
            name: String::new(),
        };
        s.dispatch(command.clone(), cx);
        s.ingest(Event::RegistrationFailed(command.clone()), cx);
        create(s, "b", "target draft", w, cx);
        assert!(s.sessions.contains_key("a"));
        for retry in std::mem::take(&mut s.register_retries) {
            s.dispatch(retry, cx);
        }
        s.reconcile_empty_sessions(cx);
        assert!(s.sessions.contains_key("a"));
        s.ingest(
            Event::RegistrationStored {
                runtime: "a".into(),
                generation: 7,
                id: "p".into(),
                path: "/p".into(),
            },
            cx,
        );
        assert!(!s.sessions.contains_key("a"));
    });
}

#[test]
fn cleanup_ack_cannot_be_reacquired_by_late_retry_failure() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        create(s, "b", "target draft", w, cx);
        assert!(s.register_retries.is_empty());
        let command = std::iter::from_fn(|| requests.try_recv().ok())
            .find(|command| matches!(command, Command::DisposeEmpty { .. }))
            .unwrap();
        let Command::DisposeEmpty {
            runtime,
            attempt,
            generation,
        } = command.clone()
        else {
            panic!("missing disposal")
        };
        s.empty_disposed(runtime.clone(), attempt, generation, true, cx);
        assert!(!s.empty_session_retry_is_current(&command));
        s.empty_disposed(runtime, attempt, generation, false, cx);
        s.empty_disposal_dispatch_failed(command, cx);
        assert!(s.register_retries.is_empty());
        assert!(s.empty_disposals.is_empty());
    });
}
