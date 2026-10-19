use super::*;
use crate::app::tests::{app, project, rpc, shell, started};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Bounds, MouseButton, Pixels, TestApp, TestAppWindow, size};
use serde_json::json;

type Painted = Vec<(String, Bounds<Pixels>)>;

fn saved(id: &str, timestamp: u64) -> tau_core::models::SessionSummary {
    serde_json::from_value(json!({
        "id":id,"path":format!("/{id}"),"title":id,"lastActive":"now",
        "sortAt":timestamp,"lastUserMessageAt":timestamp,"archived":false,"selected":false
    }))
    .unwrap()
}

fn workspace() -> WorkspaceSnapshot {
    let mut upper = project();
    upper.sessions = [
        ("first", 400_000),
        ("second", 300_000),
        ("third", 200_000),
        ("fourth", 100_000),
    ]
    .into_iter()
    .map(|(id, ts)| saved(id, ts))
    .collect();
    let mut lower = project();
    lower.path = "/lower".into();
    lower.name = "Lower".into();
    lower.sessions = vec![saved("lower-saved", 50_000)];
    WorkspaceSnapshot {
        projects: vec![upper.clone(), lower],
        active_project_path: upper.path,
        pi_path: None,
    }
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
            s.window = Some(w.window_handle());
            s.workspace = Some(workspace());
            s.mount(w, cx);
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        });
    });
    root.draw();
    (app, root, shell, requests)
}

fn painted(root: &mut TestAppWindow<Root>, ids: &[&str]) -> Painted {
    root.update(|_, w, _| {
        ids.iter()
            .map(|id| ((*id).into(), w.find((*id).to_owned()).bounds()))
            .collect()
    })
}

fn assert_painted(root: &mut TestAppWindow<Root>, expected: &Painted) {
    root.draw();
    let ids: Vec<_> = expected.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(painted(root, &ids), *expected);
    let rows: Vec<_> = expected
        .iter()
        .filter(|(id, _)| id.starts_with("session-status-"))
        .collect();
    for pair in rows.windows(2) {
        assert!(pair[0].1.top() < pair[1].1.top());
    }
}

fn hydrate_painted(
    root: &mut TestAppWindow<Root>,
    shell: &Entity<Shell>,
    requests: &async_channel::Receiver<Command>,
    runtime: &str,
    id: &str,
    expected: &Painted,
) {
    let mut replies = 0;
    while let Ok(command) = requests.try_recv() {
        let Command::Send {
            runtime: target,
            generation,
            request,
        } = command
        else {
            continue;
        };
        assert_eq!(target, runtime);
        let command = request["type"].as_str().unwrap();
        let data = match command {
            "get_state" => json!({"sessionId":id,"sessionFile":format!("/{id}"),"sessionName":id}),
            "get_messages" => {
                json!({"messages":[{"role":"user","timestamp":900_000,"content":[{"type":"text","text":"history"}]}]})
            }
            "get_available_models" | "get_scoped_models" => json!({"models":[],"enabled":false}),
            "get_commands" => json!({"commands":[]}),
            "get_available_thinking_levels" => json!({"levels":[]}),
            other => panic!("unexpected bootstrap request: {other}"),
        };
        root.update(|_, w, cx| shell.update(cx, |s, cx| {
            s.ingest(rpc(runtime, generation, json!({"type":"response","id":request["id"],"command":command,"success":true,"data":data})), cx);
            s.sync(w, cx);
        }));
        assert_painted(root, expected);
        replies += 1;
        assert!(replies < 30);
    }
    assert!(replies >= 6);
    root.update(|_, _, cx| {
        let s = shell.read(cx);
        assert!(s.sessions[runtime].controller.state().ready);
        assert!(!s.sessions[runtime].controller.state().history_loading);
    });
}

#[test]
fn root_hover_four_cold_and_four_warm_pointer_selections_preserve_exact_painted_rows() {
    let (_app, mut root, shell, requests) = fixture();
    let expected = painted(
        &mut root,
        &[
            "session-status-first",
            "session-status-second",
            "session-status-third",
            "session-status-fourth",
            "project-row-/lower",
            "session-status-lower-saved",
        ],
    );
    let pointer = root.update(|_, w, _| w.find("session-first").bounds().center());
    root.simulate_mouse_move(pointer);
    root.draw();
    let mut runtimes = HashMap::new();
    for id in [
        "fourth", "second", "third", "first", "fourth", "third", "second", "first",
    ] {
        let pointer = root.update(|_, w, _| w.find(format!("session-{id}")).bounds().center());
        root.simulate_mouse_move(pointer);
        root.simulate_click(pointer, MouseButton::Left);
        assert_painted(&mut root, &expected);
        let runtime = root.update(|_, _, cx| {
            let s = shell.read(cx);
            let runtime = s.active.clone();
            assert_eq!(
                SessionIdentity::live(&runtime, &s.sessions[&runtime]).pi_id,
                id
            );
            assert_eq!(
                s.sidebar_projection(600_000, &runtime_index(&s.sessions)).0[0]
                    .sessions
                    .iter()
                    .map(|row| row.id.as_str())
                    .collect::<Vec<_>>(),
                ["first", "second", "third", "fourth"]
            );
            runtime
        });
        if let Some(previous) = runtimes.get(id) {
            assert_eq!(previous, &runtime);
            while let Ok(command) = requests.try_recv() {
                assert!(!matches!(command, Command::Send { .. }));
            }
        } else {
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    started(s, &runtime, 1, cx);
                    s.sync(w, cx);
                })
            });
            assert_painted(&mut root, &expected);
            hydrate_painted(&mut root, &shell, &requests, &runtime, id, &expected);
            runtimes.insert(id, runtime);
        }
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.ingest(Event::Workspace(Ok(workspace())), cx);
                s.sync(w, cx);
                assert!(s.sidebar_state.last_user.is_empty());
            })
        });
        assert_painted(&mut root, &expected);
    }
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let mut refresh = workspace();
            refresh.projects[0]
                .sessions
                .insert(0, saved("arrival", 1_000_000));
            s.ingest(Event::Workspace(Ok(refresh)), cx);
            s.sync(w, cx);
        })
    });
    assert_painted(&mut root, &expected);
    root.update(|_, w, _| assert!(w.try_find("session-status-arrival").is_none()));
    let footer = root.update(|_, w, _| w.find("sidebar-footer").bounds().center());
    root.simulate_mouse_move(footer);
    root.draw();
    root.update(|_, w, _| {
        assert!(
            w.find("session-status-arrival").bounds().top()
                < w.find("session-status-first").bounds().top()
        );
    });
}

#[test]
fn root_hover_negative_control_legacy_runtime_row_id_breaks_the_painted_oracle() {
    let (_app, mut root, shell, _requests) = fixture();
    let before = painted(
        &mut root,
        &[
            "session-status-first",
            "session-status-second",
            "project-row-/lower",
        ],
    );
    let pointer = root.update(|_, w, _| w.find("session-first").bounds().center());
    root.simulate_mouse_move(pointer);
    root.simulate_click(pointer, MouseButton::Left);
    assert_painted(&mut root, &before);
    root.update(|_, _, cx| {
        shell.update(cx, |s, cx| {
            let (mut projects, _) = s.sidebar_projection(600_000, &runtime_index(&s.sessions));
            let first = projects[0]
                .sessions
                .iter_mut()
                .find(|row| row.id == "first")
                .unwrap();
            // Reproduce the old cold-selection projection, without weakening the positive oracle.
            first.id = s.active.clone();
            s.sidebar
                .as_ref()
                .unwrap()
                .update(cx, |sidebar, cx| sidebar.set_projects(projects, cx));
        })
    });
    root.draw();
    root.update(|_, w, _| {
        assert!(w.try_find("session-status-first").is_none());
        assert_eq!(
            w.find("session-status-second").bounds().top(),
            before[0].1.top()
        );
        assert_eq!(
            w.find("project-row-/lower").bounds().top(),
            before[2].1.top() - px(29.)
        );
    });
}

#[test]
fn root_archive_workflow_predecessor_is_not_the_active_replacement_or_a_held_alias() {
    for target in ["plan", "implement"] {
        for success in [false, true] {
            let (_app, mut root, shell, requests) = fixture();
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    s.workspace.as_mut().unwrap().projects[0].sessions =
                        vec![saved("plan", 400_000), saved("implement", 300_000)];
                    s.sidebar_state.invalidate_metadata();
                    s.select("plan", w, cx);
                    s.sync(w, cx);
                })
            });
            root.draw();
            let runtime = root.update(|_, _, cx| shell.read(cx).active.clone());
            let expected = painted(
                &mut root,
                &["session-status-plan", "session-status-implement"],
            );
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    started(s, &runtime, 1, cx);
                    s.sync(w, cx);
                })
            });
            hydrate_painted(&mut root, &shell, &requests, &runtime, "plan", &expected);
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    s.ingest(rpc(&runtime, 1, json!({"type":"session_info_changed"})), cx);
                    let request = loop {
                        if let Command::Send { request, .. } = requests.try_recv().unwrap()
                            && request["type"] == "get_state"
                        {
                            break request;
                        }
                    };
                    s.ingest(
                        rpc(
                            &runtime,
                            1,
                            json!({"type":"response","id":request["id"],
                    "command":"get_state","success":true,"data":{"sessionId":"implement",
                    "sessionFile":"/implement","sessionName":"implement"}}),
                        ),
                        cx,
                    );
                    s.sessions
                        .get_mut(&runtime)
                        .unwrap()
                        .controller
                        .set_draft("replacement draft");
                    s.sync(w, cx);
                    assert_eq!(s.sessions[&runtime].source_id, "implement");
                    let predecessor = s.saved_session("plan").unwrap();
                    assert_eq!(predecessor.1.id, "plan");
                    assert_eq!(predecessor.1.path, "/plan");
                    assert_eq!(predecessor.1.title, "plan");
                    assert_eq!(predecessor.2, None);
                    assert_eq!(s.saved_session(&runtime).unwrap().1.id, "implement");
                })
            });
            root.draw();
            let pointer =
                root.update(|_, w, _| w.find(format!("session-{target}")).bounds().center());
            root.simulate_mouse_move(pointer);
            root.draw();
            let pointer =
                root.update(|_, w, _| w.find(format!("archive-{target}")).bounds().center());
            root.simulate_mouse_move(pointer);
            root.simulate_click(pointer, MouseButton::Left);
            root.draw();
            let (project, revision) = loop {
                if let Command::Archive {
                    project,
                    id,
                    revision,
                    archived,
                } = requests.try_recv().unwrap()
                {
                    assert_eq!(id, target);
                    assert!(archived);
                    break (project, revision);
                }
            };
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    assert_eq!(s.active == runtime, target == "plan");
                    assert_eq!(
                        s.sessions[&runtime].controller.state().draft,
                        "replacement draft"
                    );
                    assert_eq!(
                        s.sessions[&runtime].controller.state().identity.id,
                        "implement"
                    );
                    assert_eq!(s.sessions[&runtime].controller.generation(), 1);
                    let mut refresh = s.workspace.clone().unwrap();
                    for row in &mut refresh.projects[0].sessions {
                        row.archived = false;
                    }
                    s.ingest(Event::Workspace(Ok(refresh)), cx);
                    s.sync(w, cx);
                    assert!(
                        s.workspace.as_ref().unwrap().projects[0]
                            .sessions
                            .iter()
                            .find(|row| row.id == target)
                            .unwrap()
                            .archived
                    );
                })
            });
            root.draw();
            root.update(|_, w, _| {
                assert!(w.try_find(format!("session-status-{target}")).is_none());
                let survivor = if target == "plan" {
                    "implement"
                } else {
                    "plan"
                };
                assert_eq!(
                    w.find(format!("session-status-{survivor}")).bounds().top(),
                    expected[0].1.top()
                );
            });
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    s.finish_archive(project, target.into(), revision, true, success);
                    s.sync(w, cx);
                    assert_eq!(
                        s.workspace.as_ref().unwrap().projects[0]
                            .sessions
                            .iter()
                            .find(|row| row.id == target)
                            .unwrap()
                            .archived,
                        success
                    );
                    assert_eq!(s.workspace.as_ref().unwrap().projects[0].sessions.len(), 2);
                    assert_eq!(
                        s.sessions[&runtime].controller.state().draft,
                        "replacement draft"
                    );
                })
            });
            root.draw();
            root.update(|_, w, _| {
                assert_eq!(
                    w.try_find(format!("session-status-{target}")).is_none(),
                    success
                );
            });
            if target == "plan" {
                root.update(|_, w, cx| {
                    shell.update(cx, |s, cx| {
                        // Two pending writes: an older restore succeeds, then the latest archive fails.
                        s.session_action(&Action::Restore("plan".into()), w, cx);
                        s.session_action(&Action::Archive("plan".into()), w, cx);
                        let writes: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok())
                            .filter_map(|command| match command {
                                Command::Archive {
                                    project,
                                    id,
                                    revision,
                                    archived,
                                } => Some((project, id, revision, archived)),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(writes.len(), 2);
                        for (index, (project, id, revision, archived)) in
                            writes.into_iter().enumerate()
                        {
                            s.finish_archive(project, id, revision, archived, index == 0);
                            s.sync(w, cx);
                        }
                        assert_eq!(s.active, runtime);
                        assert_eq!(
                            s.sessions[&runtime].controller.state().draft,
                            "replacement draft"
                        );
                        assert!(!s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
                    })
                });
                assert_painted(&mut root, &expected);
            }
        }
    }
}

#[test]
fn root_held_phantom_materializes_then_exact_saved_predecessor_wins_replacement() {
    let (_app, mut root, shell, _requests) = fixture();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.create_live(
                "phantom-runtime".into(),
                project(),
                String::new(),
                String::new(),
                String::new(),
                w,
                cx,
            );
            s.sync(w, cx);
        })
    });
    root.draw();
    let before = painted(
        &mut root,
        &[
            "session-status-phantom-runtime",
            "session-status-first",
            "project-row-/lower",
            "session-status-lower-saved",
        ],
    );
    let pointer = root.update(|_, w, _| w.find("session-phantom-runtime").bounds().center());
    root.simulate_mouse_move(pointer);
    root.draw();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.sessions
                .get_mut("phantom-runtime")
                .unwrap()
                .controller
                .bind_source("materialized", "/materialized");
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    let mut materialized = before.clone();
    materialized[0].0 = "session-status-materialized".into();
    assert_painted(&mut root, &materialized);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let mut refresh = workspace();
            refresh.projects[0].sessions.push(saved("materialized", 1));
            s.ingest(Event::Workspace(Ok(refresh)), cx);
            s.sync(w, cx);
        })
    });
    assert_painted(&mut root, &materialized);
    // Recapture the semantic phase before reusing its runtime for a different Pi session.
    let footer = root.update(|_, w, _| w.find("sidebar-footer").bounds().center());
    root.simulate_mouse_move(footer);
    root.draw();
    let pointer = root.update(|_, w, _| w.find("session-materialized").bounds().center());
    root.simulate_mouse_move(pointer);
    root.draw();
    let captured = painted(
        &mut root,
        &[
            "session-status-first",
            "session-status-second",
            "session-status-third",
            "session-status-fourth",
            "session-status-materialized",
            "project-row-/lower",
            "session-status-lower-saved",
        ],
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.sessions
                .get_mut("phantom-runtime")
                .unwrap()
                .controller
                .bind_source("replacement", "/replacement");
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
            assert_eq!(
                s.sidebar_state.row_sources["materialized"],
                "phantom-runtime"
            );
            assert!(!s.sidebar_state.row_sources.contains_key("replacement"));
        })
    });
    assert_painted(&mut root, &captured);
    root.update(|_, w, _| {
        assert!(w.try_find("session-status-replacement").is_none());
        assert!(w.try_find("session-status-phantom-runtime").is_none());
    });
}
