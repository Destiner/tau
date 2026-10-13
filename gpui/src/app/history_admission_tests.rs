use super::tests::{app, create, project, rpc, shell, started};
use super::*;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, size, test::TestWindowExt};
use serde_json::{Value, json};

struct Fixture {
    root: TestAppWindow<Root>,
    shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    app: TestApp,
}
impl Fixture {
    fn new() -> Self {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(1024);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.activate_accessibility();
        root.update(|_, w, cx| {
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| {
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.restored = true;
                s.window = Some(w.window_handle());
                s.mount(w, cx);
                create(s, "a", "precious draft", w, cx);
                started(s, "a", 7, cx);
                s.sync(w, cx);
            });
        });
        let mut f = Self {
            app,
            root,
            shell,
            requests,
        };
        // Hydrate through the real startup requests, not a parallel UI fixture.
        while let Ok(command) = f.requests.try_recv() {
            if let Command::Send { request, .. } = command {
                let data = match request["type"].as_str().unwrap() {
                    "get_state" => json!({"sessionId":"a", "isStreaming":false}),
                    "get_messages" => json!({"messages":[
                        {"role":"compactionSummary", "timestamp":5, "summary":"context"},
                        {"role":"user", "timestamp":3, "content":"recent hidden"},
                        {"role":"user", "timestamp":4, "content":"current"}
                    ]}),
                    "get_available_models" | "get_scoped_models" => json!({"models":[]}),
                    "get_commands" => json!({"commands":[]}),
                    "get_available_thinking_levels" => json!({"levels":[]}),
                    other => panic!("unexpected startup request {other}"),
                };
                f.reply(&request, data);
            }
        }
        f.paint();
        f.busy(false);
        f
    }
    fn paint(&mut self) {
        self.app.run_until_parked();
        self.root.draw();
    }
    fn reply(&mut self, request: &Value, data: Value) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(
                    rpc(
                        "a",
                        7,
                        json!({"type":"response", "id":request["id"],
                "command":request["type"], "success":true, "data":data}),
                    ),
                    cx,
                );
                s.sync(w, cx);
            })
        });
    }
    fn click(&mut self, key: usize) {
        let point = self
            .root
            .update(|_, w, _| w.find(format!("compaction-{key}")).bounds().center());
        self.root.simulate_mouse_move(point);
        self.root.simulate_click(point, MouseButton::Left);
    }
    fn busy(&mut self, expected: bool) {
        let tree = self.root.accessibility_tree().unwrap();
        let controls: Vec<_> = tree
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == gpui_kit::accesskit::Role::Button
                    && node
                        .label()
                        .is_some_and(|label| label.to_lowercase().contains("earlier"))
            })
            .collect();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].1.is_busy(), expected);
        assert_eq!(controls[0].1.is_disabled(), expected);
    }
    fn history_requests(&self) -> Vec<Value> {
        let mut requests = vec![];
        while let Ok(command) = self.requests.try_recv() {
            if let Command::Send { request, .. } = command {
                assert_eq!(request["type"], "get_entries", "unrelated RPC from divider");
                requests.push(request);
            }
        }
        requests
    }
}

#[test]
fn disconnected_root_divider_rejects_without_rpc_or_false_busy_and_preserves_work() {
    let mut f = Fixture::new();
    let before = f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let live = s.sessions.get_mut("a").unwrap();
            live.controller.transport_failed(7);
            live.dirty = true;
            live.projection_dirty = true;
            s.sync(w, cx);
            let state = s.sessions["a"].controller.state();
            assert!(state.transcript[0].history_available);
            (
                state.transcript[0].revision,
                state.draft.clone(),
                state.error.clone(),
            )
        })
    });
    f.paint();
    let close = f.root.update(|_, w, _| w.find("close").bounds().center());
    f.root.simulate_click(close, MouseButton::Left);
    f.paint();
    // The connection incident may move the retained reader; navigate back to the divider.
    let viewport = f
        .root
        .update(|_, w, _| w.find("shell-transcript-slot").bounds());
    f.root
        .simulate_scroll(viewport.center(), gpui_kit::point(px(0.), px(550.)));
    f.paint();
    for _ in 0..2 {
        f.click(0);
        f.paint();
        f.busy(false);
        assert!(f.history_requests().is_empty());
        f.root.update(|_, _, cx| {
            let live = &f.shell.read(cx).sessions["a"];
            let state = live.controller.state();
            assert!(!state.transcript[0].history_loading);
            assert_eq!(state.transcript[0].revision, before.0);
            assert_eq!(state.draft, before.1);
            assert_eq!(state.error, before.2);
            assert_eq!(
                live.composer.read(cx).editor().read(cx).value().as_ref(),
                "precious draft"
            );
        });
    }
}

#[test]
fn accepted_root_divider_stays_owned_until_reply_and_suppresses_duplicate_clicks() {
    let mut f = Fixture::new();
    f.click(0);
    // Exercise retained handlers before repaint as well as the disabled new paint.
    f.click(0);
    f.paint();
    f.busy(true);
    let requests = f.history_requests();
    assert_eq!(requests.len(), 1);
    f.click(0);
    f.paint();
    f.busy(true);
    assert!(f.history_requests().is_empty());
    f.root.update(|_, _, cx| {
        let state = f.shell.read(cx).sessions["a"].controller.state();
        assert!(state.transcript[0].history_loading);
        assert_eq!(state.draft, "precious draft");
    });
    f.reply(&requests[0], json!({"entries":[], "leafId":""}));
    f.paint();
    f.root.update(|_, _, cx| {
        let state = f.shell.read(cx).sessions["a"].controller.state();
        assert!(!state.transcript[0].history_loading);
        assert!(!state.transcript[0].history_available);
    });
    assert!(
        f.root
            .accessibility_tree()
            .unwrap()
            .nodes
            .iter()
            .all(|(_, n)| !n.is_busy())
    );
}

#[test]
fn cached_prefix_root_divider_reveals_without_rpc_or_lingering_busy() {
    let mut f = Fixture::new();
    f.click(0);
    f.paint();
    let request = f.history_requests().remove(0);
    f.reply(&request, json!({"entries":[
        {"type":"message","id":"a","parentId":null,"message":{"role":"user","timestamp":1,"content":"oldest"}},
        {"type":"message","id":"b","parentId":"a","message":{"role":"assistant","timestamp":2,"content":[{"type":"text","text":"older"}]}},
        {"type":"compaction","id":"c","parentId":"b","firstKeptEntryId":"b","summary":"context"},
        {"type":"message","id":"d","parentId":"c","message":{"role":"user","timestamp":3,"content":"recent hidden"}},
        {"type":"compaction","id":"e","parentId":"d","firstKeptEntryId":"d","summary":"context"},
        {"type":"message","id":"f","parentId":"e","message":{"role":"user","timestamp":4,"content":"current"}}
    ], "leafId":"f"}));
    f.paint();
    f.busy(false);
    // New prefix rows allocate after the three retained tail rows.
    f.click(3);
    f.paint();
    assert!(f.history_requests().is_empty());
    f.root.update(|_, _, cx| {
        let state = f.shell.read(cx).sessions["a"].controller.state();
        assert_eq!(state.transcript[0].text, "oldest");
        assert!(
            state
                .transcript
                .iter()
                .all(|entry| !entry.history_loading && !entry.history_available)
        );
        assert_eq!(state.draft, "precious draft");
    });
    assert!(
        f.root
            .accessibility_tree()
            .unwrap()
            .nodes
            .iter()
            .all(|(_, n)| !n.is_busy())
    );
}
