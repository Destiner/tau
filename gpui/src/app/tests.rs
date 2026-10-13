use super::*;
use gpui_kit::TestApp;
use serde_json::json;
use tau_core::pi::PiEvent;

pub(super) fn shell(commands: async_channel::Sender<Command>) -> Shell {
    Shell {
        commands,
        workspace: None,
        workspace_mutations: workspace_mutations::State::default(),
        preferences: Preferences::default(),
        sessions: BTreeMap::new(),
        active: String::new(),
        sidebar: None,
        header: None,
        window: None,
        shell_focus: None,
        picker: None,
        picker_busy: false,
        subscriptions: vec![],
        dark: false,
        error: None,
        lifecycle: None,
        confirmation: None,
        quit_error: None,
        fallback: None,
        preview: file_preview::State::default(),
        save_revision: 0,
        exit_revision: None,
        requested_session: None,
        epoch: Instant::now(),
        loaded: true,
        save_failed: false,
        register_retries: vec![],
        empty_candidates: Default::default(),
        empty_disposals: Default::default(),
        pending_saves: BTreeMap::new(),
        restored: false,
        session_actions: session_actions::SessionActions::default(),
        palette: palette::State::default(),
        remote: remote::State::default(),
        admin: admin::State::default(),
        sidebar_state: sidebar::State::default(),
        session_loading: loading::State::default(),
    }
}
pub(super) fn project() -> ProjectSummary {
    ProjectSummary {
        path: "/test/project".into(),
        name: "Project".into(),
        working_directory: "/test/project".into(),
        connection_string: None,
        collapsed: false,
        selected: true,
        sessions: vec![],
    }
}
pub(super) fn app() -> TestApp {
    let mut app = TestApp::new();
    app.update(|cx| components::init(cx, false));
    app
}
pub(super) fn with_shell(
    capacity: usize,
    test: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>, async_channel::Receiver<Command>),
) {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(capacity);
    let shell = app.new_entity(|_| shell(commands));
    let root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let window: AnyWindowHandle = root.handle().into();
    app.update(|cx| {
        window
            .update(cx, |_, w, cx| {
                shell.update(cx, |s, cx| test(s, w, cx, requests))
            })
            .unwrap()
    });
}
pub(super) fn create(s: &mut Shell, id: &str, text: &str, w: &mut Window, cx: &mut Context<Shell>) {
    if s.workspace.is_none() {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
    }
    s.create_live(
        id.into(),
        project(),
        String::new(),
        String::new(),
        text.into(),
        w,
        cx,
    );
}
pub(super) fn started(s: &mut Shell, id: &str, generation: u64, cx: &mut Context<Shell>) {
    s.ingest(
        Event::Started {
            runtime: id.into(),
            attempt: s.sessions[id].start_attempt,
            result: Ok(generation),
        },
        cx,
    );
}
pub(super) fn rpc(runtime: &str, generation: u64, value: serde_json::Value) -> Event {
    Event::Pi(PiEvent {
        runtime_id: runtime.into(),
        generation,
        kind: "rpc",
        line: Some(value.to_string()),
        message: None,
        code: None,
    })
}
#[test]
fn reopened_surface_never_exposes_dropped_sidebar_handlers() {
    {
        let mut app = app();
        let (commands, _requests) = async_channel::bounded(64);
        let shell = app.new_entity(|_| shell(commands));
        let mut first = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        first.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.window = Some(w.window_handle());
                s.mount(w, cx);
                create(s, "a", "draft A", w, cx);
                create(s, "b", "draft B", w, cx);
                s.select("a", w, cx);
            });
        });
        first.draw();
        let (old_sidebar, old_header, editor) = app.read_entity(&shell, |s, cx| {
            (
                s.sidebar.as_ref().unwrap().downgrade(),
                s.header.as_ref().unwrap().downgrade(),
                s.sessions["a"].composer.read(cx).editor().entity_id(),
            )
        });
        first.update(|_, w, _| w.remove_window());
        app.update(|cx| {
            assert!(cx.windows().is_empty());
            let target = open(shell.clone(), cx).unwrap();
            assert!(old_sidebar.upgrade().is_none());
            assert!(old_header.upgrade().is_none());
            target
                .update(cx, |_, w, cx| {
                    let id = "session-status-b".to_string().into();
                    let stale = gpui_kit::base::test_support::find(w, &[], &id);
                    assert!(
                        stale.is_none(),
                        "first paint must not expose retained old chrome"
                    );
                    w.draw(cx).clear(cx);
                    let position = gpui_kit::base::test_support::find(w, &[], &id)
                        .unwrap()
                        .bounds()
                        .center();
                    w.dispatch_event(
                        gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                            button: gpui_kit::MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    w.dispatch_event(
                        gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                            button: gpui_kit::MouseButton::Left,
                            position,
                            modifiers: Default::default(),
                            click_count: 1,
                        }),
                        cx,
                    );
                })
                .unwrap();
        });
        app.read_entity(&shell, |s, cx| {
            assert_eq!(s.active, "b");
            assert_eq!(s.sessions["a"].controller.state().draft, "draft A");
            assert_eq!(s.sessions["b"].controller.state().draft, "draft B");
            assert_eq!(
                s.sessions["a"].composer.read(cx).editor().entity_id(),
                editor
            );
            assert_eq!(
                s.sessions["b"]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref(),
                "draft B"
            );
        });
        app.update(|cx| {
            let target = cx.windows()[0];
            target
                .update(cx, |_, w, cx| {
                    use gpui_kit::test::TestWindowExt;
                    w.click("session-status-a", cx);
                })
                .unwrap();
        });
        app.read_entity(&shell, |s, cx| {
            assert_eq!(s.active, "a");
            assert_eq!(
                s.sessions["a"].composer.read(cx).editor().entity_id(),
                editor
            );
            assert_eq!(
                s.sessions["a"]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref(),
                "draft A"
            );
        });
    }
}

#[test]
fn session_switches_retain_editor_transcript_and_local_drafts() {
    with_shell(64, |s, w, cx, _| {
        create(s, "a", "draft A", w, cx);
        let editor = s.sessions["a"].composer.read(cx).editor().entity_id();
        let transcript = s.sessions["a"].transcript.entity_id();
        create(s, "b", "draft B", w, cx);
        s.select("a", w, cx);
        assert_eq!(
            s.sessions["a"].composer.read(cx).editor().entity_id(),
            editor
        );
        assert_eq!(s.sessions["a"].transcript.entity_id(), transcript);
        assert_eq!(s.sessions["a"].controller.state().draft, "draft A");
        assert_eq!(s.sessions["b"].controller.state().draft, "draft B");
    });
}
#[test]
fn stale_start_and_protocol_callbacks_cannot_replace_newer_drafts() {
    with_shell(64, |s, w, cx, _| {
        create(s, "a", "latest", w, cx);
        s.sessions.get_mut("a").unwrap().start_attempt = 2;
        s.ingest(
            Event::Started {
                runtime: "a".into(),
                attempt: 1,
                result: Ok(8),
            },
            cx,
        );
        assert_eq!(s.sessions["a"].controller.generation(), 0);
        started(s, "a", 9, cx);
        s.ingest(
            rpc(
                "a",
                8,
                json!({"type":"extension_ui_request", "id":"stale-editor", "method":"set_editor_text", "text":"stale"}),
            ),
            cx,
        );
        assert_eq!(s.sessions["a"].controller.state().draft, "latest");
        s.ingest(rpc("a", 9, json!({"type":"extension_ui_request", "id":"editor", "method":"set_editor_text", "text":"extension draft"})), cx);
        assert_eq!(s.sessions["a"].controller.state().draft, "extension draft");
        assert_eq!(s.preferences_snapshot().drafts[0].text, "extension draft");
    });
}
#[test]
fn startup_extension_events_are_replayed_only_for_acknowledged_generation() {
    with_shell(64, |s, w, cx, _| {
        create(s, "a", "original", w, cx);
        s.ingest(
            rpc(
                "a",
                7,
                json!({"type":"extension_ui_request", "id":"old-editor", "method":"set_editor_text", "text":"old"}),
            ),
            cx,
        );
        s.ingest(
            rpc(
                "a",
                8,
                json!({"type":"extension_ui_request", "id":"early-editor", "method":"set_editor_text", "text":"early"}),
            ),
            cx,
        );
        s.ingest(rpc("a", 8, json!({"type":"extension_ui_request", "id":"early-dialog", "method":"input", "title":"Early"})), cx);
        assert_eq!(s.sessions["a"].startup_events.len(), 3);
        started(s, "a", 8, cx);
        assert_eq!(s.sessions["a"].controller.state().draft, "early");
        assert_eq!(
            s.sessions["a"].controller.state().extension_dialogs[0].id,
            "early-dialog"
        );
        assert!(s.sessions["a"].startup_events.is_empty());
    });
}
#[test]
fn startup_buffer_is_bounded_and_overflow_cancels_start() {
    with_shell(64, |s, w, cx, _| {
        create(s, "a", "keep", w, cx);
        for _ in 0..513 {
            s.ingest(rpc("a", 8, json!({"type":"unknown"})), cx);
        }
        assert!(!s.sessions["a"].starting);
        assert!(s.sessions["a"].startup_events.is_empty());
        s.ingest(
            Event::Started {
                runtime: "a".into(),
                attempt: 1,
                result: Ok(8),
            },
            cx,
        );
        assert_eq!(s.sessions["a"].controller.generation(), 0);
        assert_eq!(s.sessions["a"].controller.state().draft, "keep");
    });
}
#[test]
fn generation_bootstrap_after_idle_uses_current_deadline_clock() {
    with_shell(64, |s, w, cx, _| {
        s.epoch = Instant::now() - Duration::from_secs(60);
        create(s, "a", "keep", w, cx);
        started(s, "a", 8, cx);
        let live = s.sessions.get_mut("a").unwrap();
        let pending = live.controller.pending_count();
        assert!(pending > 0);
        live.controller.tick(60_001);
        assert_eq!(live.controller.pending_count(), pending);
        assert!(live.controller.state().error.is_none());
    });
}
#[test]
fn restoration_starts_only_selected_draft_and_keeps_metadata() {
    with_shell(64, |s, w, cx, requests| {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
        s.preferences.selected_draft = "a".into();
        s.preferences.drafts = vec![
            Draft {
                key: "a".into(),
                project: project().path,
                text: "A".into(),
                recovered: vec!["uncertain".into()],
                ..Default::default()
            },
            Draft {
                key: "b".into(),
                project: project().path,
                text: "B".into(),
                ..Default::default()
            },
        ];
        s.sync(w, cx);
        assert_eq!(s.active, "a");
        assert!(s.sessions["a"].starting);
        assert!(!s.sessions["b"].starting);
        assert_eq!(
            s.preferences_snapshot().drafts[0].recovered,
            vec!["uncertain"]
        );
        let mut starts = vec![];
        while let Ok(command) = requests.try_recv() {
            if let Command::Start { runtime, .. } = command {
                starts.push(runtime);
            }
        }
        assert_eq!(starts, vec!["a"]);
    });
}
#[test]
fn saturated_dispatch_recovers_request_and_does_not_acknowledge_draft_save() {
    with_shell(1, |s, w, cx, requests| {
        create(s, "a", "keep", w, cx);
        // The authoritative workspace selection occupies the one-slot queue first.
        assert!(matches!(
            requests.try_recv().unwrap(),
            Command::WorkspaceMutation { .. }
        ));
        s.retry_pi("a", cx);
        // The retried Start fills the channel. Bootstrap writes fail immediately, not after 10s.
        started(s, "a", 8, cx);
        assert_eq!(s.sessions["a"].controller.pending_count(), 0);
        s.sessions
            .get_mut("a")
            .unwrap()
            .controller
            .set_draft("new unsaved");
        s.persist(false, cx);
        assert!(s.save_failed);
        assert_eq!(s.sessions["a"].persisted_draft, "keep");
        assert!(s.pending_saves.is_empty());
        assert!(matches!(
            requests.try_recv().unwrap(),
            Command::Start { .. }
        ));
        s.persist(false, cx);
        let revision = s.save_revision;
        assert_eq!(s.sessions["a"].persisted_draft, "keep");
        s.ingest(
            Event::Saved {
                revision,
                success: true,
                stopped: vec![],
            },
            cx,
        );
        assert_eq!(s.sessions["a"].persisted_draft, "new unsaved");
    });
}
#[test]
fn registration_uses_captured_identity_without_selecting_it() {
    with_shell(64, |s, w, cx, requests| {
        create(s, "a", "retained work", w, cx);
        started(s, "a", 4, cx);
        create(s, "b", "", w, cx);
        s.effects(
            "a",
            vec![Effect::RegisterSession {
                runtime_id: "a".into(),
                generation: 4,
                id: "captured".into(),
                path: "/captured.jsonl".into(),
                name: "Captured".into(),
            }],
            cx,
        );
        assert_eq!(s.active, "b");
        let mut registered = false;
        while let Ok(command) = requests.try_recv() {
            if let Command::Register {
                runtime,
                generation,
                id,
                path,
                ..
            } = command
            {
                assert_eq!(
                    (runtime.as_str(), generation, id.as_str(), path.as_str()),
                    ("a", 4, "captured", "/captured.jsonl")
                );
                registered = true;
            }
        }
        assert!(registered);
    });
}
fn bootstrap_ready(
    s: &mut Shell,
    requests: &async_channel::Receiver<Command>,
    cx: &mut Context<Shell>,
) {
    let mut state = None;
    while let Ok(command) = requests.try_recv() {
        if let Command::Send { request, .. } = command
            && request["type"] == "get_state"
        {
            state = Some(request);
        }
    }
    let request = state.unwrap();
    s.ingest(
        rpc(
            "a",
            8,
            json!({"type":"response", "id":request["id"], "command":"get_state", "success":true,
        "data":{"sessionId":"saved-a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}}),
        ),
        cx,
    );
    assert!(s.sessions["a"].controller.state().ready);
}

#[test]
fn failed_write_removes_optimistic_row_and_reprojects_immediately() {
    with_shell(64, |s, w, cx, requests| {
        create(s, "a", "keep", w, cx);
        started(s, "a", 8, cx);
        bootstrap_ready(s, &requests, cx);
        s.composer_event("a", &ComposerEvent::Send("keep".into()), cx);
        let mut prompt = None;
        while let Ok(command) = requests.try_recv() {
            if let Command::Send { request, .. } = command
                && request["type"] == "prompt"
            {
                prompt = Some(request);
            }
        }
        s.sync(w, cx);
        assert_eq!(s.sessions["a"].transcript.read(cx).len(), 1);
        assert!(!s.sessions["a"].projection_dirty);
        s.ingest(
            Event::Written {
                runtime: "a".into(),
                generation: 8,
                request: prompt.unwrap(),
                success: false,
            },
            cx,
        );
        assert!(s.sessions["a"].projection_dirty);
        s.sync(w, cx);
        assert_eq!(s.sessions["a"].transcript.read(cx).len(), 0);
        assert_eq!(s.sessions["a"].controller.state().draft, "keep");
    });
}

#[test]
fn save_ack_records_only_the_text_actually_written() {
    with_shell(64, |s, w, cx, _| {
        create(s, "a", "initial", w, cx);
        let old_revision = s.save_revision;
        s.sessions
            .get_mut("a")
            .unwrap()
            .controller
            .set_draft("newer");
        s.persist(false, cx);
        let current_revision = s.save_revision;
        s.ingest(
            Event::Saved {
                revision: old_revision,
                success: true,
                stopped: vec![],
            },
            cx,
        );
        assert_eq!(s.sessions["a"].persisted_draft, "initial");
        assert_eq!(s.sessions["a"].controller.state().draft, "newer");
        s.ingest(
            Event::Saved {
                revision: current_revision,
                success: false,
                stopped: vec![],
            },
            cx,
        );
        assert_eq!(s.sessions["a"].persisted_draft, "initial");
        assert_eq!(s.sessions["a"].controller.state().draft, "newer");
        assert!(s.save_failed);
    });
}

#[test]
fn remount_retains_editor_and_routes_enter_exactly_once_in_new_window() {
    let mut app = app();
    let (commands, _requests) = async_channel::bounded(64);
    let shell = app.new_entity(|_| shell(commands));
    let received = Rc::new(std::cell::Cell::new(0));
    let events = received.clone();
    let first = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let first_handle: AnyWindowHandle = first.handle().into();
    let editor = app.update(|cx| {
        first_handle
            .update(cx, |_, w, cx| {
                shell.update(cx, |s, cx| {
                    create(s, "a", "keep", w, cx);
                    let composer = s.sessions["a"].composer.clone();
                    let subscription =
                        cx.subscribe(&composer, move |_, _, event: &ComposerEvent, _| {
                            if matches!(event, ComposerEvent::Send(_)) {
                                events.set(events.get() + 1);
                            }
                        });
                    s.sessions
                        .get_mut("a")
                        .unwrap()
                        ._subscriptions
                        .push(subscription);
                    let editor = s.sessions["a"].composer.read(cx).editor();
                    editor.update(cx, |editor, cx| editor.set_selected_range(1..3, cx));
                    editor
                })
            })
            .unwrap()
    });
    let second = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let second_handle: AnyWindowHandle = second.handle().into();
    app.update(|cx| {
        second_handle
            .update(cx, |_, w, cx| {
                shell.update(cx, |s, cx| {
                    s.window = Some(second_handle);
                    s.mount(w, cx);
                    assert_eq!(
                        s.sessions["a"].composer.read(cx).editor().entity_id(),
                        editor.entity_id()
                    );
                    assert_eq!(editor.read(cx).selected_range(), 1..3);
                    let composer = s.sessions["a"].composer.clone();
                    composer.update(cx, |c, cx| {
                        c.set_status(false, false, cx);
                        c.set_readiness(true, false, false, false, false, cx);
                    });
                    editor.update(cx, |_, cx| {
                        cx.emit(InputEvent::PressEnter {
                            shift: false,
                            secondary: false,
                        })
                    });
                })
            })
            .unwrap()
    });
    app.run_until_parked();
    assert_eq!(received.get(), 1);
    app.read_entity(&shell, |s, cx| {
        assert_eq!(s.sessions["a"].controller.state().draft, "keep");
        assert_eq!(
            s.sessions["a"]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .to_string(),
            "keep"
        );
    });
}
