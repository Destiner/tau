//! Exercise session navigation through painted chrome under the production Root.
use super::confirm_queue_fixture::Fixture;
use super::*;
use crate::app::tests::{create, rpc, started};
use gpui_kit::{Focusable, MouseButton, test::TestWindowExt};
use serde_json::json;

fn warm_pair() -> Fixture {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            // Saved Pi identities and timestamps give the real sidebar a stable A → B order.
            s.workspace.as_mut().unwrap().projects[0].sessions =
                [("a", "Focus Alpha", 2), ("b", "Focus Beta", 1)]
                    .into_iter()
                    .map(|(key, title, order)| {
                        serde_json::from_value(json!({
                            "id":format!("saved-{key}"), "path":format!("/fixture/{key}.jsonl"),
                            "title":title, "lastActive":"now", "sortAt":order,
                            "lastUserMessageAt":0, "archived":false, "selected":key == "a"
                        }))
                        .unwrap()
                    })
                    .collect();
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .set_draft("  A 🌍\nsecond  ");
            s.sessions.get_mut("a").unwrap().dirty = true;
            create(s, "b", "  B 🦀\nsecond  ", w, cx);
            started(s, "b", 9, cx);
            s.sync(w, cx);
        });
    });
    while let Ok(command) = f.requests.try_recv() {
        let Command::Send {
            runtime,
            generation,
            request,
        } = command
        else {
            continue;
        };
        let kind = request["type"].as_str().unwrap();
        let data = match kind {
            "get_state" => {
                json!({"sessionId":"saved-b", "sessionFile":"/fixture/b.jsonl", "isStreaming":false})
            }
            "get_messages" => json!({"messages":[]}),
            "get_available_models" | "get_scoped_models" => json!({"models":[]}),
            "get_available_thinking_levels" => json!({"levels":["off"]}),
            "get_commands" => json!({"commands":[]}),
            other => panic!("unexpected bootstrap: {other}"),
        };
        f.root.update(|_, w, cx| f.shell.update(cx, |s, cx| {
            s.ingest(rpc(&runtime, generation, json!({"type":"response", "id":request["id"], "command":kind, "success":true, "data":data})), cx);
            s.sync(w, cx);
        }));
    }
    f.click("session-status-saved-a");
    settle(&mut f);
    f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert!(s.sessions["a"].controller.state().ready);
        assert!(s.sessions["b"].controller.state().ready);
        assert_eq!(s.active, "a");
    });
    f
}

fn settle(f: &mut Fixture) {
    for _ in 0..4 {
        f.root.update(|_, w, cx| w.simulate_next_frame(cx));
        f.root.draw();
    }
}

fn assert_composer(f: &mut Fixture, key: &str) {
    f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.active, key);
        assert!(
            s.sessions[key]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
}

#[test]
fn warm_session_pointer_navigation_routes_real_typing_without_editor_click() {
    let mut f = warm_pair();
    let ids = f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        (
            s.sessions["a"].composer.read(cx).editor().entity_id(),
            s.sessions["b"].composer.read(cx).editor().entity_id(),
        )
    });
    f.click("session-status-saved-b");
    settle(&mut f);
    assert_composer(&mut f, "b");
    f.press(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    f.root.simulate_input("  typed B 🦀\nexact  ");
    f.root.draw();
    f.click("session-status-saved-a");
    settle(&mut f);
    assert_composer(&mut f, "a");
    f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.sessions["a"].controller.state().draft, "  A 🌍\nsecond  ");
        assert_eq!(
            s.sessions["b"].controller.state().draft,
            "  typed B 🦀\nexact  "
        );
        assert_eq!(
            s.sessions["a"].composer.read(cx).editor().entity_id(),
            ids.0
        );
        assert_eq!(
            s.sessions["b"].composer.read(cx).editor().entity_id(),
            ids.1
        );
    });
}

#[test]
fn warm_session_keyboard_navigation_routes_real_typing_without_editor_click() {
    let mut f = warm_pair();
    // The real focus-sidebar command, then the sidebar's own row traversal/activation.
    f.press(if cfg!(target_os = "macos") {
        "cmd-shift-l"
    } else {
        "ctrl-shift-l"
    });
    for _ in 0..24 {
        if f.root
            .update(|_, w, _| w.find("session-saved-b").focused().unwrap_or(false))
        {
            break;
        }
        f.press("tab");
    }
    f.root
        .update(|_, w, _| assert!(w.find("session-saved-b").focused().unwrap()));
    f.press("enter");
    settle(&mut f);
    assert_composer(&mut f, "b");
    f.press(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    f.root.simulate_input("  keyboard B 🦀\nexact  ");
    f.root.draw();
    f.click("session-status-saved-a");
    settle(&mut f);
    f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.sessions["a"].controller.state().draft, "  A 🌍\nsecond  ");
        assert_eq!(
            s.sessions["b"].controller.state().draft,
            "  keyboard B 🦀\nexact  "
        );
    });
}

fn replace_focused_draft(f: &mut Fixture, key: &str, text: &str) {
    assert_composer(f, key);
    f.press(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    f.root.simulate_input(text);
    f.root.draw();
    f.root.update(|_, _, cx| {
        assert_eq!(
            f.shell.read(cx).sessions[key].controller.state().draft,
            text
        );
    });
}

#[test]
fn warm_session_cmd_p_navigation_routes_real_typing_without_editor_click() {
    let mut f = warm_pair();
    for (key, title, draft, retained_key, retained_draft) in [
        (
            "b",
            "Focus Beta",
            "  palette B 🦀\nexact  ",
            "a",
            "  A 🌍\nsecond  ",
        ),
        (
            "a",
            "Focus Alpha",
            "  palette A 🌍\nexact  ",
            "b",
            "  palette B 🦀\nexact  ",
        ),
    ] {
        f.press(if cfg!(target_os = "macos") {
            "cmd-p"
        } else {
            "ctrl-p"
        });
        f.root
            .update(|_, _, cx| assert!(f.shell.read(cx).palette.is_open(cx)));
        f.root.simulate_input(title);
        f.root.draw();
        f.root.update(|_, _, cx| {
            let s = f.shell.read(cx);
            let selected = s
                .palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_id()
                .unwrap();
            let live = &s.sessions[key];
            let identity = &live.controller.state().identity;
            assert_eq!(
                selected,
                format!(
                    "live:Live({key:?}, {}, {:?}, {:?})",
                    live.controller.generation(),
                    identity.id,
                    identity.path
                )
            );
        });
        f.press("enter");
        settle(&mut f);
        f.root
            .update(|_, _, cx| assert!(!f.shell.read(cx).palette.is_open(cx)));
        replace_focused_draft(&mut f, key, draft);
        f.root.update(|_, _, cx| {
            assert_eq!(
                f.shell.read(cx).sessions[retained_key]
                    .controller
                    .state()
                    .draft,
                retained_draft
            );
        });
    }
}

#[test]
fn warm_session_ctrl_tab_and_ctrl_shift_tab_route_real_typing_without_editor_click() {
    let mut f = warm_pair();
    f.press("ctrl-tab");
    settle(&mut f);
    replace_focused_draft(&mut f, "b", "  next B 🦀\nexact  ");
    f.root.update(|_, _, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"].controller.state().draft,
            "  A 🌍\nsecond  "
        );
    });
    f.press("ctrl-shift-tab");
    settle(&mut f);
    replace_focused_draft(&mut f, "a", "  previous A 🌍\nexact  ");
    f.root.update(|_, _, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["b"].controller.state().draft,
            "  next B 🦀\nexact  "
        );
    });
}

#[test]
fn warm_navigation_returns_to_retained_extension_owner_not_hidden_composer() {
    let mut f = warm_pair();
    f.dialog("input");
    let (dialog, origin) = f.root.update(|_, w, cx| {
        (
            f.shell.read(cx).sessions["a"]
                .extension
                .as_ref()
                .unwrap()
                .1
                .clone(),
            w.focused(cx),
        )
    });
    f.click("session-status-saved-b");
    settle(&mut f);
    assert_composer(&mut f, "b");
    f.click("session-status-saved-a");
    settle(&mut f);
    f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.sessions["a"].extension.as_ref().unwrap().1, dialog);
        assert_eq!(w.focused(cx), origin);
        assert_eq!(s.sessions["a"].controller.state().draft, "  A 🌍\nsecond  ");
        assert!(w.try_find("shell-composer-area").is_none());
    });
}

#[test]
fn keyboard_navigation_handoff_does_not_steal_a_newer_pointer_focus_owner() {
    for palette in [false, true] {
        let mut f = warm_pair();
        if palette {
            f.press(if cfg!(target_os = "macos") {
                "cmd-p"
            } else {
                "ctrl-p"
            });
            f.root.simulate_input("Focus Beta");
            f.root.draw();
            f.press("enter");
        } else {
            f.press("ctrl-tab");
        }
        f.root
            .update(|_, _, cx| assert_eq!(f.shell.read(cx).active, "b"));
        // The next pointer event owns focus before either handoff frame runs.
        let control = f
            .root
            .update(|_, w, _| w.find("sidebar-resize-handle").bounds().center());
        f.root.simulate_click(control, MouseButton::Left);
        f.root
            .update(|_, w, _| assert!(w.find("sidebar-resize-handle").focused().unwrap()));
        let owner = f.root.update(|_, w, cx| w.focused(cx));
        settle(&mut f);
        f.root.update(|_, w, cx| assert_eq!(w.focused(cx), owner));
    }
}

#[test]
fn navigation_handoff_does_not_steal_a_newer_pointer_focus_owner() {
    let mut f = warm_pair();
    // Do not advance the handoff frames between navigation and the user's next click.
    let target = f
        .root
        .update(|_, w, _| w.find("session-status-saved-b").bounds().center());
    f.root.simulate_click(target, MouseButton::Left);
    let control = f
        .root
        .update(|_, w, _| w.find("sidebar-resize-handle").bounds().center());
    f.root.simulate_click(control, MouseButton::Left);
    f.root
        .update(|_, w, _| assert!(w.find("sidebar-resize-handle").focused().unwrap()));
    let owner = f.root.update(|_, w, cx| w.focused(cx));
    settle(&mut f);
    f.root.update(|_, w, cx| assert_eq!(w.focused(cx), owner));
}
