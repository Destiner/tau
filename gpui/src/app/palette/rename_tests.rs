//! Rename handoff through the production Root/Shell dispatch tree.
use super::*;
use crate::app::tests::{app, create, project, rpc, started};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Focusable, MouseButton};
use serde_json::json;

fn focus_frames(root: &mut gpui_kit::TestAppWindow<Root>) {
    for _ in 0..2 {
        root.update(|_, w, cx| w.simulate_next_frame(cx));
        root.draw();
    }
}

fn rename_requests(requests: &async_channel::Receiver<Command>) -> Vec<serde_json::Value> {
    let mut sent = vec![];
    while let Ok(command) = requests.try_recv() {
        if let Command::Send { request, .. } = command
            && request["type"] == "set_session_name"
        {
            sent.push(request);
        }
    }
    sent
}

#[test]
fn palette_preserves_partial_rename_and_escape_restores_editor_until_explicit_enter_or_switch() {
    for pointer in [false, true] {
        for outcome in ["commit", "switch", "dialog"] {
            let mut app = app();
            let (commands, requests) = async_channel::bounded(512);
            let shell = app.new_entity(|_| crate::app::tests::shell(commands));
            let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    let mut p = project();
                    for (key, title) in [("a", "Choice Alpha"), ("b", "Choice Beta")] {
                        p.sessions.push(serde_json::from_value(json!({
                            "id":key, "path":format!("/{key}"), "title":title,
                            "lastActive":"now", "sortAt":0, "lastUserMessageAt":0,
                            "archived":false, "selected":key == "a"
                        })).unwrap());
                    }
                    s.workspace = Some(WorkspaceSnapshot {
                        projects: vec![p],
                        active_project_path: project().path,
                        pi_path: None,
                    });
                    for (key, title) in [("a", "Choice Alpha"), ("b", "Choice Beta")] {
                        create(s, key, &format!("{key} draft"), w, cx);
                        started(s, key, 1, cx);
                        s.ingest(
                            rpc(
                                key,
                                1,
                                json!({
                                    "type":"response", "id":format!("tau:{key}:g1:1"),
                                    "command":"get_state", "success":true,
                                    "data":{"sessionId":key, "sessionFile":format!("/{key}"), "sessionName":title}
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
            root.simulate_keystroke(&format!("{modifier}-shift-r"));
            app.run_until_parked();
            root.draw();
            root.simulate_input("unfinished name");
            app.run_until_parked();
            while requests.try_recv().is_ok() {}

            for _ in 0..2 {
                if pointer {
                    let point = root.update(|_, w, _| w.find("palette").bounds().center());
                    root.simulate_click(point, MouseButton::Left);
                } else {
                    root.simulate_keystroke(&format!("{modifier}-k"));
                }
                app.run_until_parked();
                root.draw();
                assert!(rename_requests(&requests).is_empty());
                app.update(|cx| {
                    let s = shell.read(cx);
                    assert!(s.palette.is_open(cx));
                    let header = s.header.as_ref().unwrap().read(cx);
                    assert!(header.renaming());
                    assert_eq!(header.editor().read(cx).value().as_ref(), "unfinished name");
                });
                root.simulate_keystroke("escape");
                app.run_until_parked();
                focus_frames(&mut root);
                root.update(|_, w, cx| {
                    let s = shell.read(cx);
                    assert!(!s.palette.is_open(cx));
                    let header = s.header.as_ref().unwrap().read(cx);
                    assert!(header.renaming());
                    assert!(header.editor().read(cx).focus_handle(cx).is_focused(w));
                    assert_eq!(header.editor().read(cx).value().as_ref(), "unfinished name");
                });
                assert!(rename_requests(&requests).is_empty());
            }

            if outcome == "dialog" {
                root.simulate_keystroke(&format!("{modifier}-k"));
                app.run_until_parked();
                root.update(|_, w, cx| {
                    shell.update(cx, |s, cx| {
                        s.ingest(
                            rpc(
                                "a",
                                1,
                                json!({
                                    "type":"extension_ui_request", "id":"question",
                                    "method":"input", "title":"Answer"
                                }),
                            ),
                            cx,
                        );
                        s.sync(w, cx);
                    });
                });
                app.run_until_parked();
                focus_frames(&mut root);
                root.update(|_, w, cx| {
                    let s = shell.read(cx);
                    assert!(!s.palette.is_open(cx));
                    let (_, dialog) = s.sessions["a"].extension.as_ref().unwrap();
                    assert!(
                        dialog
                            .read(cx)
                            .editor()
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(w)
                    );
                    let header = s.header.as_ref().unwrap().read(cx);
                    assert!(header.renaming());
                    assert_eq!(header.editor().read(cx).value().as_ref(), "unfinished name");
                });
                root.simulate_input("extension answer");
                app.update(|cx| {
                    let s = shell.read(cx);
                    let (_, dialog) = s.sessions["a"].extension.as_ref().unwrap();
                    assert_eq!(
                        dialog.read(cx).editor().read(cx).value().as_ref(),
                        "extension answer"
                    );
                });
                assert!(rename_requests(&requests).is_empty());
            } else if outcome == "switch" {
                root.simulate_keystroke(&format!("{modifier}-p"));
                root.simulate_input("Choice Beta");
                app.run_until_parked();
                root.draw();
                root.simulate_keystroke("enter");
                app.run_until_parked();
                focus_frames(&mut root);
                app.update(|cx| {
                    let s = shell.read(cx);
                    assert_eq!(s.active, "b");
                    assert!(!s.header.as_ref().unwrap().read(cx).renaming());
                });
                assert!(rename_requests(&requests).is_empty());
                // Returning cannot resurrect or commit the prior session's partial name.
                root.simulate_keystroke(&format!("{modifier}-p"));
                root.simulate_input("Choice Alpha");
                app.run_until_parked();
                root.draw();
                root.simulate_keystroke("enter");
                app.run_until_parked();
                focus_frames(&mut root);
                app.update(|cx| {
                    let s = shell.read(cx);
                    assert_eq!(s.active, "a");
                    assert!(!s.header.as_ref().unwrap().read(cx).renaming());
                });
                assert!(rename_requests(&requests).is_empty());
            } else {
                root.simulate_input(" completed");
                root.simulate_keystroke("enter");
                app.run_until_parked();
                focus_frames(&mut root);
                let sent = rename_requests(&requests);
                assert_eq!(sent.len(), 1);
                assert_eq!(sent[0]["name"], "unfinished name completed");
                app.update(|cx| {
                    assert!(!shell.read(cx).header.as_ref().unwrap().read(cx).renaming());
                });
                root.simulate_keystroke("enter");
                app.run_until_parked();
                assert!(rename_requests(&requests).is_empty());
            }
        }
    }
}

#[test]
fn pointer_outside_commits_old_name_once_but_palette_preserves_rename() {
    for target in ["sidebar-blank", "new-session", "palette"] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| crate::app::tests::shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                let mut p = project();
                p.sessions.push(
                    serde_json::from_value(json!({
                        "id":"a", "path":"/a", "title":"Original name",
                        "lastActive":"now", "sortAt":0, "lastUserMessageAt":0,
                        "archived":false, "selected":true
                    }))
                    .unwrap(),
                );
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![p],
                    active_project_path: project().path,
                    pi_path: None,
                });
                create(s, "a", "retained draft", w, cx);
                started(s, "a", 1, cx);
                s.ingest(
                    rpc("a", 1, json!({
                        "type":"response", "id":"tau:a:g1:1",
                        "command":"get_state", "success":true,
                        "data":{"sessionId":"a", "sessionFile":"/a", "sessionName":"Original name"}
                    })),
                    cx,
                );
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
        root.simulate_keystroke(&format!("{modifier}-shift-r"));
        app.run_until_parked();
        root.draw();
        root.simulate_input("Pointer rename");
        app.run_until_parked();
        while requests.try_recv().is_ok() {}
        root.update(|_, w, cx| {
            let header = shell.read(cx).header.as_ref().unwrap().read(cx);
            assert!(header.editor().read(cx).focus_handle(cx).is_focused(w));
        });

        let point = root.update(|_, w, _| w.find(target).bounds().center());
        root.simulate_click(point, MouseButton::Left);
        app.run_until_parked();
        focus_frames(&mut root);

        if target == "palette" {
            assert!(rename_requests(&requests).is_empty());
            // A real pointer click inside the palette must also bypass outside capture.
            let search = root.update(|_, w, _| w.find("search").bounds().center());
            root.simulate_click(search, MouseButton::Left);
            root.simulate_input("Choice");
            app.run_until_parked();
            focus_frames(&mut root);
            root.update(|_, w, cx| {
                let s = shell.read(cx);
                assert_eq!(s.active, "a");
                assert!(s.palette.is_open(cx));
                let header = s.header.as_ref().unwrap().read(cx);
                assert!(header.renaming());
                assert_eq!(header.editor().read(cx).value().as_ref(), "Pointer rename");
                let search = s.palette.view.as_ref().unwrap().read(cx).search_editor();
                assert!(search.read(cx).focus_handle(cx).is_focused(w));
                assert_eq!(search.read(cx).value().as_ref(), "Choice");
            });
            assert!(rename_requests(&requests).is_empty());
        } else {
            let sent = rename_requests(&requests);
            assert_eq!(sent.len(), 1, "{target}");
            assert_eq!(sent[0]["name"], "Pointer rename");
            assert!(sent[0]["id"].as_str().unwrap().starts_with("tau:a:"));
            root.update(|_, w, cx| {
                let s = shell.read(cx);
                assert!(!s.header.as_ref().unwrap().read(cx).renaming());
                assert_eq!(
                    s.sessions["a"].controller.state().identity.name,
                    "Pointer rename"
                );
                assert_eq!(s.sessions["a"].controller.state().draft, "retained draft");
                assert!(
                    !s.sessions["a"]
                        .composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w)
                );
                if target == "new-session" {
                    assert_ne!(s.active, "a");
                    assert!(s.shell_focus.as_ref().unwrap().is_focused(w));
                } else {
                    assert_eq!(s.active, "a");
                }
            });
            if target == "sidebar-blank" {
                root.simulate_click(point, MouseButton::Left);
                app.run_until_parked();
                focus_frames(&mut root);
            }
            assert!(rename_requests(&requests).is_empty());
        }
    }
}
