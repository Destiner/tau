use super::tests::{app, create, project, rpc, shell, started};
use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, size};
use serde_json::{Value, json};

struct QueueShortcutFixture {
    root: TestAppWindow<Root>,
    shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    // The app must outlive its entity handles during fixture teardown.
    _app: TestApp,
}

impl QueueShortcutFixture {
    fn new() -> Self {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(256);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| {
                s.window = Some(w.window_handle());
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.mount(w, cx);
            });
        });
        let mut fixture = Self {
            _app: app,
            root,
            shell,
            requests,
        };
        fixture.add_working_session("a", 8);
        fixture
    }

    fn add_working_session(&mut self, id: &str, generation: u64) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                create(s, id, "", w, cx);
                started(s, id, generation, cx);
            })
        });
        let state = self
            .sent()
            .into_iter()
            .find(|(_, _, r)| r["type"] == "get_state")
            .unwrap();
        self.respond(
            &state,
            json!({"sessionId":format!("saved-{id}"),
            "sessionFile":format!("/fixture/{id}.jsonl"), "isStreaming":false}),
        );
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(rpc(id, generation, json!({"type":"agent_start"})), cx);
                assert!(s.sessions[id].controller.state().ready);
                assert!(s.sessions[id].controller.state().working);
                s.sync(w, cx);
            })
        });
        self.sent();
        self.focus(id);
    }

    fn focus(&mut self, id: &str) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.select(id, w, cx);
                s.sync(w, cx);
                s.sessions[id]
                    .composer
                    .read(cx)
                    .editor()
                    .update(cx, |e, cx| e.focus(w, cx));
            })
        });
        self.root.draw();
    }

    fn sent(&self) -> Vec<(String, u64, Value)> {
        std::iter::from_fn(|| self.requests.try_recv().ok())
            .filter_map(|c| match c {
                Command::Send {
                    runtime,
                    generation,
                    request,
                } => Some((runtime, generation, request)),
                _ => None,
            })
            .collect()
    }

    fn respond(&mut self, request: &(String, u64, Value), data: Value) {
        self.root.update(|_, _, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(
                    rpc(
                        &request.0,
                        request.1,
                        json!({"type":"response",
                "id":request.2["id"], "command":request.2["type"], "success":true, "data":data}),
                    ),
                    cx,
                );
            })
        });
    }

    fn prompt(&mut self) -> (String, u64, Value) {
        let mut seen = Vec::new();
        for _ in 0..8 {
            self.root.draw();
            let requests = self.sent();
            for request in requests {
                seen.push(request.2.clone());
                match request.2["type"].as_str() {
                    Some("set_steering_mode") => {
                        assert_eq!(request.2["mode"], "all");
                        self.respond(&request, json!({}));
                    }
                    Some("set_follow_up_mode") => {
                        assert_eq!(request.2["mode"], "one-at-a-time");
                        self.respond(&request, json!({}));
                    }
                    Some("prompt") => return request,
                    _ => {}
                }
            }
        }
        panic!("editor submission did not produce a prompt: {seen:?}");
    }

    fn submit(&mut self, text: &str, key: &str) -> (String, u64, Value) {
        self.root.simulate_input(text);
        self.root.simulate_keystroke(key);
        self.prompt()
    }
}

fn secondary_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    }
}

fn assert_prompt(request: &(String, u64, Value), id: &str, text: &str, behavior: &str) {
    assert_eq!(request.0, id);
    assert_eq!(request.2["message"], text);
    assert_eq!(request.2["streamingBehavior"], behavior);
}

#[test]
fn working_secondary_enter_queues_follow_up_not_steering() {
    let mut f = QueueShortcutFixture::new();
    let request = f.submit("Later 🧑🏽‍💻 café", secondary_key());
    assert_prompt(&request, "a", "Later 🧑🏽‍💻 café", "followUp");
}

#[test]
fn remounted_composer_keeps_follow_up_intent() {
    let mut f = QueueShortcutFixture::new();
    let composer = f
        .root
        .update(|_, _, cx| f.shell.read(cx).sessions["a"].composer.clone());
    f.root
        .update(|_, w, cx| f.shell.update(cx, |s, cx| s.mount(w, cx)));
    f.focus("a");
    f.root
        .update(|_, _, cx| assert_eq!(composer, f.shell.read(cx).sessions["a"].composer));
    let request = f.submit("After remount", secondary_key());
    assert_prompt(&request, "a", "After remount", "followUp");
}

#[test]
fn idle_secondary_enter_sends_an_ordinary_prompt() {
    let mut f = QueueShortcutFixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.ingest(rpc("a", 8, json!({"type":"agent_settled"})), cx);
            assert!(!s.sessions["a"].controller.state().working);
            s.sync(w, cx);
        })
    });
    f.sent();
    let request = f.submit("Start now", secondary_key());
    assert_eq!(request.2["message"], "Start now");
    assert!(request.2.get("streamingBehavior").is_none());
}

#[test]
fn working_plain_enter_queues_steering() {
    let mut f = QueueShortcutFixture::new();
    let request = f.submit("Now 🚀", "enter");
    assert_prompt(&request, "a", "Now 🚀", "steer");
}

#[test]
fn shift_enter_inserts_newline_without_rpc() {
    let mut f = QueueShortcutFixture::new();
    f.root.simulate_input("First 🌍");
    f.root.simulate_keystroke("shift-enter");
    f.root.simulate_input("Second");
    f.root.draw();
    assert!(f.sent().is_empty(), "newline must not send RPC commands");
    let request = f.submit("", "enter");
    assert_prompt(&request, "a", "First 🌍\nSecond", "steer");
}

#[test]
fn primary_after_secondary_does_not_reuse_follow_up_intent() {
    let mut f = QueueShortcutFixture::new();
    let secondary = f.submit("Later 🦀", secondary_key());
    f.respond(&secondary, json!({}));
    let primary = f.submit("Now 🦀", "enter");
    assert_prompt(&primary, "a", "Now 🦀", "steer");
    assert_prompt(&secondary, "a", "Later 🦀", "followUp");
}

#[test]
fn sessions_do_not_share_secondary_submit_intent() {
    let mut f = QueueShortcutFixture::new();
    let secondary = f.submit("A later 🐚", secondary_key());
    f.respond(&secondary, json!({}));
    f.add_working_session("b", 9);
    let primary = f.submit("B now 🐚", "enter");
    assert_prompt(&primary, "b", "B now 🐚", "steer");
    f.focus("a");
    let primary = f.submit("A now 🐚", "enter");
    assert_prompt(&primary, "a", "A now 🐚", "steer");
    assert_prompt(&secondary, "a", "A later 🐚", "followUp");
}

#[test]
fn working_send_button_defaults_to_steering() {
    let mut f = QueueShortcutFixture::new();
    f.root.simulate_input("Button 🚀");
    f.root.draw();
    let position = f.root.update(|_, w, _| w.find("send").bounds().center());
    f.root.simulate_mouse_down(position, MouseButton::Left);
    f.root.simulate_mouse_up(position, MouseButton::Left);
    let request = f.prompt();
    assert_prompt(&request, "a", "Button 🚀", "steer");
}
