use super::*;
use crate::app::tests::{app, project, rpc, shell, started};
use gpui_kit::{MouseButton, TestAppWindow, point, rgb, size, test::TestWindowExt};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

pub(super) struct MissReader;
impl Reader for MissReader {
    fn begin(&self) -> u64 {
        0
    }
    fn cancel(&self) {}
    fn read(&self, _: &Job) -> Option<Vec<serde_json::Value>> {
        None
    }
}
fn messages() -> Vec<serde_json::Value> {
    vec![json!({"role":"user", "timestamp":1, "content":"offline history"})]
}
fn binding(ticket: u64, remote: bool) -> Binding {
    let mut c = Controller::new("a");
    c.bind_source("a", "/fixture/a.jsonl");
    Binding {
        ticket,
        runtime: "a".into(),
        project: if remote { "ssh:fake" } else { "/fixture" }.into(),
        id: "a".into(),
        path: "/fixture/a.jsonl".into(),
        attempt: 1,
        token: c.saved_preview_token().unwrap(),
    }
}
struct SlowReader {
    generation: AtomicU64,
    cancellations: AtomicUsize,
    entered: std::sync::mpsc::Sender<u64>,
}
impl Reader for SlowReader {
    fn begin(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }
    fn cancel(&self) {
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
    fn read(&self, job: &Job) -> Option<Vec<serde_json::Value>> {
        let _ = self.entered.send(job.binding.ticket);
        if job.binding.ticket == 1 {
            while self.generation.load(Ordering::SeqCst) == job.read_generation {
                std::thread::sleep(Duration::from_millis(1));
            }
            None
        } else {
            Some(messages())
        }
    }
}
#[test]
fn bounded_worker_cancels_local_and_remote_without_pi_fifo_or_reply_deadlock() {
    for remote in [false, true] {
        let (entered, observed) = std::sync::mpsc::channel();
        let reader = Arc::new(SlowReader {
            generation: AtomicU64::new(0),
            cancellations: AtomicUsize::new(0),
            entered,
        });
        let (worker, replies) = Worker::launch(reader.clone());
        worker.submit(binding(1, remote));
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        worker.cancel();
        worker.submit(binding(2, remote));
        let stale = replies.recv_blocking().unwrap();
        assert_eq!(stale.binding.ticket, 1);
        assert!(stale.messages.is_none());
        let fresh = replies.recv_blocking().unwrap();
        assert_eq!(fresh.binding.ticket, 2);
        assert_eq!(fresh.messages, Some(messages()));
        // Replies may be full: UI cancellation and replacement never block on them.
        worker.submit(binding(3, remote));
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 2);
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), 3);
        worker.cancel();
        worker.submit(binding(4, remote));
        assert!(worker.pending.len() <= 1);
        replies.close();
        drop(worker);
        assert!(reader.cancellations.load(Ordering::SeqCst) >= 3);
    }
}

fn painted_user_glyphs(
    root: &mut TestAppWindow<Root>,
    index: usize,
    expected: &str,
) -> (
    gpui_kit::Point<gpui_kit::Pixels>,
    gpui_kit::Point<gpui_kit::Pixels>,
) {
    root.update(|root, w, cx| {
        let owner = root
            .view()
            .clone()
            .downcast::<Shell>()
            .expect("actual Shell root");
        let shell = owner.read(cx);
        let texts = crate::components::transcript::renderer_transcript_tests::cached_prose(
            shell.sessions[&shell.active].transcript.read(cx),
        );
        let text = texts
            .get(index)
            .expect("mounted saved prose source")
            .1
            .read(cx);
        assert_eq!(text.rendered_text().as_str().trim(), expected);
        let slot = w.find("shell-transcript-slot").bounds();
        assert!(slot.size.height > px(0.));
        assert!(slot.bottom() <= w.find("shell-composer-area").bounds().top());
        let mut bubbles: Vec<_> = w
            .painted_quads()
            .into_iter()
            .filter(|q| {
                q.background == rgb(0xebeef0).into()
                    && slot.contains(&q.bounds.map(|p| px(p.as_f32() / w.scale_factor())).center())
            })
            .map(|q| q.bounds.map(|p| px(p.as_f32() / w.scale_factor())))
            .collect();
        bubbles.sort_by(|a, b| a.top().partial_cmp(&b.top()).unwrap());
        let bubble = bubbles
            .get(index)
            .expect("saved user bubble must actually paint inside the transcript viewport");
        let bounds = text.bounds();
        assert!(bounds.size.width > px(0.), "saved prose width: {bounds:?}");
        assert!(bounds.size.height > px(0.));
        assert!(bubble.contains(&bounds.center()));
        assert!(slot.contains(&bounds.center()));
        let pointer = |offset| {
            (0..=(f32::from(bounds.size.width) * 4.) as usize)
                .map(|step| point(bounds.left() + px(step as f32 / 4.), bounds.center().y))
                .find(|p| text.rendered_offset_at(*p) == Some(offset))
                .unwrap_or_else(|| panic!("no painted glyph hit for offset {offset}: {bounds:?}"))
        };
        (pointer(0), pointer(expected.len()))
    })
}

fn painted_user_text(root: &mut TestAppWindow<Root>, index: usize, expected: &str) {
    let (start, end) = painted_user_glyphs(root, index, expected);
    root.simulate_mouse_move(start);
    root.simulate_mouse_down(start, MouseButton::Left);
    root.simulate_event(gpui_kit::MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    root.simulate_mouse_up(end, MouseButton::Left);
    root.draw();
    root.update(|_, w, cx| {
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(w, cx).trim(),
            expected,
            "saved history must be readable through painted glyph hit testing"
        );
    });
    root.simulate_keystroke(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    root.update(|_, _, cx| {
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref()
                .map(str::trim),
            Some(expected),
            "saved history Copy must use the glyph-selected source"
        );
    });
}

#[test]
fn actual_root_saved_preview_selects_graphemes_without_pi_readiness() {
    let mut app = app();
    let (commands, _requests) = async_channel::bounded(1024);
    let owner = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(owner.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(700.)));
    root.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    let prose = "café e\u{301} 👩‍💻 history";
    root.update(|_, w, cx| {
        owner.update(cx, |s, cx| {
            let p = project();
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![p.clone()],
                active_project_path: p.path.clone(),
                pi_path: None,
            });
            s.restored = true;
            s.mount(w, cx);
            s.create_live(
                "a".into(),
                p,
                "a".into(),
                "/fixture/a.jsonl".into(),
                "keep draft".into(),
                w,
                cx,
            );
            let binding = s.saved_history.binding.clone().unwrap();
            s.saved_history_reply(
                Reply {
                    binding,
                    messages: Some(vec![json!({"role":"user", "timestamp":1, "content":prose})]),
                },
                cx,
            );
            s.sync(w, cx);
            assert!(!s.sessions["a"].controller.state().ready);
        });
    });
    root.draw();
    painted_user_text(&mut root, 0, prose);
    root.update(|_, _, cx| {
        assert_eq!(
            owner.read(cx).sessions["a"].controller.state().draft,
            "keep draft"
        );
        assert!(!owner.read(cx).sessions["a"].controller.state().ready);
    });
}

#[test]
fn actual_root_preview_occurrences_preserve_painted_selection_through_rpc_and_empty_reset() {
    for remote in [false, true] {
        for before_start in [false, true] {
            for identical_text in [false, true] {
                let mut app = app();
                let (commands, requests) = async_channel::bounded(1024);
                let owner = app.new_entity(|_| shell(commands));
                let mut root = app.open_window(|w, cx| Root::new(owner.clone(), w, cx));
                root.simulate_resize(size(px(1000.), px(700.)));
                root.update(|_, w, cx| {
                    w.activate_window();
                    w.bounds_changed(cx);
                });
                let old = json!({"role":"user", "timestamp":1, "content":"old"});
                let new = if identical_text {
                    json!({"role":"user", "timestamp":1,
                        "content":[{"type":"text", "text":"old"}]})
                } else {
                    json!({"role":"user", "timestamp":1, "content":"new"})
                };
                let (stale, ids) = root.update(|_, w, cx| {
                    owner.update(cx, |s, cx| {
                        let mut p = project();
                        if remote {
                            p.connection_string = Some("fake-host".into());
                        }
                        s.workspace = Some(WorkspaceSnapshot {
                            projects: vec![p.clone()],
                            active_project_path: p.path.clone(),
                            pi_path: None,
                        });
                        s.restored = true;
                        s.window = Some(w.window_handle());
                        s.mount(w, cx);
                        s.create_live(
                            "a".into(),
                            p,
                            "a".into(),
                            "/fixture/a.jsonl".into(),
                            "keep draft".into(),
                            w,
                            cx,
                        );
                        let binding = s.saved_history.binding.clone().unwrap();
                        if !before_start {
                            started(s, "a", 7, cx);
                        }
                        s.saved_history_reply(
                            Reply {
                                binding: binding.clone(),
                                messages: Some(vec![old.clone()]),
                            },
                            cx,
                        );
                        if before_start {
                            started(s, "a", 7, cx);
                        }
                        s.ingest(
                            rpc("a", 7, json!({"type":"message_start", "message":new})),
                            cx,
                        );
                        let stale_request = std::iter::from_fn(|| requests.try_recv().ok()).find_map(|command| match command {
                            Command::Send { request, .. } if request["type"] == "get_messages" => Some(request),
                            _ => None,
                        }).unwrap();
                        s.ingest(rpc("a", 7, json!({"type":"response", "id":stale_request["id"],
                            "command":"get_messages", "success":true, "data":{"messages":[old.clone()]}})), cx);
                        s.sync(w, cx);
                        let rows = &s.sessions["a"].controller.state().transcript;
                        assert_eq!(rows.len(), 2);
                        assert_ne!(rows[0].id, rows[1].id);
                        assert_eq!(rows[0].source, "user:1:occ0");
                        assert_eq!(rows[1].source, "user:1:occ1");
                        (
                            binding,
                            rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                        )
                    })
                });
                root.draw();
                painted_user_text(&mut root, 0, "old");
                let selected = if identical_text { "old" } else { "new" };
                painted_user_text(&mut root, 1, selected);
                for empty in [false, true] {
                    while requests.try_recv().is_ok() {}
                    root.update(|_, w, cx| owner.update(cx, |s, cx| {
                        let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
                        let state_request = effects.iter().find_map(|e| match e {
                            Effect::Send(v) if v["type"] == "get_state" => Some(v.clone()), _ => None,
                        }).unwrap();
                        s.effects("a", effects, cx);
                        s.ingest(rpc("a", 7, json!({"type":"response", "id":state_request["id"],
                            "command":"get_state", "success":true, "data":{
                                "sessionId":"a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}})), cx);
                        let messages_request = std::iter::from_fn(|| requests.try_recv().ok()).filter_map(|c| match c {
                            Command::Send { request, .. } if request["type"] == "get_messages" => Some(request),
                            _ => None,
                        }).last().unwrap();
                        let messages = if empty { vec![] } else { vec![old.clone(), new.clone()] };
                        s.ingest(rpc("a", 7, json!({"type":"response", "id":messages_request["id"],
                            "command":"get_messages", "success":true, "data":{"messages":messages}})), cx);
                        s.saved_history_reply(Reply { binding: stale.clone(), messages: Some(vec![old.clone()]) }, cx);
                        s.sync(w, cx);
                        let live = &s.sessions["a"];
                        assert_eq!(live.source_id, "a");
                        assert_eq!(live.source_path, "/fixture/a.jsonl");
                        assert_eq!(live.controller.state().draft, "keep draft");
                        assert!(!live.controller.state().durable);
                        assert_eq!(live.controller.state().transcript.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                            if empty { vec![] } else { ids.clone() });
                    }));
                    root.draw();
                    root.update(|_, w, cx| {
                        assert_eq!(
                            gpui_kit::base::TextSelection::selected_text(w, cx).trim(),
                            if empty { "" } else { selected }
                        )
                    });
                }
                root.update(|_, w, cx| {
                    owner.update(cx, |s, cx| {
                        s.ingest(
                            rpc(
                                "a",
                                7,
                                json!({"type":"message_start", "message":{
                        "role":"user", "timestamp":1, "content":"after empty"}}),
                            ),
                            cx,
                        );
                        s.sync(w, cx);
                        assert_eq!(
                            s.sessions["a"].controller.state().transcript[0].source,
                            "user:1:occ0"
                        );
                        assert!(
                            !ids.contains(&s.sessions["a"].controller.state().transcript[0].id)
                        );
                    })
                });
                root.draw();
                painted_user_text(&mut root, 0, "after empty");
                while let Ok(command) = requests.try_recv() {
                    assert!(!matches!(command, Command::Register { .. }));
                }
            }
        }
    }
}

#[test]
fn actual_root_missing_pi_feedback_remains_truthful_and_dismissible_over_saved_history() {
    for remote in [false, true] {
        let mut app = app();
        let (commands, _requests) = async_channel::bounded(1024);
        let owner = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(owner.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        let error = root.update(|_, w, cx| {
            owner.update(cx, |s, cx| {
                let mut p = project();
                if remote {
                    p.connection_string = Some("fake-host".into());
                }
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![p.clone()],
                    active_project_path: p.path.clone(),
                    pi_path: None,
                });
                s.restored = true;
                s.window = Some(w.window_handle());
                s.mount(w, cx);
                s.create_live(
                    "a".into(),
                    p,
                    "a".into(),
                    "/fixture/a.jsonl".into(),
                    "keep draft".into(),
                    w,
                    cx,
                );
                let binding = s.saved_history.binding.clone().unwrap();
                s.ingest(
                    Event::Started {
                        runtime: "a".into(),
                        attempt: s.sessions["a"].start_attempt,
                        result: Err(()),
                    },
                    cx,
                );
                s.saved_history_reply(
                    Reply {
                        binding,
                        messages: Some(messages()),
                    },
                    cx,
                );
                s.sync(w, cx);
                s.sessions["a"].controller.state().error.clone().unwrap()
            })
        });
        root.activate_accessibility();
        root.draw();
        painted_user_glyphs(&mut root, 0, "offline history");
        let tree = root.accessibility_tree().unwrap();
        let body = tree
            .nodes
            .iter()
            .filter_map(|(_, n)| n.value().or(n.label()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            body.contains(&error),
            "feedback must expose the real failure body: {body:?}"
        );
        let close = root.update(|_, w, cx| {
            assert!(w.find("dialog").visible());
            assert!(
                owner
                    .read(cx)
                    .fallback
                    .as_ref()
                    .unwrap()
                    .1
                    .read(cx)
                    .is_open()
            );
            w.find("close").bounds().center()
        });
        if remote {
            root.simulate_keystroke("escape");
        } else {
            root.simulate_click(close, MouseButton::Left);
        }
        root.draw();
        root.update(|_, _, cx| {
            let s = owner.read(cx);
            assert!(!s.fallback.as_ref().unwrap().1.read(cx).is_open());
            assert_eq!(
                s.sessions["a"].controller.state().error.as_deref(),
                Some(error.as_str())
            );
            assert!(!s.sessions["a"].controller.state().ready);
            assert!(!s.sessions["a"].controller.state().durable);
        });
        painted_user_text(&mut root, 0, "offline history");
    }
}

#[test]
fn actual_root_saved_preview_cancels_loader_survives_missing_pi_and_fences_navigation() {
    for (remote, failed_start) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(1024);
        let owner = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(owner.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        let first = root.update(|_, w, cx| {
            owner.update(cx, |s, cx| {
                let mut p = project();
                if remote {
                    p.connection_string = Some("fake-host".into());
                }
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![p.clone()],
                    active_project_path: p.path.clone(),
                    pi_path: None,
                });
                s.restored = true;
                s.window = Some(w.window_handle());
                s.mount(w, cx);
                s.create_live(
                    "a".into(),
                    p.clone(),
                    "a".into(),
                    "/fixture/a.jsonl".into(),
                    "keep draft".into(),
                    w,
                    cx,
                );
                let first = s.saved_history.binding.clone().unwrap();
                s.create_live(
                    "b".into(),
                    p,
                    "b".into(),
                    "/fixture/b.jsonl".into(),
                    String::new(),
                    w,
                    cx,
                );
                s.select("a", w, cx);
                s.saved_history_reply(
                    Reply {
                        binding: first.clone(),
                        messages: Some(messages()),
                    },
                    cx,
                );
                assert!(s.sessions["a"].controller.state().transcript.is_empty());
                first
            })
        });
        app.advance_clock(Duration::from_millis(250));
        app.run_until_parked();
        root.draw();
        root.update(|_, w, cx| {
            assert!(w.find("session-loading-label").visible());
            owner.update(cx, |s, cx| {
                let binding = s.saved_history.binding.clone().unwrap();
                assert_ne!(first.ticket, binding.ticket);
                // A read miss is cosmetic and does not manufacture a new incident.
                s.saved_history_reply(
                    Reply {
                        binding: binding.clone(),
                        messages: None,
                    },
                    cx,
                );
                assert!(s.sessions["a"].controller.state().error.is_none());
                if failed_start {
                    s.ingest(
                        Event::Started {
                            runtime: "a".into(),
                            attempt: s.sessions["a"].start_attempt,
                            result: Err(()),
                        },
                        cx,
                    );
                    assert!(s.sessions["a"].controller.state().error.is_some());
                }
                s.saved_history_reply(
                    Reply {
                        binding,
                        messages: Some(messages()),
                    },
                    cx,
                );
                assert_eq!(s.sessions["a"].controller.state().draft, "keep draft");
                assert_eq!(s.sessions["a"].source_id, "a");
                assert!(!s.sessions["a"].controller.state().ready);
                s.sync(w, cx);
            });
        });
        root.draw();
        painted_user_glyphs(&mut root, 0, "offline history");
        if failed_start {
            let close = root.update(|_, w, cx| {
                assert!(
                    owner
                        .read(cx)
                        .fallback
                        .as_ref()
                        .unwrap()
                        .1
                        .read(cx)
                        .is_open()
                );
                assert!(w.find("dialog").visible());
                w.find("close").bounds().center()
            });
            root.simulate_click(close, MouseButton::Left);
            root.draw();
            root.update(|_, _, cx| {
                assert!(
                    !owner
                        .read(cx)
                        .fallback
                        .as_ref()
                        .unwrap()
                        .1
                        .read(cx)
                        .is_open()
                );
                assert!(
                    owner.read(cx).sessions["a"]
                        .controller
                        .state()
                        .error
                        .is_some()
                );
            });
        }
        painted_user_text(&mut root, 0, "offline history");
        root.update(|_, w, cx| {
            assert!(w.try_find("session-loading-label").is_none());
            owner.update(cx, |s, cx| {
                // Successful startup must retain preview until real RPC hydration wins.
                if !failed_start {
                    started(s, "a", 7, cx);
                    assert_eq!(s.sessions["a"].controller.state().transcript.len(), 1);
                    s.sessions
                        .get_mut("a")
                        .unwrap()
                        .controller
                        .transport_failed(7);
                }
                s.sync(w, cx);
                assert_eq!(s.sessions["a"].controller.state().transcript.len(), 1);
                assert_eq!(s.sessions["a"].controller.state().draft, "keep draft");
            });
        });
        while let Ok(command) = requests.try_recv() {
            assert!(!matches!(command, Command::Register { .. }));
        }
    }
}
