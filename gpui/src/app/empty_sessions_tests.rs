use super::*;
use crate::app::tests::{create, project, rpc, started, with_shell};
use serde_json::json;

fn settle_startup(
    s: &mut Shell,
    id: &str,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
) {
    started(s, id, 7, cx);
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
            if runtime != id {
                continue;
            }
            let data = match request["type"].as_str().unwrap_or_default() {
                "get_state" => json!({"sessionId":"", "sessionFile":"", "isStreaming":false}),
                "get_messages" => json!({"messages":[]}),
                "get_available_models" => json!({"models":[]}),
                "get_scoped_models" => json!({"models":[]}),
                "get_commands" => json!({"commands":[]}),
                "get_available_thinking_levels" => json!({"levels":[]}),
                _ => json!({}),
            };
            s.ingest(rpc(&runtime, generation, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
        }
    }
    assert_eq!(s.sessions[id].controller.pending_count(), 0);
    assert!(!s.sessions[id].controller.state().history_loading);
}

#[test]
fn actual_new_session_switch_removes_empty_start_and_late_events_cannot_resurrect() {
    with_shell(128, |s, w, cx, requests| {
        create(s, "empty", "", w, cx);
        let attempt = s.sessions["empty"].start_attempt;
        // A late save originally captured the retired selection and an obsolete draft row.
        let stale = Preferences {
            selected_draft: "empty".into(),
            drafts: vec![Draft {
                key: "empty".into(),
                ..Draft::default()
            }],
            ..Preferences::default()
        };
        s.preferences = stale.clone();
        s.pending_saves.insert(99, stale);
        create(s, "target", "keep me", w, cx);
        assert!(!s.sessions.contains_key("empty"));
        assert_eq!(s.active, "target");
        assert!(
            !s.preferences_snapshot()
                .drafts
                .iter()
                .any(|draft| draft.key == "empty")
        );
        assert!(std::iter::from_fn(|| requests.try_recv().ok()).any(|command| matches!(command, Command::DisposeEmpty { runtime, attempt: a, generation: 0 } if runtime == "empty" && a == attempt)));
        s.ingest(
            Event::Started {
                runtime: "empty".into(),
                attempt,
                result: Ok(7),
            },
            cx,
        );
        s.ingest(rpc("empty", 7, json!({"type":"agent_start"})), cx);
        s.ingest(
            Event::Written {
                runtime: "empty".into(),
                generation: 7,
                request: json!({"id":"late", "type":"prompt"}),
                success: false,
            },
            cx,
        );
        s.ingest(
            Event::Saved {
                revision: 99,
                success: true,
                stopped: vec![],
            },
            cx,
        );
        assert!(!s.sessions.contains_key("empty"));
        assert!(
            !s.preferences_snapshot()
                .drafts
                .iter()
                .any(|draft| draft.key == "empty")
        );
        assert_ne!(s.preferences.selected_draft, "empty");
    });
}

#[test]
fn invalid_and_same_selection_do_not_retire_and_whitespace_is_work() {
    for draft in ["", " ", "\n\t", "unsent"] {
        with_shell(128, |s, w, cx, _| {
            create(s, "source", draft, w, cx);
            s.select("missing", w, cx);
            s.select("source", w, cx);
            assert!(s.sessions.contains_key("source"));
            assert_eq!(s.active, "source");
            create(s, "target", "target draft", w, cx);
            assert_eq!(s.sessions.contains_key("source"), !draft.is_empty());
            if !draft.is_empty() {
                assert_eq!(s.sessions["source"].controller.state().draft, draft);
            }
        });
    }
}

#[test]
fn startup_pending_work_notifications_and_extension_input_are_retained() {
    for kind in [
        "pending",
        "working",
        "notice",
        "extension",
        "registration",
        "saved",
        "recovered",
        "queued",
        "rejected",
    ] {
        with_shell(256, |s, w, cx, requests| {
            create(s, "source", "", w, cx);
            if kind == "pending" {
                started(s, "source", 7, cx);
            } else {
                settle_startup(s, "source", cx, &requests);
                match kind {
                    "working" => s.ingest(rpc("source", 7, json!({"type":"agent_start"})), cx),
                    "notice" => s.ingest(rpc("source", 7, json!({"type":"extension_ui_request", "id":"notice", "method":"notify", "message":"visible output"})), cx),
                    "extension" => s.ingest(rpc("source", 7, json!({"type":"extension_ui_request", "id":"input", "method":"input", "title":"Input"})), cx),
                    "registration" => s.protect_empty_registration(&Command::Register { runtime: "source".into(), generation: 7, project: project().path, id: "saved".into(), path: "/saved".into(), name: String::new() }),
                    "saved" => s.sessions.get_mut("source").unwrap().ephemeral = false,
                    "recovered" => s.sessions.get_mut("source").unwrap().restored_recovered.push("failed submission".into()),
                    "queued" => {
                        let live = s.sessions.get_mut("source").unwrap();
                        live.controller.handle(json!({"type":"agent_start"}));
                        let effects = live.controller.submit_queued("queued work", QueueKind::FollowUp);
                        s.effects("source", effects, cx);
                    }
                    "rejected" => {
                        let live = s.sessions.get_mut("source").unwrap();
                        let effects = live.controller.submit("rejected work");
                        for effect in effects {
                            if let Effect::Send(request) = effect {
                                live.controller.request_failed(request["id"].as_str().unwrap());
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            create(s, "target", "target draft", w, cx);
            assert!(s.sessions.contains_key("source"), "lost {kind}");
        });
    }
}

#[test]
fn cleanup_failure_keeps_one_retry_and_acknowledgement_releases_it() {
    with_shell(1, |s, w, cx, requests| {
        // Avoid Start/Save saturation from becoming a session error before retirement.
        s.loaded = false;
        s.create_retained(
            "empty".into(),
            project(),
            String::new(),
            String::new(),
            String::new(),
            w,
            cx,
        );
        s.active = "empty".into();
        s.commands
            .try_send(Command::Collapse {
                project: project().path,
                collapsed: false,
            })
            .unwrap();
        assert!(s.discard_empty_before_switch("valid-target", cx));
        assert!(s.error.is_some());
        assert_eq!(s.register_retries.len(), 1);
        let _ = requests.try_recv();
        let Command::DisposeEmpty {
            runtime,
            attempt,
            generation,
        } = s.register_retries[0].clone()
        else {
            panic!("missing cleanup retry")
        };
        s.empty_disposed(runtime.clone(), attempt, generation, false, cx);
        assert_eq!(s.register_retries.len(), 1);
        // begin_exit drains retry ownership; a saturated retry must restore it.
        let retry = s.register_retries.pop().unwrap();
        s.empty_disposal_dispatch_failed(retry, cx);
        assert_eq!(s.register_retries.len(), 1);
        s.empty_disposed(runtime, attempt, generation, true, cx);
        assert!(s.register_retries.is_empty());
    });
}

#[test]
fn actual_new_session_and_project_switch_retire_only_after_valid_target() {
    for target_project in [None, Some("/other/project".to_string())] {
        with_shell(256, |s, w, cx, _| {
            let mut other = project();
            other.path = "/other/project".into();
            other.working_directory = other.path.clone();
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project(), other],
                active_project_path: project().path,
                pi_path: None,
            });
            create(s, "empty", "", w, cx);
            s.new_session(Some("/invalid/project".into()), w, cx);
            assert_eq!(s.active, "empty");
            assert!(s.sessions.contains_key("empty"));
            s.new_session(target_project.clone(), w, cx);
            assert!(!s.sessions.contains_key("empty"));
            assert!(s.sessions.contains_key(&s.active));
            assert_eq!(
                s.sessions[&s.active].project.path,
                target_project.clone().unwrap_or_else(|| project().path)
            );
        });
    }
}

#[test]
fn settled_empty_fake_rpc_session_is_disposable() {
    with_shell(256, |s, w, cx, requests| {
        create(s, "source", "", w, cx);
        settle_startup(s, "source", cx, &requests);
        create(s, "target", "target draft", w, cx);
        assert!(!s.sessions.contains_key("source"));
    });
}
