use super::*;
use crate::app::tests::{app, project, rpc, shell, started};
use gpui_kit::{TestApp, TestAppWindow, size};
use serde_json::{Value, json};

type Sent = (String, u64, Value);

struct Fixture {
    root: TestAppWindow<Root>,
    shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    commands: Vec<Command>,
    identity: String,
    app: TestApp,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.root.update(|_, w, cx| w.blur(cx));
            self.app.run_until_parked();
        }
    }
}

impl Fixture {
    fn new(remote: bool) -> Self {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| {
                let mut p = project();
                if remote {
                    p.connection_string = Some("ssh fake@fixture".into());
                }
                s.window = Some(w.window_handle());
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![p.clone()],
                    active_project_path: p.path.clone(),
                    pi_path: None,
                });
                s.mount(w, cx);
                s.create_live(
                    "a".into(),
                    p,
                    String::new(),
                    String::new(),
                    String::new(),
                    w,
                    cx,
                );
                started(s, "a", 8, cx);
            });
        });
        let mut f = Self {
            root,
            shell,
            requests,
            commands: vec![],
            identity: "phase".into(),
            app,
        };
        f.reads(json!([]));
        assert!(f.commands.iter().any(|command| matches!(command,
            Command::Start { runtime, project, .. }
                if runtime == "a" && project.connection_string.is_some() == remote)));
        f.root.update(|_, w, cx| {
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .editor()
                .update(cx, |e, cx| e.focus(w, cx));
        });
        f.paint();
        f
    }

    fn paint(&mut self) {
        self.app.run_until_parked();
        self.root.draw();
        self.app.run_until_parked();
        for _ in 0..2 {
            self.root.update(|_, w, cx| w.simulate_next_frame(cx));
            self.root.draw();
            self.app.run_until_parked();
        }
    }

    fn sent(&mut self) -> Vec<Sent> {
        let mut sent = vec![];
        while let Ok(command) = self.requests.try_recv() {
            if let Command::Send {
                runtime,
                generation,
                request,
            } = &command
            {
                assert_eq!(runtime, "a");
                assert_eq!(*generation, 8);
                sent.push((runtime.clone(), *generation, request.clone()));
            }
            self.commands.push(command);
        }
        sent
    }

    fn ingest(&mut self, generation: u64, event: Value) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(rpc("a", generation, event), cx);
                s.sync(w, cx);
            })
        });
        self.paint();
    }

    fn respond(&mut self, request: &Sent, data: Value) {
        self.ingest(
            request.1,
            json!({"type":"response", "id":request.2["id"],
            "command":request.2["type"], "success":true, "data":data}),
        );
    }

    fn reads(&mut self, messages: Value) {
        for _ in 0..16 {
            let requests = self.sent();
            if requests.is_empty() {
                return;
            }
            for request in requests {
                let data = match request.2["type"].as_str().unwrap() {
                    "get_state" => {
                        json!({"sessionId":self.identity, "sessionFile":format!("/fixture/{}.jsonl", self.identity),
                        "sessionName":"Named command session", "isStreaming":false})
                    }
                    "get_messages" => json!({"messages":messages}),
                    "get_commands" => {
                        json!({"commands":[{"name":"deploy", "source":"extension", "description":"Run fixture"}]})
                    }
                    "get_available_models" | "get_scoped_models" => json!({"models":[]}),
                    "get_available_thinking_levels" => json!({"levels":["off"]}),
                    other => panic!("unexpected read {other}"),
                };
                self.respond(&request, data);
            }
        }
        panic!("reads did not settle");
    }

    fn submit(&mut self) -> Sent {
        self.root.simulate_input("/deploy");
        self.paint();
        self.root.simulate_keystroke("enter");
        self.paint();
        let sent = self.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].2["type"], "prompt");
        assert_eq!(sent[0].2["message"], "/deploy");
        self.root.read(|_, cx| {
            let c = &self.shell.read(cx).sessions["a"].controller;
            assert!(c.submission_pending());
            assert!(c.state().draft.is_empty());
        });
        sent.into_iter().next().unwrap()
    }

    fn leave(&mut self) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.new_session(None, w, cx);
                assert_ne!(s.active, "a");
            })
        });
        self.paint();
    }

    fn probes(&mut self, messages: Value) {
        // The production probe schedule, within the ordinary request timeout.
        for now in [150, 450, 1_000, 2_000, 3_200, 3_500] {
            self.root.update(|_, _, cx| {
                self.shell.update(cx, |s, cx| {
                    if let Some(live) = s.sessions.get_mut("a") {
                        let effects = live.controller.tick(now);
                        s.effects("a", effects, cx);
                    }
                    s.reconcile_empty_sessions(cx);
                })
            });
            self.reads(messages.clone());
        }
    }

    fn present(&self) -> bool {
        self.root
            .read(|_, cx| self.shell.read(cx).sessions.contains_key("a"))
    }

    fn assert_disposed_once(&mut self) {
        self.sent();
        assert!(
            !self.present(),
            "reconciled custom-only command session must retire when left"
        );
        assert_eq!(self.commands.iter().filter(|c| matches!(c, Command::DisposeEmpty { runtime, generation:8, .. } if runtime == "a")).count(), 1);
        assert!(!self.commands.iter().any(|c| matches!(
            c,
            Command::Archive { .. } | Command::ArchiveSource { .. } | Command::Register { .. }
        )));
        self.ingest(7, json!({"type":"message_end", "message":{"role":"assistant", "content":[{"type":"text", "text":"stale"}]}}));
        assert!(!self.present());
    }
}

fn progress() -> Value {
    json!([{"role":"custom", "customType":"fixture-progress", "display":true,
        "timestamp":1, "content":"Finished fixture command"}])
}

#[test]
fn custom_history_command_keeps_selected_then_retires_once_local_and_fake_ssh() {
    for remote in [false, true] {
        let mut f = Fixture::new(remote);
        let prompt = f.submit();
        f.respond(&prompt, json!({}));
        f.reads(progress());
        f.probes(progress());
        assert!(f.present());
        f.root.read(|_, cx| {
            let c = &f.shell.read(cx).sessions["a"].controller;
            assert_eq!(c.state().transcript.len(), 1);
            assert_eq!(c.state().transcript[0].kind, "notice");
            assert!(!c.reconciliation_pending());
            assert!(!c.provisional_replacement());
            assert!(!c.state().durable);
        });
        f.leave();
        f.assert_disposed_once();
    }
}

#[test]
fn leaving_during_command_revisits_retirement_after_successful_reconciliation() {
    for remote in [false, true] {
        let mut f = Fixture::new(remote);
        let prompt = f.submit();
        f.leave();
        assert!(f.present());
        f.respond(&prompt, json!({}));
        f.reads(progress());
        f.probes(progress());
        f.assert_disposed_once();
    }
}

#[test]
fn empty_history_pending_command_also_retires_without_another_selection() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.leave();
    assert!(f.present());
    f.respond(&prompt, json!({}));
    f.reads(json!([]));
    f.probes(json!([]));
    f.assert_disposed_once();
}

#[test]
fn leaving_while_command_reconciliation_reports_working_preserves_retirement_intent() {
    for remote in [false, true] {
        let mut f = Fixture::new(remote);
        let prompt = f.submit();
        f.respond(&prompt, json!({}));
        let requests = f.sent();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].2["type"], "get_state");
        f.respond(
            &requests[0],
            json!({
                "sessionId":"phase", "sessionFile":"/fixture/phase.jsonl",
                "sessionName":"Named command session", "isStreaming":true
            }),
        );
        f.reads(progress());
        f.root.read(|_, cx| {
            let controller = &f.shell.read(cx).sessions["a"].controller;
            assert!(controller.state().working);
            assert!(!controller.submission_pending());
            assert!(controller.reconciliation_pending());
        });
        f.leave();
        assert!(f.present());
        f.root
            .read(|_, cx| assert!(f.shell.read(cx).empty_candidates.contains("a")));
        f.ingest(8, json!({"type":"agent_settled"}));
        f.reads(progress());
        f.probes(progress());
        f.assert_disposed_once();
    }
}

#[test]
fn reselecting_pending_command_cancels_retirement_without_replaying_input() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.leave();
    f.root
        .update(|_, w, cx| f.shell.update(cx, |s, cx| s.select("a", w, cx)));
    f.paint();
    f.respond(&prompt, json!({}));
    f.reads(progress());
    f.probes(progress());
    assert!(f.present());
    f.root.read(|_, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.active, "a");
        assert!(!s.empty_candidates.contains("a"));
    });
    assert_eq!(
        f.commands
            .iter()
            .filter(|c| matches!(c, Command::Send { request, .. } if request["type"] == "prompt"))
            .count(),
        1
    );
    assert!(
        !f.commands
            .iter()
            .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
    );
}

#[test]
fn command_custom_output_does_not_override_real_pi_history() {
    for message in [
        json!({"role":"user", "content":"retained work", "timestamp":2}),
        json!({"role":"assistant", "content":[{"type":"text", "text":"retained answer"}], "timestamp":2}),
        json!({"role":"toolResult", "toolCallId":"tool-1", "toolName":"read", "content":"result", "timestamp":2}),
    ] {
        let mut f = Fixture::new(false);
        let prompt = f.submit();
        f.leave();
        f.respond(&prompt, json!({}));
        let mut messages = progress();
        messages.as_array_mut().unwrap().push(message);
        f.reads(messages.clone());
        f.probes(messages);
        assert!(f.present());
        assert!(
            !f.commands
                .iter()
                .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
        );
    }
}

#[test]
fn whitespace_editor_and_recovered_work_survive_custom_only_reconciliation() {
    for kind in ["draft", "editor", "recovery", "queue", "dialog"] {
        let mut f = Fixture::new(false);
        let prompt = f.submit();
        match kind {
            "draft" => f.ingest(8, json!({"type":"extension_ui_request", "id":"editor", "method":"set_editor_text", "text":" \n\t"})),
            "editor" => {
                f.root.simulate_input(" ");
                f.paint();
            }
            "dialog" => f.ingest(8, json!({"type":"extension_ui_request", "id":"input", "method":"input", "title":"Fixture input"})),
            "recovery" => f.root.update(|_, _, cx| f.shell.update(cx, |s, _| {
                s.sessions.get_mut("a").unwrap().restored_recovered.push("recovered work".into());
            })),
            "queue" => f.root.update(|_, _, cx| f.shell.update(cx, |s, cx| {
                let effects = s.sessions.get_mut("a").unwrap().controller.submit_queued("queued work", QueueKind::FollowUp);
                s.effects("a", effects, cx);
            })),
            _ => unreachable!(),
        }
        f.leave();
        assert!(f.present(), "lost {kind} before acknowledgement");
        // A queued submission has its own RPC ownership; hold it rather than
        // claiming the queue is drained when only the command has replied.
        if kind == "queue" {
            continue;
        }
        f.respond(&prompt, json!({}));
        f.reads(progress());
        f.probes(progress());
        assert!(f.present(), "lost {kind} after reconciliation");
    }
}

#[test]
fn failed_command_history_keeps_row_and_never_replays_command() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.leave();
    f.respond(&prompt, json!({}));
    let reads = f.sent();
    assert!(!reads.is_empty());
    for request in reads {
        f.ingest(
            8,
            json!({"type":"response", "id":request.2["id"],
            "command":request.2["type"], "success":false, "error":"fixture failure"}),
        );
    }
    f.root
        .update(|_, _, cx| f.shell.update(cx, |s, cx| s.reconcile_empty_sessions(cx)));
    assert!(f.present());
    f.root.read(|_, cx| {
        assert!(
            f.shell.read(cx).sessions["a"]
                .controller
                .reconciliation_pending()
        )
    });
    f.sent();
    assert_eq!(
        f.commands
            .iter()
            .filter(|c| matches!(c, Command::Send { request, .. } if request["type"] == "prompt"))
            .count(),
        1
    );
}

#[test]
fn old_generation_history_cannot_materialize_or_release_pending_command_reads() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.leave();
    f.respond(&prompt, json!({}));
    let requests = f.sent();
    assert!(!requests.is_empty());
    for request in requests {
        f.ingest(7, json!({"type":"response", "id":request.2["id"],
            "command":request.2["type"], "success":true,
            "data":{"sessionId":"stale", "sessionFile":"/stale.jsonl", "messages":[{"role":"user", "content":"stale work"}]}}));
        assert!(f.present());
        let data = match request.2["type"].as_str().unwrap() {
            "get_state" => {
                json!({"sessionId":"phase", "sessionFile":"/fixture/phase.jsonl", "isStreaming":false})
            }
            "get_messages" => json!({"messages":progress()}),
            other => panic!("unexpected command reconciliation {other}"),
        };
        f.respond(&request, data);
    }
    f.reads(progress());
    f.probes(progress());
    f.assert_disposed_once();
}

#[test]
fn late_assistant_materialization_and_unacknowledged_registration_protect_command_row() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.respond(&prompt, json!({}));
    f.reads(progress());
    f.probes(progress());
    f.ingest(8, json!({"type":"agent_start"}));
    f.ingest(
        8,
        json!({"type":"message_end", "message":{"role":"assistant", "timestamp":2,
        "content":[{"type":"text", "text":"Actual assistant work"}], "stopReason":"stop"}}),
    );
    f.leave();
    assert!(f.present());
    f.reads(json!([{"role":"assistant", "timestamp":2,
        "content":[{"type":"text", "text":"Actual assistant work"}]}]));
    f.sent();
    assert!(f.present());
    assert!(f.commands.iter().any(
        |c| matches!(c, Command::Register { runtime, generation:8, id, .. }
        if runtime == "a" && id == "phase")
    ));
    assert!(
        !f.commands
            .iter()
            .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
    );
}

#[test]
fn unknown_notice_sources_are_not_treated_as_custom_progress() {
    for source in ["notice:1", "customish:1", "assistant:1"] {
        let mut f = Fixture::new(false);
        let prompt = f.submit();
        f.respond(&prompt, json!({}));
        f.reads(progress());
        f.probes(progress());
        // Conservative boundary for future/unknown projection rows, not completion.
        f.root.read(|_, cx| {
            let mut entry = f.shell.read(cx).sessions["a"].controller.state().transcript[0].clone();
            assert!(is_non_conversation_notice(&entry));
            entry.source = source.into();
            assert!(!is_non_conversation_notice(&entry));
            entry.source = "custom:1".into();
            entry.kind = "tool".into();
            assert!(!is_non_conversation_notice(&entry));
        });
    }
}

#[test]
fn accepted_command_transport_loss_preserves_uncertain_work() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, cx| {
            s.ingest(
                Event::Written {
                    runtime: "a".into(),
                    generation: 8,
                    request: prompt.2.clone(),
                    success: true,
                },
                cx,
            );
            let effects = s
                .sessions
                .get_mut("a")
                .unwrap()
                .controller
                .transport_failed(8);
            s.effects("a", effects, cx);
        })
    });
    f.leave();
    assert!(f.present());
    f.root.read(|_, cx| {
        assert!(
            f.shell.read(cx).sessions["a"]
                .controller
                .reconciliation_pending()
        )
    });
    f.sent();
    assert!(
        !f.commands
            .iter()
            .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
    );
}

#[test]
fn custom_only_replacement_successor_stays_provisional_after_probes() {
    let mut f = Fixture::new(false);
    let prompt = f.submit();
    f.leave();
    f.identity = "successor".into();
    f.respond(&prompt, json!({}));
    f.reads(progress());
    f.probes(progress());
    assert!(f.present());
    f.root.read(|_, cx| {
        let c = &f.shell.read(cx).sessions["a"].controller;
        assert_eq!(c.state().identity.id, "successor");
        assert!(c.provisional_replacement());
        assert!(!c.state().durable);
    });
    assert!(
        !f.commands
            .iter()
            .any(|c| matches!(c, Command::DisposeEmpty { runtime, .. } if runtime == "a"))
    );
}
