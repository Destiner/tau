//! Production focus routing: no direct palette callbacks or focus repair after selection.
use super::*;
use crate::app::tests::{rpc, started};
use serde_json::json;

#[test]
fn session_selection_keeps_shortcuts_on_the_current_shell_focus_path() {
    for case in ["same", "warm", "cold", "extension"] {
        let mut app = app();
        let (commands, _requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| crate::app::tests::shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                let mut p = project();
                if case == "cold" {
                    p.sessions.push(
                        serde_json::from_value(json!({
                            "id":"cold", "path":"/cold", "title":"Choice Beta",
                            "lastActive":"now", "sortAt":0, "lastUserMessageAt":0,
                            "archived":false, "selected":false
                        }))
                        .unwrap(),
                    );
                }
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![p],
                    active_project_path: project().path,
                    pi_path: None,
                });
                for (key, name) in [("a", "Choice Alpha"), ("b", "Choice Beta")] {
                    if key == "b" && !matches!(case, "warm" | "extension") {
                        continue;
                    }
                    create(s, key, &format!("{key} draft"), w, cx);
                    started(s, key, 1, cx);
                    s.ingest(
                        rpc(
                            key,
                            1,
                            json!({
                                "type":"response", "id":format!("tau:{key}:g1:1"),
                                "command":"get_state", "success":true,
                                "data":{"sessionName":name}
                            }),
                        ),
                        cx,
                    );
                }
                if case == "extension" {
                    s.ingest(
                        rpc(
                            "b",
                            1,
                            json!({
                                "type":"extension_ui_request", "id":"question",
                                "method":"input", "title":"Answer"
                            }),
                        ),
                        cx,
                    );
                }
                s.active = "a".into();
                s.mount(w, cx);
                s.sync(w, cx);
                s.action(Action::FocusComposer, w, cx);
            });
        });
        root.draw();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        root.simulate_keystroke(&format!("{modifier}-p"));
        app.run_until_parked();
        assert!(
            app.update(|cx| shell.read(cx).palette.is_open(cx)),
            "{case}: Cmd-P"
        );
        root.simulate_input(if case == "same" {
            "Choice Alpha"
        } else {
            "Choice"
        });
        root.simulate_keystroke("down");
        app.run_until_parked();
        root.update(|_, _, cx| {
            let s = shell.read(cx);
            let selected = s
                .palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_id()
                .unwrap();
            match s.palette.targets.get(selected).unwrap() {
                Target::Live(key, ..) => assert_eq!(key, if case == "same" { "a" } else { "b" }),
                Target::Select(_, key, _) => assert_eq!(key, "cold"),
                target => panic!("{case}: unexpected row {target:?}"),
            }
        });
        root.simulate_keystroke("enter");
        // The next shortcut must route even before native frame callbacks run.
        if case != "extension" {
            root.simulate_keystroke(&format!("{modifier}-k"));
            assert!(
                app.update(|cx| shell.read(cx).palette.is_open(cx)),
                "{case}: immediate Cmd-K"
            );
            root.simulate_keystroke("escape");
        }
        app.run_until_parked();
        focus_frames(&mut root);
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            assert!(!s.palette.is_open(cx), "{case}: Enter must close");
            assert_eq!(s.sessions["a"].controller.state().draft, "a draft");
            if case == "cold" {
                assert_ne!(s.active, "a");
                assert!(s.sessions[&s.active].starting);
                assert!(
                    !s.sessions[&s.active].composer.read(cx).is_disabled(),
                    "cold drafts remain editable"
                );
                assert!(!s.sessions[&s.active].controller.state().ready);
            } else {
                assert_eq!(s.active, if case == "same" { "a" } else { "b" });
            }
            let focused = w.focused(cx).expect("selection must leave a focus owner");
            assert!(
                s.shell_focus.as_ref().unwrap().contains(&focused, w),
                "{case}: focus must be mounted"
            );
            if case == "cold" {
                assert!(
                    s.shell_focus.as_ref().unwrap().is_focused(w),
                    "cold: not-yet-ready session uses shell fallback"
                );
            }
            if matches!(case, "same" | "warm") {
                assert!(
                    s.sessions[&s.active]
                        .composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w),
                    "{case}: destination composer owns focus"
                );
            }
        });
        // The decisive regression: input is still delivered through Root<Shell>.
        root.simulate_keystroke(&format!("{modifier}-k"));
        app.run_until_parked();
        assert_eq!(
            app.update(|cx| shell.read(cx).palette.is_open(cx)),
            case != "extension",
            "{case}: Cmd-K"
        );
        if case == "extension" {
            root.update(|_, w, cx| {
                let s = shell.read(cx);
                let (_, dialog) = s.sessions[&s.active].extension.as_ref().unwrap();
                // Typing must reach the extension, not the old or disabled composer.
                assert!(dialog.read(cx).is_open());
                assert!(
                    dialog
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w),
                    "extension editor owns focus"
                );
            });
            root.simulate_input("extension answer");
            app.update(|cx| {
                let s = shell.read(cx);
                let (_, dialog) = s.sessions[&s.active].extension.as_ref().unwrap();
                assert_eq!(
                    dialog.read(cx).editor().read(cx).value().as_ref(),
                    "extension answer"
                );
            });
            assert_eq!(
                app.update(|cx| shell.read(cx).sessions["b"]
                    .controller
                    .state()
                    .draft
                    .clone()),
                "b draft"
            );
            continue;
        }
        for _ in 0..3 {
            root.simulate_keystroke("escape");
            app.run_until_parked();
            focus_frames(&mut root);
            assert!(!app.update(|cx| shell.read(cx).palette.is_open(cx)));
            root.simulate_keystroke(&format!("{modifier}-k"));
            app.run_until_parked();
            assert!(
                app.update(|cx| shell.read(cx).palette.is_open(cx)),
                "{case}: repeated open"
            );
        }
    }
}

#[test]
fn palette_escape_reconciles_composer_admission_without_a_following_pi_event() {
    let mut f = crate::app::view::confirm_queue_fixture::Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.show_palette_page("root", w, cx);
            assert!(s.palette.is_open(cx));
            s.sync(w, cx);
            assert!(s.sessions["a"].composer.read(cx).is_disabled());
        })
    });
    f.root.draw();
    f.press("escape");
    focus_frames(&mut f.root);
    f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert!(!s.palette.is_open(cx));
        let composer = s.sessions["a"].composer.read(cx);
        assert!(!composer.is_disabled());
        assert!(composer.editor().read(cx).focus_handle(cx).is_focused(w));
    });
}

#[test]
fn cancellation_preserves_mounted_origin_but_rejects_retained_unmounted_handle() {
    let mut app = app();
    let (commands, _requests) = async_channel::bounded(256);
    let shell = app.new_entity(|_| crate::app::tests::shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let mut origin = None;
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()], active_project_path: project().path, pi_path: None,
            });
            create(s, "a", "a draft", w, cx);
            started(s, "a", 1, cx);
            s.ingest(rpc("a", 1, json!({"type":"response", "id":"tau:a:g1:1", "command":"get_state", "success":true, "data":{}})), cx);
            s.mount(w, cx);
            s.action(Action::FocusComposer, w, cx);
            origin = w.focused(cx);
        });
    });
    root.draw();
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    root.simulate_keystroke(&format!("{modifier}-k"));
    app.run_until_parked();
    root.simulate_keystroke("escape");
    app.run_until_parked();
    root.update(|_, w, cx| {
        w.simulate_next_frame(cx);
    });
    root.draw();
    root.simulate_keystroke(&format!("{modifier}-k"));
    assert!(
        app.update(|cx| shell.read(cx).palette.is_open(cx)),
        "shortcut during cancellation handoff"
    );
    root.simulate_keystroke("escape");
    focus_frames(&mut root);
    root.update(|_, w, _| assert!(origin.as_ref().unwrap().is_focused(w)));
    root.simulate_keystroke(&format!("{modifier}-k"));
    app.run_until_parked();
    // A retained handle can remain alive without being a rendered focus node.
    root.update(|_, _, cx| {
        shell.update(cx, |s, cx| s.palette.origin = Some(cx.focus_handle()));
    });
    root.simulate_keystroke("escape");
    app.run_until_parked();
    focus_frames(&mut root);
    root.update(|_, w, cx| {
        let s = shell.read(cx);
        let focus = w.focused(cx).unwrap();
        assert!(s.shell_focus.as_ref().unwrap().contains(&focus, w));
        assert!(
            s.sessions[&s.active]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    root.simulate_keystroke(&format!("{modifier}-k"));
    app.run_until_parked();
    assert!(app.update(|cx| shell.read(cx).palette.is_open(cx)));
}
