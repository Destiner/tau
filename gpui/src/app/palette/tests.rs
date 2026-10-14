use super::*;
use crate::app::tests::{app, create, project, with_shell};
use gpui_kit::Focusable;
use std::{cell::RefCell, rc::Rc};

#[path = "keyboard_tests.rs"]
mod keyboard_tests;
#[path = "new_session_tests.rs"]
mod new_session_tests;
#[path = "parity_tests.rs"]
mod parity_tests;

fn focus_frames(root: &mut gpui_kit::TestAppWindow<Root>) {
    // Match native ordering: callbacks, paint, then callbacks, paint.
    // draw() alone never delivers on_next_frame callbacks.
    for _ in 0..2 {
        root.update(|_, w, cx| {
            w.simulate_next_frame(cx);
        });
        root.draw();
    }
}

#[test]
fn session_shortcuts_are_logical_readable_keys() {
    with_shell(64, |s, w, cx, _| {
        create(s, "first", "first draft", w, cx);
        create(s, "second", "second draft", w, cx);
        create(s, "third", "third draft", w, cx);
        s.active = "second".into();
        s.sync(w, cx);
        s.mount_palette(w, cx);
        let pages = s.palette_pages(cx).0;
        let rows = &pages[0].items;
        for (id, shortcut) in [
            ("session.next", "Ctrl+Tab"),
            ("session.previous", "Ctrl+Shift+Tab"),
        ] {
            assert_eq!(
                rows.iter().find(|row| row.id == id).unwrap().shortcut,
                shortcut
            );
        }
    });
}

#[test]
fn shell_rows_use_workspace_and_hide_admin_actions_while_disabled() {
    with_shell(64, |s, w, cx, _| {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
        s.mount_palette(w, cx);
        let (pages, _) = s.palette_pages(cx);
        let root = &pages[0].items;
        assert!(root.iter().any(|r| r.id == "session.new"));
        assert!(root.iter().any(|r| r.id == "project.openRemote"));
        assert!(!root.iter().any(|r| r.id.contains("settings")
            || r.id.contains("admin")
            || r.id == "app.reportIssue"));
        s.picker_busy = true;
        assert!(s.palette_pages(cx).0.iter().all(|p| p.items.is_empty()));
        s.palette_selected("session.new", w, cx);
        assert!(s.sessions.is_empty());
    });
}

#[test]
fn admin_palette_actions_follow_actual_gate_and_handoff_to_retained_reporter() {
    with_shell(64, |s, w, cx, requests| {
        s.mount_palette(w, cx);
        s.mount_admin(w, cx);
        requests.try_recv().unwrap();
        let super::super::admin::Command::SetMode { revision, .. } = s.admin.set_mode(true) else {
            unreachable!()
        };
        s.admin_event(
            super::super::admin::Reply::ModeSet {
                revision,
                enabled: true,
                saved: true,
            },
            cx,
        );
        s.toggle_palette(w, cx);
        let (_, targets) = s.palette_pages(cx);
        assert_eq!(targets.get("app.reportIssue"), Some(&Target::ReportIssue));
        assert_eq!(
            targets.get("app.disableAdminMode"),
            Some(&Target::DisableAdmin)
        );
        s.palette_selected("app.reportIssue", w, cx);
        assert!(!s.palette.is_open(cx));
        assert!(s.admin_reporter_open(cx));
        let reporter = s.admin.view.clone().unwrap();
        let editor = reporter.read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("retained report", w, cx));
        s.toggle_palette(w, cx);
        assert!(s.palette.is_open(cx));
        assert!(reporter.read(cx).is_open());
        assert_eq!(editor.read(cx).value().as_ref(), "retained report");
        let command = s.admin.set_mode(false);
        s.dispatch_admin(command, cx);
        assert!(!s.admin.enabled());
        assert!(!s.admin_reporter_open(cx));
        assert!(!s.palette_pages(cx).1.contains_key("app.reportIssue"));
        assert!(matches!(
            requests.try_recv().unwrap(),
            Command::Admin(super::super::admin::Command::SetMode { enabled: false, .. })
        ));
    });
}

#[test]
fn shell_callback_executes_archived_toggle_and_rejects_busy_execution() {
    with_shell(64, |s, w, cx, _| {
        s.mount_palette(w, cx);
        s.toggle_palette(w, cx);
        s.palette_selected("sidebar.toggleArchived", w, cx);
        assert!(s.session_actions.archived);
        s.picker_busy = true;
        s.palette_selected("sidebar.toggleArchived", w, cx);
        assert!(s.session_actions.archived);
    });
}

#[test]
fn selected_session_replacement_cannot_rebind_new_session() {
    with_shell(64, |s, w, cx, requests| {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
        create(s, "original", "sacred draft", w, cx);
        s.sessions.get_mut("original").unwrap().starting = false;
        s.mount_palette(w, cx);
        s.toggle_palette(w, cx);
        assert!(s.palette.binding.is_some());
        // Same runtime key, replacement generation is not the captured target.
        s.sessions
            .get_mut("original")
            .unwrap()
            .controller
            .begin_generation(42);
        while requests.try_recv().is_ok() {}
        s.palette_selected("session.new", w, cx);
        assert_eq!(s.sessions.len(), 1);
        assert!(requests.try_recv().is_err());
    });
}

#[test]
fn new_session_callback_dispatches_real_start_without_losing_old_draft() {
    with_shell(128, |s, w, cx, requests| {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
        create(s, "original", "sacred draft", w, cx);
        s.sessions.get_mut("original").unwrap().starting = false;
        s.mount_palette(w, cx);
        s.toggle_palette(w, cx);
        while requests.try_recv().is_ok() {}
        s.palette_selected("session.new", w, cx);
        assert_eq!(s.sessions.len(), 2);
        assert_ne!(s.active, "original");
        assert_eq!(
            s.sessions["original"].controller.state().draft,
            "sacred draft"
        );
        let mut started = false;
        while let Ok(command) = requests.try_recv() {
            if let Command::Start { runtime, .. } = command {
                assert_eq!(runtime, s.active);
                started = true;
            }
        }
        assert!(started);
    });
}

#[test]
fn nested_back_restores_root_text_and_refresh_does_not_change_page() {
    let mut app = app();
    let mut initial = pages(&["models"]);
    initial[0].items[0].destination = Some("models".into());
    initial.push(PalettePage {
        id: "models".into(),
        placeholder: "Search models…".into(),
        items: pages(&["alpha"])[0].items.clone(),
    });
    let refresh = initial.clone();
    let mut p = app.open_window(|w, cx| Palette::new(false, initial, w, cx));
    p.update(|p, w, cx| {
        p.open(w, cx);
        p.set_query("mod", w, cx);
    });
    p.simulate_keystroke("enter");
    app.run_until_parked();
    p.update(|p, w, cx| {
        assert_eq!(p.page_id(), "models");
        p.set_query("al", w, cx);
        p.refresh_pages(refresh, w, cx);
        assert_eq!(p.page_id(), "models");
        assert_eq!(p.query(), "al");
        p.back(w, cx);
        assert_eq!(p.page_id(), "root");
        assert_eq!(p.query(), "mod");
        assert_eq!(p.selected_id(), Some("models"));
        assert!(p.search_editor().read(cx).focus_handle(cx).is_focused(w));
    });
}

#[test]
fn active_and_background_work_do_not_block_navigation_or_project_actions() {
    with_shell(128, |s, w, cx, _| {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
        create(s, "background", "background draft", w, cx);
        create(s, "active", "active draft", w, cx);
        for live in s.sessions.values_mut() {
            live.starting = false;
            live.controller.begin_generation(1);
            live.controller
                .handle(serde_json::json!({"type":"agent_start"}));
        }
        s.mount_palette(w, cx);
        assert!(s.is_busy());
        s.toggle_palette(w, cx);
        assert!(s.palette.is_open(cx));
        let (pages, _) = s.palette_pages(cx);
        for id in [
            "session.switch",
            "project.switch",
            "session.new",
            "project.openLocal",
            "project.openRemote",
            "focus.composer",
        ] {
            assert!(pages[0].items.iter().any(|row| row.id == id), "{id}");
        }
        assert!(
            !pages[0]
                .items
                .iter()
                .any(|row| matches!(row.id.as_str(), "model.choose" | "effort.choose"))
        );
        s.palette_selected("session.new", w, cx);
        assert_eq!(s.sessions.len(), 3);
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "active draft"
        );
        assert_eq!(
            s.sessions["background"].controller.state().draft,
            "background draft"
        );
    });
}

#[test]
fn model_and_effort_choices_allow_pending_boot_reads_but_block_setting_writes() {
    with_shell(128, |s, w, cx, requests| {
        create(s, "active", "draft", w, cx);
        crate::app::tests::started(s, "active", 1, cx);
        while let Ok(command) = requests.try_recv() {
            let Command::Send { request, .. } = command else {
                continue;
            };
            let kind = request["type"].as_str().unwrap();
            let data = match kind {
                "get_state" => serde_json::json!({}),
                "get_available_models" => serde_json::json!({"models":[
                    {"provider":"fixture","id":"alpha","name":"Alpha"}
                ]}),
                "get_available_thinking_levels" => {
                    serde_json::json!({"levels":["high", "xhigh", "max"]})
                }
                _ => continue,
            };
            s.ingest(
                crate::app::tests::rpc(
                    "active",
                    1,
                    serde_json::json!({
                        "type":"response", "id":request["id"], "command":kind,
                        "success":true, "data":data
                    }),
                ),
                cx,
            );
        }
        s.mount_palette(w, cx);
        s.toggle_palette(w, cx);
        let controller = &s.sessions["active"].controller;
        assert!(controller.state().ready);
        assert!(controller.pending_count() > 0);
        assert!(!controller.setting_pending());
        let pages = s.palette_pages(cx).0;
        assert!(pages[0].items.iter().any(|r| r.id == "model.choose"));
        assert!(pages[0].items.iter().any(|r| r.id == "effort.choose"));
        let model = pages[3]
            .items
            .iter()
            .find(|r| r.id == "model:fixture/alpha")
            .unwrap();
        assert_eq!(model.label, "Alpha");
        assert!(
            model.detail.is_empty(),
            "Vue model rows show names, not IDs"
        );
        assert!(pages[4].items.iter().any(|r| r.id == "effort:high"));
        for (id, label) in [("xhigh", "Extra High"), ("max", "Max")] {
            let row = pages[4]
                .items
                .iter()
                .find(|r| r.id == format!("effort:{id}"))
                .unwrap();
            assert_eq!(row.label, label);
            assert_eq!(row.label, crate::components::composer::effort_label(id));
        }
        s.palette_selected("model:fixture/alpha", w, cx);
        assert!(s.sessions["active"].controller.setting_pending());
        let pages = s.palette_pages(cx).0;
        assert!(
            !pages[0]
                .items
                .iter()
                .any(|r| matches!(r.id.as_str(), "model.choose" | "effort.choose"))
        );
        assert!(pages[3].items.is_empty());
        assert!(pages[4].items.is_empty());
        assert_eq!(s.sessions["active"].controller.state().draft, "draft");
        let mut setting_sent = false;
        while let Ok(command) = requests.try_recv() {
            if let Command::Send { request, .. } = command {
                setting_sent |= request["type"] == "set_model";
            }
        }
        assert!(setting_sent);
    });
}

#[test]
fn rename_dispatch_hands_focus_to_header_even_with_pending_reads() {
    with_shell(128, |s, w, cx, _| {
        let mut p = project();
        p.sessions.push(serde_json::from_value(serde_json::json!({"id":"saved","path":"/saved","title":"Original","lastActive":"now","lastUserMessageAt":0,"sortAt":0,"archived":false,"selected":true})).unwrap());
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![p.clone()],
            active_project_path: p.path.clone(),
            pi_path: None,
        });
        s.create_live(
            "live".into(),
            p,
            "saved".into(),
            "/saved".into(),
            "sacred".into(),
            w,
            cx,
        );
        crate::app::tests::started(s, "live", 1, cx);
        s.ingest(crate::app::tests::rpc("live", 1, serde_json::json!({"type":"response","id":"tau:live:g1:1","command":"get_state","success":true,"data":{"sessionId":"saved","sessionFile":"/saved","sessionName":"Original"}})), cx);
        s.restored = true;
        s.mount(w, cx);
        s.sync(w, cx);
        assert!(s.can_rename());
        s.toggle_palette(w, cx);
        s.palette_selected("session.rename", w, cx);
        assert!(!s.palette.is_open(cx));
        let header = s.header.as_ref().unwrap();
        assert!(header.read(cx).renaming());
        assert!(
            header
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
        assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
    });
}

#[test]
fn remote_dispatch_closes_palette_before_modal_and_modal_blocks_reopening() {
    with_shell(64, |s, w, cx, _| {
        create(s, "original", "draft", w, cx);
        s.mount_palette(w, cx);
        s.toggle_palette(w, cx);
        s.palette_selected("project.openRemote", w, cx);
        assert!(!s.palette.is_open(cx));
        assert!(s.remote.is_open());
        let remote = s.remote.view.clone().unwrap();
        // Palette focus restoration must not run after the remote editor owns focus.
        assert!(
            !s.palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .search_editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
        s.toggle_palette(w, cx);
        assert!(!s.palette.is_open(cx));
        assert!(
            s.palette_pages(cx)
                .0
                .iter()
                .all(|page| page.items.is_empty())
        );
        assert_eq!(s.remote.view.as_ref(), Some(&remote));
    });
}

#[test]
fn global_shortcuts_use_the_real_shell_wrapper_and_recheck_availability() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(128);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()],
                active_project_path: project().path,
                pi_path: None,
            });
            create(s, "active", "draft", w, cx);
            // A starting composer's disabled Input has no focus node in the dispatch tree.
            crate::app::tests::started(s, "active", 1, cx);
            s.ingest(
                crate::app::tests::rpc(
                    "active",
                    1,
                    serde_json::json!({
                        "type":"response", "id":"tau:active:g1:1", "command":"get_state",
                        "success":true, "data":{}
                    }),
                ),
                cx,
            );
            s.mount(w, cx);
            s.action(Action::FocusComposer, w, cx);
        });
    });
    root.draw();
    root.update(|_, w, cx| {
        let s = shell.read(cx);
        assert_eq!(s.active, "active");
        assert!(!s.palette_blocked(cx));
        assert!(
            s.sessions["active"]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    for (key, page) in [("k", "root"), ("p", "sessions"), ("shift-p", "projects")] {
        let key = format!("{modifier}-{key}");
        root.simulate_keystroke(&key);
        app.run_until_parked();
        root.update(|_, _, cx| {
            let view = shell.read(cx).palette.view.clone().unwrap();
            assert!(view.read(cx).is_open(), "{key}");
            assert_eq!(view.read(cx).page_id(), page);
        });
        root.simulate_keystroke("escape");
        app.run_until_parked();
        focus_frames(&mut root);
    }
    root.simulate_keystroke(&format!("{modifier}-shift-h"));
    app.run_until_parked();
    assert!(app.update(|cx| shell.read(cx).session_actions.archived));
    // Cmd-N remains NativeHost's action; the palette contributes no duplicate.
    while requests.try_recv().is_ok() {}
    root.simulate_keystroke(&format!("{modifier}-alt-down"));
    app.run_until_parked();
    assert!(requests.try_recv().is_err()); // No neighbor: unavailable shortcut is inert.
    root.update(|_, _, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(
                crate::app::tests::rpc("active", 1, serde_json::json!({"type":"agent_start"})),
                cx,
            );
        });
    });
    root.draw();
    root.simulate_keystroke(&format!("{modifier}-."));
    app.run_until_parked();
    assert_eq!(sent(&requests, "abort").len(), 1);
    app.update(|cx| {
        assert_eq!(
            shell.read(cx).sessions["active"].controller.state().draft,
            "draft"
        );
    });
    focus_frames(&mut root);
    root.update(|_, w, cx| {
        let focus = w.focused(cx).expect("Stop must leave a focus owner");
        assert!(
            shell
                .read(cx)
                .shell_focus
                .as_ref()
                .unwrap()
                .contains(&focus, w),
            "Stop focus must be mounted under TauShell"
        );
    });
    root.simulate_keystroke(&format!("{modifier}-alt-o"));
    app.run_until_parked();
    assert!(app.update(|cx| shell.read(cx).remote.is_open()));
}

#[test]
fn session_handoff_waits_for_the_destination_dispatch_tree() {
    let mut app = app();
    let (commands, _requests) = async_channel::bounded(256);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()],
                active_project_path: project().path,
                pi_path: None,
            });
            for key in ["a", "b"] {
                create(s, key, "draft", w, cx);
                crate::app::tests::started(s, key, 1, cx);
                s.ingest(
                    crate::app::tests::rpc(
                        key,
                        1,
                        serde_json::json!({
                            "type":"response", "id":format!("tau:{key}:g1:1"),
                            "command":"get_state", "success":true, "data":{}
                        }),
                    ),
                    cx,
                );
            }
            s.active = "a".into();
            s.mount(w, cx);
            s.action(Action::FocusComposer, w, cx);
        });
    });
    root.draw();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let (_, targets) = s.palette_pages(cx);
            let id = targets
                .iter()
                .find_map(|(id, target)| {
                    matches!(target, Target::Live(key, ..) if key == "b").then(|| id.clone())
                })
                .unwrap();
            s.palette.targets = targets;
            s.palette_selected(&id, w, cx);
        });
        let target = w.window_handle();
        let shell = shell.clone();
        // Run before Test mode's automatic dirty-window draw. Otherwise the
        // test's eager paint hides production's callbacks-before-paint ordering.
        cx.defer(move |cx| {
            target
                .update(cx, |_, w, cx| {
                    w.simulate_next_frame(cx);
                    w.draw(cx).clear(cx);
                    w.simulate_next_frame(cx);
                    let s = shell.read(cx);
                    assert_eq!(s.active, "b");
                    let focus = s.sessions["b"]
                        .composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx);
                    assert!(s.shell_focus.as_ref().unwrap().contains(&focus, w));
                    assert!(
                        focus.is_focused(w),
                        "handoff must use the freshly painted destination"
                    );
                })
                .unwrap();
        });
    });
}

#[test]
fn double_escape_emits_only_one_dismissal_and_does_not_activate_rows() {
    let mut app = app();
    let mut palette = None;
    let mut root = app.open_window(|w, cx| {
        let p = cx.new(|cx| Palette::new(false, pages(&["alpha"]), w, cx));
        palette = Some(p.clone());
        Root::new(p, w, cx)
    });
    let p = palette.unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let captured = events.clone();
    let _sub = app.update(|cx| {
        cx.subscribe(&p, move |_, event: &PaletteEvent, _| {
            captured.borrow_mut().push(event.clone())
        })
    });
    root.update(|_, w, cx| p.update(cx, |p, cx| p.open(w, cx)));
    root.simulate_keystroke("escape");
    root.simulate_keystroke("escape");
    root.simulate_keystroke("enter");
    app.run_until_parked();
    assert_eq!(*events.borrow(), vec![PaletteEvent::Dismissed]);
}

fn ready(s: &mut Shell, w: &mut Window, cx: &mut Context<Shell>) {
    create(s, "active", "sacred draft", w, cx);
    crate::app::tests::started(s, "active", 1, cx);
    s.ingest(
        crate::app::tests::rpc(
            "active",
            1,
            serde_json::json!({
                "type":"response", "id":"tau:active:g1:1", "command":"get_state",
                "success":true, "data":{}
            }),
        ),
        cx,
    );
    s.mount_palette(w, cx);
    s.toggle_palette(w, cx);
}

fn sent(requests: &async_channel::Receiver<Command>, kind: &str) -> Vec<serde_json::Value> {
    let mut result = vec![];
    while let Ok(command) = requests.try_recv() {
        if let Command::Send { request, .. } = command
            && request["type"] == kind
        {
            result.push(request);
        }
    }
    result
}

#[test]
fn draft_commands_use_controller_draft_and_pending_admission() {
    with_shell(128, |s, w, cx, requests| {
        ready(s, w, cx);
        let root = s.palette_pages(cx).0.remove(0).items;
        assert!(
            root.iter()
                .any(|r| r.id == "app.quit" && r.shortcut == "⌘Q")
        );
        assert!(
            root.iter()
                .any(|r| r.id == "composer.send" && r.label == "Send Message")
        );
        assert!(!root.iter().any(|r| matches!(
            r.id.as_str(),
            "composer.followUp" | "composer.stop" | "queue.clear" | "queue.review" | "app.updates"
        )));
        s.sessions
            .get_mut("active")
            .unwrap()
            .controller
            .set_draft("latest draft");
        s.palette_selected("composer.send", w, cx);
        let prompt = sent(&requests, "prompt");
        assert_eq!(prompt.len(), 1);
        assert_eq!(prompt[0]["message"], "latest draft");
        assert!(s.sessions["active"].controller.state().draft.is_empty());
        assert!(s.sessions["active"].controller.submission_pending());
        s.toggle_palette(w, cx);
        s.palette_selected("composer.send", w, cx);
        assert!(sent(&requests, "prompt").is_empty());
        s.ingest(crate::app::tests::rpc("active", 1, serde_json::json!({
            "type":"response", "id":prompt[0]["id"], "command":"prompt", "success":false, "error":"fixture"
        })), cx);
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "latest draft"
        );
    });
}

#[test]
fn queued_commands_dispatch_modes_and_stop_preserves_draft() {
    for command in ["composer.send", "composer.followUp"] {
        with_shell(128, |s, w, cx, requests| {
            ready(s, w, cx);
            s.ingest(
                crate::app::tests::rpc("active", 1, serde_json::json!({"type":"agent_start"})),
                cx,
            );
            s.sync_palette(w, cx);
            let root = s.palette_pages(cx).0.remove(0).items;
            assert!(
                root.iter()
                    .any(|r| r.id == "composer.send" && r.label == "Queue Steering Message")
            );
            assert!(root.iter().any(|r| r.id == "composer.followUp"));
            s.palette_selected(command, w, cx);
            let steering = sent(&requests, "set_steering_mode");
            assert_eq!(steering.len(), 1);
            assert!(s.sessions["active"].controller.submission_pending());
            assert!(!s.palette_pages(cx).1.contains_key("composer.send"));
            s.ingest(crate::app::tests::rpc("active", 1, serde_json::json!({
                "type":"response", "id":steering[0]["id"], "command":"set_steering_mode", "success":true
            })), cx);
            let follow = sent(&requests, "set_follow_up_mode");
            assert_eq!(follow.len(), 1);
            s.ingest(crate::app::tests::rpc("active", 1, serde_json::json!({
                "type":"response", "id":follow[0]["id"], "command":"set_follow_up_mode", "success":true
            })), cx);
            let prompt = sent(&requests, "prompt");
            assert_eq!(prompt.len(), 1);
            assert_eq!(prompt[0]["message"], "sacred draft");
            assert_eq!(
                prompt[0]["streamingBehavior"],
                if command == "composer.followUp" {
                    "followUp"
                } else {
                    "steer"
                }
            );
            s.sessions
                .get_mut("active")
                .unwrap()
                .controller
                .set_draft("sacred draft");
            s.toggle_palette(w, cx);
            s.palette_selected("composer.stop", w, cx);
            assert_eq!(sent(&requests, "abort").len(), 1);
            assert_eq!(
                s.sessions["active"].controller.state().draft,
                "sacred draft"
            );
            assert!(!s.palette_pages(cx).1.contains_key("composer.stop"));
        });
    }
}

#[test]
fn submission_rechecks_exact_binding_and_queue_clear_is_existing_effect() {
    with_shell(128, |s, w, cx, requests| {
        ready(s, w, cx);
        s.sessions
            .get_mut("active")
            .unwrap()
            .controller
            .begin_generation(2);
        s.palette_selected("composer.send", w, cx);
        assert!(sent(&requests, "prompt").is_empty());
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
    with_shell(128, |s, w, cx, requests| {
        ready(s, w, cx);
        s.ingest(
            crate::app::tests::rpc(
                "active",
                1,
                serde_json::json!({
                    "type":"queue_update", "steering":["queued"], "followUp":[]
                }),
            ),
            cx,
        );
        s.sync_palette(w, cx);
        assert!(s.palette_pages(cx).1.contains_key("queue.clear"));
        s.palette_selected("queue.clear", w, cx);
        assert_eq!(sent(&requests, "clear_queue").len(), 1);
        assert!(s.sessions["active"].controller.queue_busy());
        assert!(!s.palette_pages(cx).1.contains_key("queue.clear"));
        assert_eq!(
            s.sessions["active"].controller.state().draft,
            "sacred draft"
        );
    });
}

fn pages(ids: &[&str]) -> Vec<PalettePage> {
    vec![PalettePage {
        id: "root".into(),
        placeholder: "Search commands…".into(),
        items: ids
            .iter()
            .map(|id| PaletteItem {
                id: (*id).into(),
                label: (*id).into(),
                detail: "".into(),
                shortcut: "".into(),
                group: None,
                destination: None,
            })
            .collect(),
    }]
}

#[test]
fn real_root_keyboard_enter_arrows_escape_and_refresh_keep_identity() {
    let mut app = app();
    let mut palette = None;
    let mut root = app.open_window(|w, cx| {
        let p = cx.new(|cx| Palette::new(false, pages(&["alpha", "beta"]), w, cx));
        palette = Some(p.clone());
        Root::new(p, w, cx)
    });
    let p = palette.unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let captured = events.clone();
    let _sub = app.update(|cx| {
        cx.subscribe(&p, move |_, event: &PaletteEvent, _| {
            captured.borrow_mut().push(event.clone())
        })
    });
    root.update(|_, w, cx| p.update(cx, |p, cx| p.open(w, cx)));
    root.simulate_keystroke("down");
    root.update(|_, w, cx| {
        p.update(cx, |p, cx| {
            assert_eq!(p.selected_id(), Some("beta"));
            p.set_query("be", w, cx);
            p.refresh_pages(pages(&["gamma", "beta"]), w, cx);
            assert_eq!(p.query(), "be");
            assert_eq!(p.selected_id(), Some("beta"));
            assert!(p.search_editor().read(cx).focus_handle(cx).is_focused(w));
        })
    });
    root.simulate_keystroke("enter");
    app.run_until_parked();
    assert_eq!(
        *events.borrow(),
        vec![PaletteEvent::Selected("beta".into())]
    );
    root.update(|_, w, cx| {
        p.update(cx, |p, cx| {
            p.open(w, cx);
            p.refresh_pages(pages(&["beta"]), w, cx);
            assert_eq!(p.selected_id(), None); // Never rebound to row zero.
        })
    });
    root.simulate_keystroke("enter");
    root.simulate_keystroke("escape");
    app.run_until_parked();
    assert_eq!(events.borrow().last(), Some(&PaletteEvent::Dismissed));
}
