//! Native command routes, including deliberately excluded and dialog-scoped actions.
use super::*;
use crate::app::tests::{rpc, started};
use serde_json::json;

const COVERAGE: [(&str, &str); 36] = [
    ("session.new", "navigation/project"),
    ("project.open", "safe/open-project page"),
    ("project.openLocal", "safe/local picker"),
    ("project.openRemote", "safe/remote browser"),
    ("session.switch", "navigation/workspace"),
    ("project.switch", "navigation/workspace"),
    ("session.rename", "safe/saved ready/not pending"),
    ("session.archive", "safe/saved active/idle"),
    ("session.unarchive", "safe/saved archived"),
    ("session.markRead", "safe/canonical unread"),
    ("session.markUnread", "safe/canonical read"),
    ("sidebar.toggleArchived", "safe/sidebar"),
    ("project.expand", "safe/collapsed"),
    ("project.collapse", "safe/expanded"),
    ("project.moveUp", "safe/previous project"),
    ("project.moveDown", "safe/next project"),
    ("project.remove", "safe/lifecycle availability"),
    ("focus.composer", "safe/enabled mounted composer"),
    ("focus.sidebar", "safe/mounted sidebar"),
    ("focus.transcript", "safe/mounted transcript"),
    ("model.choose", "safe/ready idle/not setting pending"),
    ("effort.choose", "safe/ready idle/available levels"),
    ("composer.send", "safe/current draft/submission admission"),
    (
        "composer.followUp",
        "safe/current working draft/queue admission",
    ),
    ("composer.stop", "safe/working/not stopping or compacting"),
    ("queue.clear", "safe/nonempty/not queue pending"),
    ("queue.review", "safe/retained failed drafts"),
    ("app.updates", "safe/status only"),
    ("app.checkUpdates", "excluded/updater mutation deferred"),
    ("app.reportIssue", "safe/admin"),
    ("app.quit", "safe/native lifecycle"),
    ("history.load", "safe/current available boundary"),
    ("remote.reconnect", "safe/remote disconnected/not starting"),
    ("session.next", "navigation/displayed successor/no wrap"),
    (
        "session.previous",
        "navigation/displayed predecessor/no wrap",
    ),
    (
        "feedback.action",
        "dialog scoped/native submit, not palette",
    ),
];

#[test]
fn every_expected_command_has_an_explicit_native_route_or_scope_exclusion() {
    let native = include_str!("../palette.rs");
    let mut ids = std::collections::BTreeSet::new();
    for (id, scope) in COVERAGE {
        assert!(ids.insert(id), "duplicate {id}");
        assert!(
            scope.starts_with("safe/")
                || scope.starts_with("navigation/")
                || scope.starts_with("excluded/")
                || scope.starts_with("dialog scoped/"),
            "missing native scope: {id}"
        );
        if !scope.starts_with("excluded/") && id != "feedback.action" {
            assert!(
                native.contains(&format!("\"{id}\"")),
                "missing native identity: {id}"
            );
        }
    }
    assert!(!native.contains("\"app.checkUpdates\""));
    assert!(!native.contains("\"feedback.action\""));
}

#[test]
fn combined_open_project_page_and_read_variants_share_dispatch() {
    with_shell(128, |s, w, cx, _| {
        ready(s, w, cx);
        s.palette_command("project.open", w, cx);
        assert_eq!(
            s.palette.view.as_ref().unwrap().read(cx).page_id(),
            "open-project"
        );
        let (pages, _) = s.palette_pages(cx);
        assert_eq!(pages[5].items.len(), 2);
        assert!(s.palette_pages(cx).1.contains_key("session.markUnread"));
        assert!(!s.palette_pages(cx).1.contains_key("session.markRead"));
        s.palette_selected("session.markUnread", w, cx);
        assert!(s.palette_session_unread("active"));
        s.palette_command("session.markRead", w, cx);
        assert!(!s.palette_session_unread("active"));
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

#[test]
fn retained_queue_review_does_not_restore_or_submit_over_a_newer_draft() {
    with_shell(128, |s, w, cx, requests| {
        ready(s, w, cx);
        s.sessions.get_mut("active").unwrap().restored_recovered = vec!["retained".into()];
        s.sessions.get_mut("active").unwrap().dirty = true;
        s.sync(w, cx);
        s.palette_command("queue.review", w, cx);
        assert!(s.sessions["active"].queue.read(cx).details_open());
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
        assert_eq!(s.sessions["active"].restored_recovered, vec!["retained"]);
        assert!(!s.can_recover_queue("active", cx));
        assert!(sent(&requests, "prompt").is_empty());
    });
}

#[test]
fn extension_navigation_admission_is_distinct_from_editing_and_global_modals() {
    with_shell(256, |s, w, cx, _| {
        ready(s, w, cx);
        create(s, "other", "other draft", w, cx);
        started(s, "other", 2, cx);
        s.active = "active".into();
        s.ingest(rpc("active", 1, json!({
            "type":"extension_ui_request", "id":"owned-question", "method":"input", "title":"Answer"
        })), cx);
        s.sync(w, cx);
        s.toggle_palette(w, cx);
        if !s.palette.is_open(cx) {
            s.toggle_palette(w, cx);
        }
        let (pages, targets) = s.palette_pages(cx);
        for id in ["session.new", "session.switch", "project.switch"] {
            assert!(pages[0].items.iter().any(|r| r.id == id), "{id}");
        }
        for id in [
            "session.rename",
            "focus.composer",
            "queue.review",
            "project.open",
            "session.markUnread",
        ] {
            assert!(!pages[0].items.iter().any(|r| r.id == id), "{id}");
        }
        assert!(
            targets
                .values()
                .any(|t| matches!(t, Target::Live(key, ..) if key == "other"))
        );
        s.picker_busy = true;
        assert!(s.palette_pages(cx).0.iter().all(|p| p.items.is_empty()));
        s.picker_busy = false;
        assert_eq!(
            s.sessions["active"].controller.state().extension_dialogs[0].id,
            "owned-question"
        );
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

#[test]
fn adjacent_sessions_follow_sidebar_creation_order_not_runtime_map_and_never_wrap() {
    with_shell(256, |s, w, cx, _| {
        ready(s, w, cx);
        create(s, "z-newer", "z draft", w, cx);
        started(s, "z-newer", 2, cx);
        create(s, "b-newest", "b draft", w, cx);
        started(s, "b-newest", 3, cx);
        s.active = "z-newer".into();
        s.sync(w, cx);
        assert!(
            matches!(s.palette_adjacent(-1, cx), Some(Target::Live(key, ..)) if key == "b-newest")
        );
        assert!(
            matches!(s.palette_adjacent(1, cx), Some(Target::Live(key, ..)) if key == "active")
        );
        s.active = "b-newest".into();
        assert!(s.palette_adjacent(-1, cx).is_none());
        s.active = "active".into();
        assert!(s.palette_adjacent(1, cx).is_none());
    });
}

#[test]
fn captured_adjacent_target_is_rejected_after_generation_changes() {
    with_shell(256, |s, w, cx, _| {
        ready(s, w, cx);
        create(s, "other", "other draft", w, cx);
        started(s, "other", 2, cx);
        s.active = "other".into();
        s.sync(w, cx);
        s.show_palette_page("root", w, cx);
        let captured = s.palette.targets.get("session.next").cloned().unwrap();
        s.sessions
            .get_mut("active")
            .unwrap()
            .controller
            .begin_generation(99);
        assert_ne!(s.palette_pages(cx).1.get("session.next"), Some(&captured));
        s.palette_selected("session.next", w, cx);
        assert_eq!(s.active, "other");
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

#[test]
fn dismissed_session_failure_admits_reconnect_once_without_replaying_work() {
    with_shell(256, |s, w, cx, requests| {
        ready(s, w, cx);
        let live = s.sessions.get_mut("active").unwrap();
        live.project.connection_string = Some("example.invalid".into());
        live.controller.transport_failed(1);
        live.dirty = true;
        live.projection_dirty = true;
        s.palette
            .view
            .clone()
            .unwrap()
            .update(cx, |p, cx| p.close(cx));
        s.sync(w, cx);
        let dialog = s.fallback.as_ref().unwrap().1.clone();
        assert!(s.palette_blocked(cx));
        dialog.update(cx, |dialog, cx| dialog.cancel(cx));
        assert!(!dialog.read(cx).is_open());
        assert!(!s.palette_blocked(cx));
        assert!(s.sessions["active"].controller.state().error.is_some());
        while requests.try_recv().is_ok() {}
        s.palette_command("remote.reconnect", w, cx);
        s.palette_command("remote.reconnect", w, cx);
        let commands: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok()).collect();
        assert_eq!(
            commands
                .iter()
                .filter(|c| matches!(c, Command::Start { .. }))
                .count(),
            1
        );
        assert!(!commands.iter().any(|c| matches!(c, Command::Send { request, .. } if matches!(request["type"].as_str(), Some("prompt" | "steer" | "follow_up")))));
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

#[test]
fn history_command_uses_current_compaction_boundary_and_is_inert_while_pending() {
    with_shell(256, |s, w, cx, requests| {
        ready(s, w, cx);
        let messages = sent(&requests, "get_messages").pop().unwrap();
        s.ingest(rpc("active", 1, json!({
            "type":"response", "id":messages["id"], "command":"get_messages", "success":true,
            "data":{"messages":[
                {"role":"compactionSummary", "timestamp":5, "summary":"context"},
                {"role":"user", "timestamp":4, "content":"current"}
            ]}
        })), cx);
        s.sync(w, cx);
        let boundary = s.sessions["active"]
            .controller
            .state()
            .transcript
            .iter()
            .find(|r| r.history_available)
            .unwrap()
            .id
            .clone();
        assert_eq!(
            s.palette_pages(cx).1.get("history.load"),
            Some(&Target::History(boundary))
        );
        s.palette_command("history.load", w, cx);
        s.palette_command("history.load", w, cx);
        assert_eq!(sent(&requests, "get_entries").len(), 1);
        assert!(!s.palette_pages(cx).1.contains_key("history.load"));
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

#[test]
fn root_shortcuts_navigate_out_of_extension_and_return_to_its_retained_answer() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| shell.update(cx, |s, cx| {
        ready(s, w, cx);
        create(s, "owner", "owner draft", w, cx);
        started(s, "owner", 2, cx);
        s.ingest(rpc("owner", 2, json!({
            "type":"response", "id":"tau:owner:g2:1", "command":"get_state", "success":true, "data":{}
        })), cx);
        s.ingest(rpc("owner", 2, json!({
            "type":"extension_ui_request", "id":"question", "method":"input", "title":"Answer"
        })), cx);
        s.mount(w, cx);
        s.sync(w, cx);
        s.restore_input_focus(cx);
    }));
    focus_frames(&mut root);
    root.simulate_input("retained answer");
    app.run_until_parked();
    let dialog = app.update(|cx| {
        shell.read(cx).sessions["owner"]
            .extension
            .as_ref()
            .unwrap()
            .1
            .clone()
    });
    app.update(|cx| {
        assert_eq!(dialog.read(cx).value(cx), "retained answer");
        dialog
            .read(cx)
            .editor()
            .update(cx, |editor, cx| editor.set_selected_range(1..4, cx));
    });
    root.simulate_keystroke("ctrl-tab");
    app.run_until_parked();
    focus_frames(&mut root);
    app.update(|cx| {
        let s = shell.read(cx);
        assert_eq!(s.active, "active");
        assert_eq!(s.sessions["owner"].controller.state().draft, "owner draft");
        assert_eq!(dialog.read(cx).value(cx), "retained answer");
        assert!(dialog.read(cx).accepted_event().is_none());
        assert_eq!(dialog.read(cx).editor().read(cx).selected_range(), 1..4);
    });
    root.simulate_keystroke("ctrl-tab");
    app.run_until_parked();
    app.update(|cx| assert_eq!(shell.read(cx).active, "active"));
    root.simulate_keystroke("ctrl-shift-tab");
    app.run_until_parked();
    focus_frames(&mut root);
    app.update(|cx| {
        let s = shell.read(cx);
        assert_eq!(s.active, "owner");
        assert_eq!(s.sessions["owner"].extension.as_ref().unwrap().1, dialog);
        assert_eq!(dialog.read(cx).value(cx), "retained answer");
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.mount(w, cx);
            s.sync(w, cx);
            assert_eq!(s.sessions["owner"].extension.as_ref().unwrap().1, dialog);
            assert_eq!(dialog.read(cx).editor().read(cx).selected_range(), 1..4);
            assert_eq!(dialog.read(cx).value(cx), "retained answer");
        })
    });
    assert!(sent(&requests, "prompt").is_empty());
}

#[test]
fn project_page_reuses_captured_provisional_runtime_and_authoritative_fold_variant() {
    with_shell(256, |s, w, cx, requests| {
        ready(s, w, cx);
        let path = s.sessions["active"].project.path.clone();
        assert!(
            matches!(s.palette.targets.get(&path), Some(Target::Project(_, Some(target))) if matches!(target.as_ref(), Target::Live(key, ..) if key == "active"))
        );
        while requests.try_recv().is_ok() {}
        s.palette_selected(&path, w, cx);
        assert_eq!(s.active, "active");
        assert!(
            !std::iter::from_fn(|| requests.try_recv().ok())
                .any(|c| matches!(c, Command::Start { .. }))
        );
        s.workspace.as_mut().unwrap().projects[0].collapsed = true;
        let (_, targets) = s.palette_pages(cx);
        assert!(targets.contains_key("project.expand"));
        assert!(!targets.contains_key("project.collapse"));
    });
}

fn press(root: &mut gpui_kit::TestAppWindow<Root>, key: &str) {
    root.draw();
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    root.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    root.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    root.draw();
}

#[test]
fn root_composer_focus_uses_draft_admission_during_startup_work_and_acknowledged_failure() {
    use gpui_kit::test::TestWindowExt;
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            create(s, "active", "retained draft", w, cx);
            s.mount(w, cx);
            s.sync(w, cx);
        });
        w.activate_window();
        w.bounds_changed(cx);
    });
    focus_frames(&mut root);
    root.update(|_, w, cx| {
        let s = shell.read(cx);
        let focus = s.shell_focus.as_ref().unwrap();
        assert!(
            focus.is_focused(w),
            "mount owns initial pending-start focus"
        );
        assert!(w.is_action_available_in(&FocusComposer, focus));
    });
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    for phase in ["startup", "working", "failed"] {
        if phase != "startup" {
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    if phase == "working" {
                        started(s, "active", 1, cx);
                        let request = std::iter::from_fn(|| requests.try_recv().ok())
                            .find_map(|command| match command {
                                Command::Send { request, .. } if request["type"] == "get_state" => {
                                    Some(request)
                                }
                                _ => None,
                            })
                            .expect("real startup state request");
                        s.ingest(
                            rpc(
                                "active",
                                1,
                                json!({
                                    "type":"response", "id":request["id"], "command":"get_state",
                                    "success":true, "data":{}
                                }),
                            ),
                            cx,
                        );
                        assert!(s.sessions["active"].controller.state().ready);
                        s.ingest(rpc("active", 1, json!({"type":"agent_start"})), cx);
                    } else {
                        let live = s.sessions.get_mut("active").unwrap();
                        live.controller.transport_failed(1);
                        live.dirty = true;
                        live.projection_dirty = true;
                    }
                    s.sync(w, cx);
                    if phase == "failed" {
                        assert!(
                            s.palette_blocked(cx),
                            "failure dialog is genuinely blocking"
                        );
                        assert!(s.fallback.as_ref().unwrap().1.read(cx).is_open());
                        assert!(s.sessions["active"].controller.state().error.is_some());
                        assert!(!s.sessions["active"].controller.state().ready);
                    }
                })
            });
            if phase == "failed" {
                root.activate_accessibility();
                root.draw();
                let error = root.update(|_, _, cx| {
                    shell.read(cx).sessions["active"]
                        .controller
                        .state()
                        .error
                        .clone()
                        .unwrap()
                });
                assert!(
                    root.accessibility_tree()
                        .unwrap()
                        .nodes
                        .iter()
                        .any(|(_, n)| {
                            n.value()
                                .or(n.label())
                                .is_some_and(|text| text.contains(&error))
                        })
                );
                let close = root.update(|_, w, _| w.find("close").bounds().center());
                root.simulate_click(close, gpui_kit::MouseButton::Left);
                root.draw();
                root.update(|_, _, cx| {
                    let s = shell.read(cx);
                    // Acknowledgement clears feedback without repairing failed transport.
                    assert!(s.fallback.is_none());
                    assert!(s.sessions["active"].controller.state().error.is_none());
                    assert!(!s.sessions["active"].controller.state().ready);
                });
            }
            focus_frames(&mut root);
        }
        press(&mut root, &format!("{modifier}-shift-t"));
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            let focused = w.focused(cx).unwrap_or_else(|| {
                panic!("{phase}: transcript command leaves a mounted focus owner")
            });
            assert!(
                s.shell_focus.as_ref().unwrap().contains(&focused, w),
                "{phase}"
            );
            assert!(
                w.is_action_available_in(&FocusComposer, &focused),
                "{phase}"
            );
            if phase == "startup" {
                assert!(s.shell_focus.as_ref().unwrap().is_focused(w));
                assert!(!s.sessions["active"].controller.state().ready);
            }
        });
        press(&mut root, &format!("{modifier}-l"));
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            let live = &s.sessions["active"];
            assert!(!live.composer.read(cx).is_disabled(), "{phase}");
            assert!(
                live.composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w),
                "{phase}"
            );
            assert_eq!(live.controller.state().draft, "retained draft");
            assert!(!s.palette.is_open(cx));
        });
    }
    assert!(
        !std::iter::from_fn(|| requests.try_recv().ok()).any(|command| {
            matches!(command, Command::Send { request, .. }
            if matches!(request["type"].as_str(), Some("prompt" | "steer" | "follow_up" | "set_model")))
        }),
        "focus must not submit work or reconcile model choices as a write"
    );
    // Release TestPlatform's strong input-handler owner after all assertions.
    root.update(|_, w, cx| w.blur(cx));
    app.run_until_parked();
    root.draw();
}

#[test]
fn production_open_mounts_pending_start_shortcut_route_before_first_input() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    app.update(|cx| {
        let target = crate::app::open(shell.clone(), cx).unwrap();
        target
            .update(cx, |_, w, cx| {
                shell.update(cx, |s, cx| {
                    create(s, "active", "startup draft", w, cx);
                    s.sync(w, cx);
                });
                w.activate_window();
                w.bounds_changed(cx);
                w.draw(cx).clear(cx);
                let s = shell.read(cx);
                let focus = s.shell_focus.as_ref().unwrap();
                assert!(focus.is_focused(w));
                assert!(w.is_action_available_in(&FocusComposer, focus));
                assert!(!s.sessions["active"].controller.state().ready);
                assert!(!s.sessions["active"].composer.read(cx).is_disabled());
                let keystroke = gpui_kit::Keystroke::parse(if cfg!(target_os = "macos") {
                    "cmd-l"
                } else {
                    "ctrl-l"
                })
                .unwrap();
                w.dispatch_event(
                    gpui_kit::PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                        keystroke: keystroke.clone(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
                w.dispatch_event(
                    gpui_kit::PlatformInput::KeyUp(gpui_kit::KeyUpEvent { keystroke }),
                    cx,
                );
                w.draw(cx).clear(cx);
                let live = &shell.read(cx).sessions["active"];
                assert!(
                    live.composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w)
                );
                assert_eq!(live.controller.state().draft, "startup draft");
            })
            .unwrap();
    });
    assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(|command| {
        matches!(command, Command::Send { request, .. }
            if matches!(request["type"].as_str(), Some("prompt" | "steer" | "follow_up" | "set_model")))
    }));
    app.update(|cx| {
        shell
            .read(cx)
            .window
            .unwrap()
            .update(cx, |_, w, cx| {
                // TestPlatform retains its active window and strong input handler.
                w.blur(cx);
                w.draw(cx).clear(cx);
                w.remove_window();
            })
            .unwrap();
    });
    app.run_until_parked();
}

#[test]
fn root_open_project_and_focus_shortcuts_target_mounted_controls() {
    use gpui_kit::test::TestWindowExt;
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            create(s, "active", "sacred draft", w, cx);
            started(s, "active", 1, cx);
            let request = std::iter::from_fn(|| requests.try_recv().ok())
                .find_map(|command| match command {
                    Command::Send { request, .. } if request["type"] == "get_state" => {
                        Some(request)
                    }
                    _ => None,
                })
                .expect("real startup state request");
            s.ingest(
                rpc(
                    "active",
                    1,
                    json!({
                        "type":"response", "id":request["id"], "command":"get_state",
                        "success":true, "data":{}
                    }),
                ),
                cx,
            );
            assert!(s.sessions["active"].controller.state().ready);
            s.ingest(
                rpc(
                    "active",
                    1,
                    json!({
                        "type":"message_start", "message":{
                            "role":"user", "timestamp":1, "content":"mounted shortcut source"
                        }
                    }),
                ),
                cx,
            );
            s.mount(w, cx);
            s.sync(w, cx);
            s.action(Action::FocusComposer, w, cx);
        })
    });
    focus_frames(&mut root);
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    for _ in 0..3 {
        press(&mut root, &format!("{modifier}-shift-t"));
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            let focus = s.sessions["active"].transcript.read(cx).focus_handle(cx);
            assert!(focus.is_focused(w));
            assert!(s.shell_focus.as_ref().unwrap().contains(&focus, w));
            assert!(w.find("shell-transcript-slot").visible());
            assert!(w.is_action_available_in(&FocusSidebar, &focus));
        });
        press(&mut root, &format!("{modifier}-shift-l"));
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            let focused = w.focused(cx).expect("sidebar shortcut owns focus");
            assert!(s.shell_focus.as_ref().unwrap().contains(&focused, w));
            assert!(
                w.is_action_available_in(&OpenProject, &focused),
                "mounted TauShell command route"
            );
            assert!(
                !s.sessions["active"]
                    .transcript
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            );
            assert!(w.find("session-active").visible());
            assert_eq!(w.find("session-active").focused(), Some(true));
        });
        press(&mut root, &format!("{modifier}-o"));
        app.update(|cx| {
            let s = shell.read(cx);
            let palette = s.palette.view.as_ref().unwrap().read(cx);
            assert!(palette.is_open());
            assert_eq!(palette.page_id(), "open-project");
            assert_eq!(
                s.sessions["active"].controller.state().draft,
                "sacred draft"
            );
        });
        let keystroke = gpui_kit::Keystroke::parse(&format!("{modifier}-o")).unwrap();
        root.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: true,
            prefer_character_input: false,
        });
        root.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        root.draw();
        root.update(|_, _, cx| {
            let palette = shell.read(cx).palette.view.as_ref().unwrap().read(cx);
            assert!(palette.is_open());
            assert_eq!(
                palette.page_id(),
                "open-project",
                "held shortcut stays on its destination"
            );
        });
        press(&mut root, "escape");
        focus_frames(&mut root);
    }
    assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(|command| {
        matches!(command, Command::Send { request, .. }
            if matches!(request["type"].as_str(), Some("prompt" | "steer" | "follow_up" | "set_model")))
    }), "mounted focus/navigation must preserve the draft without writing model choices");
}
