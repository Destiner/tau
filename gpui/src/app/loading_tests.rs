use super::*;
use crate::app::tests::{app, create, project, rpc, shell, started};
use gpui_kit::{TestApp, TestAppWindow, size, test::TestWindowExt};
use serde_json::json;

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
            });
        });
        Self {
            root,
            shell,
            requests,
            app,
        }
    }
    fn cold(&mut self, key: &str) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.create_live(
                    key.into(),
                    project(),
                    key.into(),
                    format!("/fixture/{key}.jsonl"),
                    String::new(),
                    w,
                    cx,
                );
            })
        });
        self.root.draw();
        self.app.run_until_parked();
    }
    fn start(&mut self, key: &str) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                started(s, key, 7, cx);
                s.sync(w, cx);
            })
        });
        self.root.draw();
    }
    fn advance(&mut self, millis: u64) {
        self.app.advance_clock(Duration::from_millis(millis));
        self.app.run_until_parked();
        self.root.draw();
    }
    fn hydrate(&mut self, key: &str, messages: serde_json::Value) {
        for _ in 0..128 {
            let Ok(command) = self.requests.try_recv() else {
                break;
            };
            if let Command::Send {
                runtime,
                generation,
                request,
            } = command
            {
                if runtime != key {
                    continue;
                }
                let data = match request["type"].as_str().unwrap() {
                    "get_state" => {
                        json!({"sessionId":key, "sessionFile":format!("/fixture/{key}.jsonl"), "isStreaming":false})
                    }
                    "get_messages" => json!({"messages":messages}),
                    "get_available_models" | "get_scoped_models" => json!({"models":[]}),
                    "get_commands" => json!({"commands":[]}),
                    "get_available_thinking_levels" => json!({"levels":[]}),
                    other => panic!("unexpected request {other}"),
                };
                self.root.update(|_, w, cx| self.shell.update(cx, |s, cx| {
                    s.ingest(rpc(&runtime, generation, json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data})), cx);
                    s.sync(w, cx);
                }));
            }
        }
        self.root.draw();
    }
    fn loading(&mut self, visible: bool) {
        self.root.update(|_, w, _| {
            assert!(w.find("session-loading").visible());
            assert_eq!(w.try_find("session-loading-label").is_some(), visible);
            assert_eq!(w.try_find("working-spinner").is_some(), visible);
            assert!(w.try_find("shell-composer-area").is_none());
            assert!(w.try_find("shell-transcript-slot").is_none());
        });
    }
}

#[test]
fn cold_history_has_reserved_space_and_exact_delayed_loading_then_real_transcript() {
    let mut f = Fixture::new();
    f.cold("a");
    f.loading(false);
    let bounds = f.root.update(|_, w, _| w.find("session-loading").bounds());
    f.advance(199);
    f.start("a");
    f.loading(false);
    f.advance(1);
    f.loading(true);
    f.root.update(|_, w, _| {
        assert_eq!(w.find("session-loading").bounds(), bounds);
        let spinner = w.find("working-spinner").bounds();
        let label = w.find("session-loading-label").bounds();
        assert_eq!(spinner.size, size(px(8.), px(8.)));
        assert_eq!(label.top() - spinner.bottom(), px(10.));
        let content = w.find("session-loading-content").bounds();
        let ideal_top = bounds.center().y - px(12.) - content.size.height / 2.;
        // Layout snaps absolute edges to device pixels, with half ties toward zero.
        let scale = w.scale_factor();
        let snapped_top = px((f32::from(ideal_top) * scale - 0.5).ceil() / scale);
        assert_eq!(content.top(), snapped_top);
    });
    f.hydrate("a", json!([{"role":"user", "content":"retained history"}]));
    f.advance(300);
    f.root.update(|_, w, cx| {
        assert!(w.try_find("session-loading").is_none());
        assert!(w.find("shell-transcript-slot").bounds().size.height > px(0.));
        assert!(w.find("shell-composer-area").visible());
        assert_eq!(f.shell.read(cx).sessions["a"].transcript.read(cx).len(), 1);
    });
}

#[test]
fn fast_hydration_and_new_blank_sessions_never_flash_a_loader() {
    let mut f = Fixture::new();
    f.cold("a");
    f.start("a");
    f.hydrate("a", json!([]));
    f.advance(400);
    f.root.update(|_, w, cx| {
        assert!(w.try_find("session-loading").is_none());
        f.shell
            .update(cx, |s, cx| create(s, "blank", "saved draft", w, cx));
    });
    f.root.draw();
    f.advance(400);
    f.root.update(|_, w, _| {
        assert!(w.try_find("session-loading").is_none());
        assert!(w.find("shell-composer-area").visible());
    });
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.select("a", w, cx);
            // An already presented empty history does not flash during a later refresh.
            let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
            s.effects("a", effects, cx);
            s.sync(w, cx);
        })
    });
    f.advance(400);
    f.root
        .update(|_, w, _| assert!(w.try_find("session-loading").is_none()));
}

#[test]
fn loading_timer_is_selection_and_start_attempt_fenced_and_errors_end_the_wait() {
    let mut f = Fixture::new();
    f.cold("a");
    f.advance(150);
    f.cold("b");
    f.advance(50);
    f.loading(false);
    f.advance(150);
    f.loading(true);
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let attempt = s.sessions["b"].start_attempt;
            s.ingest(
                Event::Started {
                    runtime: "b".into(),
                    attempt,
                    result: Err(()),
                },
                cx,
            );
            s.sync(w, cx);
        })
    });
    f.root.draw();
    f.root.update(|_, w, _| {
        assert!(w.try_find("session-loading").is_none());
        assert!(w.find("retry-pi").visible());
    });
    f.advance(500);
    f.root
        .update(|_, w, _| assert!(w.try_find("session-loading-label").is_none()));
}
