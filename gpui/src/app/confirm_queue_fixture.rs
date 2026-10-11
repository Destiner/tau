use super::*;
use crate::app::tests::{app, create, project, rpc, shell, started};
use gpui_kit::{MouseButton, TestApp, TestAppWindow, size, test::TestWindowExt};
use serde_json::{Value, json};

pub(super) struct Fixture {
    pub root: TestAppWindow<Root>,
    pub shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    // Entity handles must be dropped before the app.
    _app: TestApp,
}

impl Fixture {
    pub fn new() -> Self {
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
                create(s, "a", "", w, cx);
                started(s, "a", 8, cx);
                s.select("a", w, cx);
            });
        });
        let mut f = Self {
            root,
            shell,
            requests,
            _app: app,
        };
        f.complete_reads(false);
        f.root
            .update(|_, _, cx| assert!(f.shell.read(cx).sessions["a"].controller.state().ready));
        f.sent();
        f.root.draw();
        f
    }

    pub fn complete_reads(&mut self, streaming: bool) {
        for _ in 0..16 {
            let requests = self.sent();
            if requests.is_empty() {
                return;
            }
            for request in requests {
                let data = match request["type"].as_str() {
                    Some("get_state") => {
                        json!({"sessionId":"saved-a", "sessionFile":"/fixture/a.jsonl", "isStreaming":streaming})
                    }
                    Some("get_messages") => json!({"messages":[]}),
                    Some("get_available_models" | "get_scoped_models") => json!({"models":[]}),
                    Some("get_available_thinking_levels") => json!({"levels":["off", "high"]}),
                    Some("get_commands") => json!({"commands":[]}),
                    other => panic!("unexpected bootstrap request: {other:?}"),
                };
                self.respond(&request, true, data);
            }
        }
        panic!("bootstrap did not settle");
    }

    pub fn sent(&self) -> Vec<Value> {
        std::iter::from_fn(|| self.requests.try_recv().ok())
            .filter_map(|command| match command {
                Command::Send {
                    runtime,
                    generation,
                    request,
                } => {
                    assert_eq!(runtime, "a");
                    assert_eq!(generation, 8);
                    Some(request)
                }
                _ => None,
            })
            .collect()
    }

    pub fn ingest(&mut self, event: Value) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(rpc("a", 8, event), cx);
                s.sync(w, cx);
            });
        });
        self.root.draw();
    }

    pub fn respond(&mut self, request: &Value, success: bool, data: Value) {
        self.ingest(json!({"type":"response", "id":request["id"],
            "command":request["type"], "success":success, "data":data,
            "error":if success { "" } else { "fixture failure" }}));
    }

    pub fn click(&mut self, id: &'static str) {
        self.root.draw();
        let at = self.root.update(|_, w, _| w.find(id).bounds().center());
        self.root.simulate_click(at, MouseButton::Left);
        self.root.draw();
    }

    pub fn queue(&mut self, steering: &[&str], follow_up: &[&str]) {
        self.ingest(json!({"type":"queue_update", "steering":steering, "followUp":follow_up}));
    }

    pub fn dialog(&mut self, method: &str) {
        self.ingest(json!({"type":"extension_ui_request", "id":"question",
            "method":method, "title":"Choose", "message":"Choose an answer",
            "options":["First", "Second"]}));
        self.root.update(|_, w, cx| {
            let dialog = self.shell.read(cx).sessions["a"]
                .extension
                .as_ref()
                .unwrap()
                .1
                .clone();
            dialog.update(cx, |dialog, cx| dialog.focus(w, cx));
        });
        self.root.draw();
        self.sent();
    }
}

pub(super) fn secondary(shift: bool) -> &'static str {
    match (cfg!(target_os = "macos"), shift) {
        (true, false) => "cmd-enter",
        (true, true) => "cmd-shift-enter",
        (false, false) => "ctrl-enter",
        (false, true) => "ctrl-shift-enter",
    }
}
