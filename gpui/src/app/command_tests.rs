use super::tests::{app, create, project, rpc, shell, started};
use super::*;
use gpui_kit::{Focusable, MouseButton, TestApp, TestAppWindow, size, test::TestWindowExt};
use serde_json::{Value, json};

type Sent = (String, u64, Value);

struct Fixture {
    root: TestAppWindow<Root>,
    shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    // Retained entity handles must be released before the test application.
    app: TestApp,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        // Release TestPlatform's focused input handler before checking entity leaks.
        self.root.update(|_, w, cx| w.blur(cx));
        self.app.run_until_parked();
    }
}

fn catalog() -> Value {
    json!([
        {"name":"session-name", "description":"Name this session", "source":"extension"},
        {"name":"summarize", "description":"Summarize the work", "source":"prompt"},
        {"name":"skill:source-check", "description":"Check a source", "source":"skill",
         "sourceInfo":{"path":"/fixture/skill", "source":"local", "scope":"project",
                       "origin":"top-level", "baseDir":"/fixture"}}
    ])
}

fn response(request: &Sent, data: Value) -> Value {
    json!({"type":"response", "id":request.2["id"],
           "command":request.2["type"], "success":true, "data":data})
}

impl Fixture {
    fn new() -> Self {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(256);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| {
                s.window = Some(w.window_handle());
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.mount(w, cx);
                create(s, "a", "", w, cx);
                started(s, "a", 8, cx);
                s.select("a", w, cx);
            });
        });
        app.run_until_parked();
        let mut f = Self {
            root,
            shell,
            requests,
            app,
        };
        f.complete_reads("a", "saved-a", catalog());
        f.paint();
        // Initial fixture focus only; interactions must preserve/transfer it themselves.
        f.root.update(|_, w, cx| {
            assert!(w.is_window_active());
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .editor()
                .update(cx, |e, cx| e.focus(w, cx));
        });
        f.app.run_until_parked();
        f.paint();
        assert!(f.sent().is_empty());
        f
    }

    fn sent(&self) -> Vec<Sent> {
        std::iter::from_fn(|| self.requests.try_recv().ok())
            .filter_map(|c| match c {
                Command::Send {
                    runtime,
                    generation,
                    request,
                } => {
                    assert_eq!(generation, 8);
                    Some((runtime, generation, request))
                }
                _ => None,
            })
            .collect()
    }

    fn paint(&mut self) {
        self.app.run_until_parked();
        self.root.draw();
        self.app.run_until_parked();
        // Deliver callbacks against the preceding paint, including deferred draw effects.
        for _ in 0..2 {
            self.root.update(|_, w, cx| w.simulate_next_frame(cx));
            self.root.draw();
            self.app.run_until_parked();
        }
    }

    fn ingest_at(&mut self, runtime: &str, generation: u64, event: Value) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(rpc(runtime, generation, event), cx);
                s.sync(w, cx);
            });
        });
        self.paint();
    }

    fn ingest(&mut self, event: Value) {
        self.ingest_at("a", 8, event);
    }

    fn respond(&mut self, request: &Sent, data: Value) {
        self.ingest_at(&request.0, request.1, response(request, data));
    }

    fn read_data(request: &Sent, identity: &str, commands: &Value) -> Value {
        match request.2["type"].as_str() {
            Some("get_state") => json!({"sessionId":identity, "sessionName":identity,
                "sessionFile":format!("/fixture/{identity}.jsonl"), "isStreaming":false}),
            Some("get_messages") => json!({"messages":[]}),
            Some("get_available_models" | "get_scoped_models") => json!({"models":[]}),
            Some("get_available_thinking_levels") => json!({"levels":["off", "high"]}),
            Some("get_commands") => json!({"commands":commands}),
            other => panic!("unexpected bootstrap request: {other:?}"),
        }
    }

    fn complete_reads(&mut self, runtime: &str, identity: &str, commands: Value) {
        let mut command_reads = 0;
        for _ in 0..16 {
            let requests = self.sent();
            if requests.is_empty() {
                assert!(
                    command_reads > 0,
                    "fixture must answer an actual get_commands request"
                );
                self.root.read(|_, cx| {
                    let state = self.shell.read(cx).sessions[runtime].controller.state();
                    assert!(state.ready);
                    assert_eq!(state.commands, commands.as_array().unwrap().clone());
                });
                return;
            }
            for request in requests {
                assert_eq!(request.0, runtime);
                command_reads += usize::from(request.2["type"] == "get_commands");
                let data = Self::read_data(&request, identity, &commands);
                self.respond(&request, data);
            }
        }
        panic!("bootstrap did not settle");
    }

    fn draft(&self, runtime: &str) -> String {
        self.root.read(|_, cx| {
            self.shell.read(cx).sessions[runtime]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .to_string()
        })
    }

    fn assert_focus(&mut self, runtime: &str) {
        self.root.update(|_, w, cx| {
            assert!(w.is_window_active());
            assert!(
                self.shell.read(cx).sessions[runtime]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            );
        });
    }

    fn lifecycle(&mut self, streaming: bool) {
        self.ingest(json!({"type":if streaming { "agent_start" } else { "agent_settled" }}));
        for _ in 0..8 {
            let requests = self.sent();
            if requests.is_empty() {
                self.root.read(|_, cx| {
                    assert_eq!(
                        self.shell.read(cx).sessions["a"].controller.state().working,
                        streaming
                    )
                });
                return;
            }
            for request in requests {
                assert_ne!(
                    request.2["type"], "get_commands",
                    "same identity must not refresh commands"
                );
                let mut data = Self::read_data(&request, "saved-a", &catalog());
                if request.2["type"] == "get_state" {
                    data["isStreaming"] = json!(streaming);
                }
                self.respond(&request, data);
            }
        }
        panic!("lifecycle reads did not settle");
    }

    fn assert_quiet_pending(&mut self) {
        self.paint();
        self.root.update(|_, w, cx| {
            let live = &self.shell.read(cx).sessions["a"];
            assert!(live.controller.queue_busy());
            assert!(live.queue.read(cx).props().busy);
            if let Some(clear) = w.try_find("clear") {
                assert_eq!(clear.disabled(), Some(true));
            }
            assert!(
                w.try_find("feedback").is_none(),
                "no generic pending queue feedback"
            );
        });
    }

    fn prompt(&mut self) -> Sent {
        let mut prompt = None;
        for _ in 0..8 {
            self.assert_quiet_pending();
            let requests = self.sent();
            for request in requests {
                match request.2["type"].as_str() {
                    Some("set_steering_mode") => {
                        assert_eq!(request.2["mode"], "all");
                        self.respond(&request, json!({}));
                    }
                    Some("set_follow_up_mode") => {
                        assert_eq!(request.2["mode"], "one-at-a-time");
                        self.respond(&request, json!({}));
                    }
                    Some("prompt") => {
                        assert!(
                            prompt.replace(request).is_none(),
                            "one gesture must send once"
                        );
                    }
                    other => panic!("unexpected submission request: {other:?}"),
                }
            }
            if let Some(request) = prompt {
                self.paint();
                assert!(self.sent().is_empty(), "no duplicate deferred prompt");
                return request;
            }
        }
        panic!("submission did not reach prompt");
    }

    fn select(&mut self, runtime: &str) {
        let name = self.root.read(|_, cx| {
            self.shell.read(cx).sessions[runtime]
                .controller
                .state()
                .identity
                .name
                .clone()
        });
        self.root.simulate_keystroke(if cfg!(target_os = "macos") {
            "cmd-p"
        } else {
            "ctrl-p"
        });
        self.paint();
        self.root.simulate_input(&name);
        self.paint();
        self.root.simulate_keystroke("enter");
        self.paint();
        self.root
            .read(|_, cx| assert_eq!(self.shell.read(cx).active, runtime));
        self.assert_focus(runtime);
        assert!(self.sent().is_empty());
    }
}

fn follow_up_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    }
}

#[test]
fn root_slash_picker_tab_uses_pi_order_clamps_and_never_sends() {
    let mut f = Fixture::new();
    let editor_bounds = f.root.update(|_, w, _| w.find("editor").bounds());
    f.root.simulate_input("/");
    f.paint();
    f.root.update(|_, w, cx| {
        let popup = w.find("command-menu").bounds();
        assert!(w.find("command-option-0").visible());
        assert_eq!(
            w.find("editor").bounds(),
            editor_bounds,
            "popup is an overlay"
        );
        assert!(popup.top() >= w.find("header").bounds().bottom());
        assert!(popup.bottom() <= w.viewport_size().height);
        assert!(popup.left() >= editor_bounds.left());
        assert!(popup.right() <= editor_bounds.right());
        assert!(
            w.painted_quads().iter().any(|q| {
                let bounds = q.bounds.map(|p| px(p.as_f32() / w.scale_factor()));
                popup.contains(&bounds.center())
            }),
            "picker must actually paint"
        );
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "session-name"
        );
    });
    f.root.simulate_keystroke("up");
    f.root.read(|_, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "session-name",
            "Up clamps at the first Pi option"
        )
    });
    for _ in 0..4 {
        f.root.simulate_keystroke("down");
    }
    f.root.simulate_keystroke("tab");
    f.paint();
    assert_eq!(f.draft("a"), "/skill:source-check ");
    assert!(f.sent().is_empty(), "completion is editing, not RPC");
    f.root.update(|_, w, cx| {
        assert!(w.try_find("command-menu").is_none());
        let editor = f.shell.read(cx).sessions["a"].composer.read(cx).editor();
        assert_eq!(editor.read(cx).selected_range(), 20..20);
    });
    f.assert_focus("a");
}

#[test]
fn root_first_empty_queue_submission_is_quiet_and_prompt_is_exactly_once() {
    for (text, remount) in [
        ("ordinary first send", false),
        ("/", false),
        ("/skill:source-check", true),
    ] {
        let mut f = Fixture::new();
        if remount {
            f.root
                .update(|_, w, cx| f.shell.update(cx, |s, cx| s.mount(w, cx)));
            f.paint();
            f.assert_focus("a");
        }
        f.root.simulate_input(text);
        f.paint();
        f.root.simulate_keystroke("enter");
        let request = f.prompt();
        assert_eq!(request.0, "a");
        assert_eq!(request.1, 8);
        assert_eq!(
            request.2["message"],
            if text == "/" { "/session-name" } else { text }
        );
        assert!(request.2["streamingBehavior"].is_null());
        assert_eq!(f.draft("a"), "");
        f.root.simulate_keystroke("enter");
        f.assert_quiet_pending();
        assert!(
            f.sent().is_empty(),
            "repeat Enter cannot submit while pending"
        );
        f.respond(&request, json!({}));
        f.root.update(|_, w, cx| {
            assert!(!f.shell.read(cx).sessions["a"].controller.queue_busy());
            assert!(w.try_find("feedback").is_none());
        });
        f.assert_focus("a");
    }
}

#[test]
fn root_picker_pointer_submits_without_editor_selection_dropthrough() {
    let mut f = Fixture::new();
    f.root.simulate_input("/");
    f.paint();
    let at = f
        .root
        .update(|_, w, _| w.find("command-option-1").bounds().center());
    f.root.simulate_click(at, MouseButton::Left);
    let request = f.prompt();
    assert_eq!(request.2["message"], "/summarize");
    f.assert_focus("a");
    f.root.update(|_, w, cx| {
        assert!(w.try_find("command-menu").is_none());
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .selected_range(),
            0..0
        );
    });
    assert_eq!(f.draft("a"), "");
    assert!(f.sent().is_empty());
}

#[test]
fn root_command_steering_followup_and_streaming_extension_admission() {
    for key in ["enter", follow_up_key()] {
        for (text, invocation) in [
            ("/sum", "/summarize"),
            ("/skill:source", "/skill:source-check"),
        ] {
            let mut f = Fixture::new();
            f.lifecycle(true);
            f.ingest(
                json!({"type":"queue_update", "steering":["existing steering"], "followUp":[]}),
            );
            assert!(f.sent().is_empty());
            f.root.simulate_input(text);
            f.paint();
            f.root.simulate_keystroke(key);
            let request = f.prompt();
            assert_eq!(request.2["message"], invocation);
            assert_eq!(
                request.2["streamingBehavior"],
                if key == "enter" { "steer" } else { "followUp" }
            );
            f.root.simulate_keystroke(key);
            assert!(f.sent().is_empty());
            f.assert_quiet_pending();
            f.assert_focus("a");
        }
        let mut f = Fixture::new();
        f.lifecycle(true);
        f.root.simulate_input("/session");
        f.paint();
        f.root.simulate_keystroke(key);
        f.paint();
        assert_eq!(f.draft("a"), "/session-name");
        assert!(f.sent().is_empty(), "extension invocation cannot be queued");
        f.assert_focus("a");
        f.lifecycle(false);
        f.root.simulate_keystroke(key);
        let request = f.prompt();
        assert_eq!(request.2["message"], "/session-name");
        assert!(
            request.2["streamingBehavior"].is_null(),
            "idle follow-up is ordinary submission"
        );
    }
}

#[test]
fn root_remount_and_session_switch_keep_exact_drafts_editors_and_catalogs() {
    let mut f = Fixture::new();
    f.root.simulate_input("/sum");
    f.paint();
    let retained = f
        .root
        .read(|_, cx| f.shell.read(cx).sessions["a"].composer.read(cx).editor());
    f.root
        .update(|_, w, cx| f.shell.update(cx, |s, cx| s.mount(w, cx)));
    f.paint();
    f.assert_focus("a");
    assert_eq!(f.draft("a"), "/sum");
    f.root.simulate_keystroke("tab");
    f.root.simulate_input("  London 🦀\nsecond line  ");
    let exact = "/summarize   London 🦀\nsecond line  ";
    assert_eq!(f.draft("a"), exact);
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.create_retained(
                "b".into(),
                project(),
                String::new(),
                String::new(),
                "/".into(),
                w,
                cx,
            );
            s.start_live("b", cx);
            started(s, "b", 8, cx);
            s.sync(w, cx);
        });
    });
    f.paint();
    f.root.update(|_, w, cx| {
        assert!(
            w.try_find("command-menu").is_none(),
            "loading B must not expose A's catalog"
        );
        assert!(
            f.shell.read(cx).sessions["b"]
                .controller
                .state()
                .commands
                .is_empty()
        );
    });
    assert_eq!(f.draft("a"), exact);
    f.complete_reads(
        "b",
        "saved-b",
        json!([
            {"name":"only-b", "description":"B catalog", "source":"prompt"}
        ]),
    );
    f.select("b");
    f.root.update(|_, w, cx| {
        assert!(w.find("command-menu").visible());
        assert_eq!(
            f.shell.read(cx).sessions["b"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "only-b"
        );
        assert!(w.try_find("command-option-1").is_none());
    });
    f.root.simulate_keystroke("tab");
    assert_eq!(f.draft("b"), "/only-b ");
    f.select("a");
    assert_eq!(f.draft("a"), exact);
    f.root.read(|_, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"].composer.read(cx).editor(),
            retained
        )
    });
    // Native select-all replaces the retained draft; no host-side catalog/editor injection.
    f.root.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    f.root.simulate_input("/");
    f.paint();
    f.root.read(|_, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "session-name"
        )
    });
    assert!(f.sent().is_empty());
    f.select("b");
    assert_eq!(f.draft("b"), "/only-b ");
    drop(retained);
}

#[test]
fn root_get_commands_reply_requires_runtime_generation_and_current_request() {
    let mut f = Fixture::new();
    f.root.simulate_input("/");
    f.paint();
    f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, cx| {
            let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
            s.effects("a", effects, cx);
        });
    });
    let reads = f.sent();
    assert!(
        !reads.iter().any(|r| r.2["type"] == "get_commands"),
        "same-identity refresh starts with get_state, not a fabricated catalog read"
    );
    for request in reads {
        f.respond(
            &request,
            Fixture::read_data(&request, "replacement-a", &catalog()),
        );
    }
    let mut command_request = None;
    for request in f.sent() {
        if request.2["type"] == "get_commands" {
            assert!(command_request.replace(request).is_none());
        } else {
            f.respond(
                &request,
                Fixture::read_data(&request, "replacement-a", &catalog()),
            );
        }
    }
    let request = command_request.expect("identity change must issue actual get_commands");
    let new_catalog = json!([{ "name":"new-only", "source":"prompt" }]);
    let reply = response(&request, json!({"commands":new_catalog}));
    for (runtime, generation) in [("b", 8), ("a", 7), ("a", 9)] {
        f.ingest_at(runtime, generation, reply.clone());
        f.root.read(|_, cx| {
            assert!(
                !f.shell.read(cx).sessions["a"]
                    .controller
                    .state()
                    .commands
                    .iter()
                    .any(|c| c["name"] == "new-only")
            )
        });
    }
    let mut unknown = reply.clone();
    unknown["id"] = json!("not-a-request");
    f.ingest(unknown);
    f.root.read(|_, cx| {
        assert!(
            !f.shell.read(cx).sessions["a"]
                .controller
                .state()
                .commands
                .iter()
                .any(|c| c["name"] == "new-only")
        )
    });
    f.ingest(reply);
    f.root.update(|_, w, cx| {
        assert!(w.find("command-menu").visible());
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "new-only"
        );
    });
    f.respond(&request, json!({"commands":catalog()}));
    f.root.read(|_, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"]
                .composer
                .read(cx)
                .active_command()
                .unwrap()
                .name,
            "new-only",
            "consumed request cannot replace the catalog"
        )
    });
    f.root.simulate_keystroke("tab");
    assert_eq!(f.draft("a"), "/new-only ");
    assert!(f.sent().is_empty());
    f.assert_focus("a");
}
