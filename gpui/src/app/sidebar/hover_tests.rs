use super::*;
use crate::app::tests::{app, project, rpc, shell, started};
use gpui_kit::{MouseButton, TestApp, TestAppWindow, size, test::TestWindowExt};
use serde_json::json;

fn saved(id: &str, timestamp: u64) -> tau_core::models::SessionSummary {
    serde_json::from_value(json!({
        "id":id,"path":format!("/{id}"),"title":id,"lastActive":"now",
        "sortAt":timestamp,"lastUserMessageAt":0,"archived":false,"selected":false
    }))
    .unwrap()
}

fn fixture() -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Shell>,
    async_channel::Receiver<Command>,
) {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(800.)));
    root.update(|_, w, cx| {
        w.bounds_changed(cx);
        shell.update(cx, |s, cx| {
            let mut upper = project();
            upper.sessions = vec![saved("a", 400_000), saved("b", 300_000)];
            let mut empty = project();
            empty.path = "/empty".into();
            s.workspace = Some(WorkspaceSnapshot {
                active_project_path: upper.path.clone(),
                projects: vec![upper, empty],
                pi_path: None,
            });
            s.mount(w, cx);
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        });
    });
    root.draw();
    (app, root, shell, requests)
}

fn displayed(root: &mut TestAppWindow<Root>, shell: &Entity<Shell>) -> Vec<String> {
    root.update(|_, _, cx| {
        shell
            .read(cx)
            .sidebar
            .as_ref()
            .unwrap()
            .read(cx)
            .displayed_sessions(&project().path)
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect()
    })
}

#[test]
fn root_initial_inactive_entry_holds_current_content_and_hides_arrivals_including_empty_projects() {
    let (_app, mut root, shell, _requests) = fixture();
    root.update(|_, w, _| assert!(!w.is_window_active()));
    let point = root.update(|_, w, _| w.find("session-a").bounds().center());
    root.simulate_mouse_move(point);
    root.draw();
    let lower = root.update(|_, w, _| w.find("project-row-/empty").bounds());
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.select("b", w, cx);
            s.sessions
                .get_mut(&s.active)
                .unwrap()
                .controller
                .set_draft("kept draft");
            let binding = SessionIdentity::live(&s.active, &s.sessions[&s.active]);
            s.sidebar_state.last_user.insert(binding, 900_000);
            let workspace = s.workspace.as_mut().unwrap();
            workspace.projects[0]
                .sessions
                .push(saved("arrival", 1_000_000));
            workspace.projects[1]
                .sessions
                .push(saved("empty-arrival", 1_000_000));
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.simulate_keystroke("shift");
    root.draw();
    assert_eq!(displayed(&mut root, &shell), ["a", "b"]);
    root.update(|_, w, cx| {
        assert_eq!(w.find("project-row-/empty").bounds(), lower);
        assert!(w.try_find("session-empty-arrival").is_none());
        let s = shell.read(cx);
        assert_eq!(s.sessions[&s.active].controller.state().draft, "kept draft");
        let row = s
            .sidebar
            .as_ref()
            .unwrap()
            .read(cx)
            .displayed_sessions(&project().path)
            .unwrap()
            .remove(1);
        assert_eq!(row.status, Some(SessionStatus::Draft));
    });
    let footer = root.update(|_, w, _| w.find("sidebar-footer").bounds().center());
    root.simulate_mouse_move(footer);
    root.draw();
    assert_eq!(displayed(&mut root, &shell), ["arrival", "b", "a"]);
    root.update(|_, w, _| {
        w.find("session-empty-arrival");
    });
}

fn arrive(root: &mut TestAppWindow<Root>, shell: &Entity<Shell>, id: &str, timestamp: u64) {
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace.as_mut().unwrap().projects[0]
                .sessions
                .push(saved(id, timestamp));
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.draw();
}

#[test]
fn root_deactivated_or_exited_window_cannot_recapture_a_hold_from_deferred_hover() {
    for exit in [false, true] {
        let (_app, mut root, shell, _requests) = fixture();
        // Only the deactivation case needs an actual active-to-inactive transition.
        if !exit {
            root.simulate_activation(true);
        }
        let point = root.update(|_, w, _| w.find("session-a").bounds().center());
        root.simulate_mouse_move(point);
        root.draw();
        arrive(&mut root, &shell, "held-arrival", 450_000);
        assert_eq!(displayed(&mut root, &shell), ["a", "b"]);
        if exit {
            root.simulate_event(gpui_kit::MouseExitEvent {
                position: gpui_kit::point(px(-1.), point.y),
                pressed_button: None,
                modifiers: Default::default(),
            });
        } else {
            root.simulate_activation(false);
        }
        root.update(|_, w, _| assert!(!w.is_window_active()));
        // GPUI can replay hover from its retained pointer position on later frames.
        for _ in 0..2 {
            root.update(|_, _, cx| {
                let sidebar = shell.read(cx).sidebar.as_ref().unwrap().clone();
                sidebar.update(cx, |s, cx| s.body_hover(true, cx));
            });
            root.draw();
        }
        arrive(&mut root, &shell, "arrival", 500_000);
        assert_eq!(
            displayed(&mut root, &shell),
            ["arrival", "held-arrival", "a", "b"]
        );
    }
}

#[test]
fn root_real_inactive_reentry_holds_after_deactivation_or_exit_without_activation() {
    for exit in [false, true] {
        for via_footer in [false, true] {
            let (_app, mut root, shell, _requests) = fixture();
            if !exit {
                root.simulate_activation(true);
            }
            let point = root.update(|_, w, _| w.find("session-a").bounds().center());
            root.simulate_mouse_move(point);
            root.draw();
            if exit {
                root.simulate_event(gpui_kit::MouseExitEvent {
                    position: gpui_kit::point(px(-1.), point.y),
                    pressed_button: None,
                    modifiers: Default::default(),
                });
            } else {
                root.simulate_activation(false);
            }
            arrive(&mut root, &shell, "arrival", 500_000);
            assert_eq!(displayed(&mut root, &shell), ["arrival", "a", "b"]);
            if via_footer {
                let footer = root.update(|_, w, _| w.find("sidebar-footer").bounds().center());
                root.simulate_mouse_move(footer);
                root.draw();
            }
            // The direct path also covers real motion at GPUI's retained position:
            // a new hover transition is not required to clear the stale-event fence.
            root.simulate_mouse_move(point);
            root.draw();
            root.update(|_, w, _| assert!(!w.is_window_active()));
            arrive(&mut root, &shell, "later", 600_000);
            assert_eq!(displayed(&mut root, &shell), ["arrival", "a", "b"]);
        }
    }
}

#[test]
fn root_unregistered_materialized_phase_uses_activity_not_phantom_creation_rank() {
    let (_app, mut root, shell, _requests) = fixture();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            for (runtime, id, timestamp) in
                [("z-old", "older", 100_000), ("a-new", "newer", 350_000)]
            {
                s.create_live(
                    runtime.into(),
                    project(),
                    String::new(),
                    String::new(),
                    String::new(),
                    w,
                    cx,
                );
                started(s, runtime, 1, cx);
                s.sessions
                    .get_mut(runtime)
                    .unwrap()
                    .controller
                    .bind_source(id, format!("/{id}"));
                if id == "older" {
                    s.ingest(
                        rpc(
                            runtime,
                            1,
                            json!({"type":"message_start", "message":{
                                "role":"user", "timestamp":100_000, "content":"request"
                            }}),
                        ),
                        cx,
                    );
                }
                let binding = SessionIdentity::live(runtime, &s.sessions[runtime]);
                s.sidebar_state.phase_at.insert(binding, timestamp);
            }
            s.create_live(
                "phantom".into(),
                project(),
                String::new(),
                String::new(),
                String::new(),
                w,
                cx,
            );
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
            let (projects, details) = s.sidebar_projection(500_000, &runtime_index(&s.sessions));
            assert_eq!(details["older"].relative_time.as_deref(), Some("6m"));
            assert_eq!(details["newer"].relative_time, None);
            assert_eq!(
                projects[0]
                    .sessions
                    .iter()
                    .map(|row| row.id.as_str())
                    .collect::<Vec<_>>(),
                ["phantom", "a", "newer", "b", "older"]
            );
        })
    });
    root.draw();
    assert_eq!(
        displayed(&mut root, &shell),
        ["phantom", "a", "newer", "b", "older"]
    );
}

#[test]
fn root_context_menu_survives_unrelated_activity_and_removes_missing_target() {
    let (_app, mut root, shell, _requests) = fixture();
    let point = root.update(|_, w, _| w.find("session-a").bounds().center());
    root.simulate_mouse_move(point);
    root.simulate_click(point, MouseButton::Right);
    root.draw();
    let menu = root.update(|_, _, cx| {
        shell
            .read(cx)
            .sidebar
            .as_ref()
            .unwrap()
            .read(cx)
            .context_menu()
    });
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace.as_mut().unwrap().projects[0].sessions[1].title =
                "updated elsewhere".into();
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.draw();
    root.update(|_, _, cx| assert!(menu.read(cx).is_open()));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace.as_mut().unwrap().projects[0].sessions[0].archived = true;
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.draw();
    root.update(|_, _, cx| assert!(!menu.read(cx).is_open()));
}
