use super::*;
use crate::app::tests::{app, create, project, shell};
use gpui_kit::{TestApp, TestAppWindow, size};
use serde_json::json;

type Fixture = (
    TestApp,
    TestAppWindow<Root>,
    Entity<Shell>,
    async_channel::Receiver<Command>,
);
fn workspace() -> WorkspaceSnapshot {
    let mut upper = project();
    upper.sessions = vec![
        serde_json::from_value(json!({
            "id":"saved", "path":"/saved.jsonl", "title":"Saved", "lastActive":"now",
            "sortAt":100, "lastUserMessageAt":100, "archived":false, "selected":false
        }))
        .unwrap(),
    ];
    let mut lower = project();
    lower.path = "/lower".into();
    lower.name = "Lower".into();
    WorkspaceSnapshot {
        active_project_path: upper.path.clone(),
        projects: vec![upper, lower],
        pi_path: None,
    }
}
fn fixture(capacity: usize) -> Fixture {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(capacity);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(800.)));
    root.update(|_, w, cx| {
        w.bounds_changed(cx);
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.workspace = Some(workspace());
            s.mount(w, cx);
            // Metadata tests assert settled paint before ack, not animation-t0 offsets.
            s.sidebar.as_ref().unwrap().update(cx, |sidebar, cx| {
                sidebar.set_animations_enabled(false, cx);
            });
            s.sync(w, cx);
        });
    });
    root.draw();
    // Mount owns an admin preference read. Complete it before testing queue pressure;
    // skipping it in revision() would hide a rejected first mutation in a one-slot queue.
    let Command::Admin(admin::Command::Load { revision }) = requests.try_recv().unwrap() else {
        panic!("mount must request admin preferences")
    };
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(
                Event::Admin(admin::Reply::Loaded {
                    revision,
                    enabled: false,
                }),
                cx,
            );
            s.sync(w, cx);
        })
    });
    root.draw();
    assert!(requests.is_empty(), "unexpected mount side effects");
    (app, root, shell, requests)
}
fn revision(requests: &async_channel::Receiver<Command>) -> u64 {
    let Command::WorkspaceMutation { revision, .. } = requests.try_recv().unwrap() else {
        panic!("mutation")
    };
    revision
}
fn ack(root: &mut TestAppWindow<Root>, shell: &Entity<Shell>, revision: u64, success: bool) {
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(Event::WorkspaceMutation { revision, success }, cx);
            s.sync(w, cx);
        })
    });
    root.draw();
}
fn session_node_bounds(w: &mut Window) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    gpui_kit::base::test_support::find(w, &[], &"session-saved".into()).map(|node| node.bounds())
}
fn session_bounds(root: &mut TestAppWindow<Root>) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    root.update(|_, w, _| session_node_bounds(w))
}
#[test]
fn root_fold_paints_before_ack_and_fences_late_failure_and_snapshot() {
    let (_app, mut root, shell, requests) = fixture(64);
    let expanded = session_bounds(&mut root).expect("saved row is painted");
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ProjectToggle(project().path), w, cx)
        })
    });
    let first = revision(&requests);
    root.draw();
    assert!(
        session_bounds(&mut root).is_none(),
        "collapse must paint before disk ack"
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ProjectToggle(project().path), w, cx)
        })
    });
    let second = revision(&requests);
    root.draw();
    assert_eq!(session_bounds(&mut root), Some(expanded));
    ack(&mut root, &shell, first, false);
    assert_eq!(
        session_bounds(&mut root),
        Some(expanded),
        "old failure cannot undo expansion"
    );
    ack(&mut root, &shell, second, true);
    ack(&mut root, &shell, first, true); // An already-resolved predecessor is inert.
    assert_eq!(session_bounds(&mut root), Some(expanded));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let mut stale = workspace();
            stale.projects[0].collapsed = true;
            s.ingest(Event::Workspace(Ok(stale)), cx);
            s.sync(w, cx);
        })
    });
    root.draw();
    assert_eq!(
        session_bounds(&mut root),
        Some(expanded),
        "late full snapshot cannot undo intent"
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ProjectToggle(project().path), w, cx)
        })
    });
    let third = revision(&requests);
    root.draw();
    assert!(session_bounds(&mut root).is_none());
    ack(&mut root, &shell, third, false);
    assert_eq!(
        session_bounds(&mut root),
        Some(expanded),
        "latest failure rolls back to confirmed state"
    );
}
#[test]
fn root_reorder_rolls_back_to_latest_confirmed_order_not_original_order() {
    let (_app, mut root, shell, requests) = fixture(64);
    let original = session_bounds(&mut root).unwrap();
    let swapped = vec!["/lower".into(), project().path];
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ReorderProjects(swapped.clone()), w, cx);
            assert_eq!(
                s.workspace
                    .as_ref()
                    .unwrap()
                    .projects
                    .iter()
                    .map(|project| project.path.clone())
                    .collect::<Vec<_>>(),
                swapped,
                "optimistic workspace order changes before ack"
            );
        })
    });
    let first = revision(&requests);
    root.draw();
    let moved = session_bounds(&mut root).unwrap();
    assert!(moved.top() > original.top(), "order changes before ack");
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(
                Action::ReorderProjects(vec![project().path, "/lower".into()]),
                w,
                cx,
            )
        })
    });
    let second = revision(&requests);
    root.draw();
    assert_eq!(session_bounds(&mut root), Some(original));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let mut stale = workspace();
            stale.projects.reverse();
            s.ingest(Event::Workspace(Ok(stale)), cx);
            s.sync(w, cx);
        })
    });
    root.draw();
    assert_eq!(
        session_bounds(&mut root),
        Some(original),
        "pending order fences stale snapshots"
    );
    ack(&mut root, &shell, first, true);
    assert_eq!(session_bounds(&mut root), Some(original));
    ack(&mut root, &shell, second, false);
    assert_eq!(session_bounds(&mut root), Some(moved));
    ack(&mut root, &shell, first, false); // Duplicate/stale callbacks are inert.
    assert_eq!(session_bounds(&mut root), Some(moved));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(Event::Workspace(Ok(workspace())), cx);
            s.sync(w, cx);
        })
    });
    root.draw();
    assert_eq!(
        session_bounds(&mut root),
        Some(moved),
        "rollback baseline fences late snapshots"
    );
}
#[test]
fn new_session_expands_and_selects_before_metadata_ack() {
    let (_app, mut root, shell, requests) = fixture(64);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace.as_mut().unwrap().projects[0].collapsed = true;
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.draw();
    assert!(session_bounds(&mut root).is_none());
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::NewSessionIn(project().path), w, cx);
            assert!(!s.active.is_empty());
            assert!(s.sessions.contains_key(&s.active));
            assert!(!s.workspace.as_ref().unwrap().projects[0].collapsed);
        })
    });
    root.draw();
    assert!(session_bounds(&mut root).is_some());
    let collapse = revision(&requests);
    let selection = revision(&requests);
    ack(&mut root, &shell, collapse, true);
    ack(&mut root, &shell, selection, false);
    root.update(|_, _, cx| {
        shell.update(cx, |s, _| {
            assert!(
                s.sessions.contains_key(&s.active),
                "selection persistence failure never removes the new editor"
            );
        })
    });
}

#[test]
fn saturated_dispatch_rolls_back_fold_without_changing_retained_draft() {
    let (_app, mut root, shell, requests) = fixture(1);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            create(s, "draft", "keep this draft", w, cx);
            // create() enqueues project selection first; folding cannot be enqueued.
            s.action(Action::ProjectToggle(project().path), w, cx);
            assert!(!s.workspace.as_ref().unwrap().projects[0].collapsed);
            assert_eq!(s.active, "draft");
            assert_eq!(
                s.sessions["draft"].controller.state().draft,
                "keep this draft"
            );
            assert!(s.error.is_some());
        })
    });
    assert!(matches!(
        requests.try_recv().unwrap(),
        Command::WorkspaceMutation { mutation: Mutation::Select { project: path, session }, .. }
            if path == project().path && session.is_empty()
    ));
    assert!(requests.is_empty());
    root.draw();
    assert!(session_bounds(&mut root).is_some());
}
fn check_rapid_intent<T: Clone + PartialEq + std::fmt::Debug>(initial: T, values: [T; 3]) {
    for successes in 0..8 {
        for replies in [
            [1, 2, 3],
            [1, 3, 2],
            [2, 1, 3],
            [2, 3, 1],
            [3, 1, 2],
            [3, 2, 1],
        ] {
            let mut intent = Intent {
                revision: 0,
                confirmed_revision: 0,
                confirmed: initial.clone(),
                desired: initial.clone(),
                rolled_back: false,
            };
            for (index, value) in values.iter().enumerate() {
                intent.begin(index as u64 + 1, value.clone());
            }
            let mut confirmed = 0;
            let mut latest_failed = false;
            for revision in replies {
                let success = successes & (1 << (revision - 1)) != 0;
                intent.finish(revision, values[revision as usize - 1].clone(), success);
                if success {
                    confirmed = confirmed.max(revision);
                }
                latest_failed |= revision == 3 && !success;
                let baseline = if confirmed == 0 {
                    &initial
                } else {
                    &values[confirmed as usize - 1]
                };
                assert_eq!(&intent.confirmed, baseline);
                assert_eq!(
                    &intent.desired,
                    if latest_failed { baseline } else { &values[2] }
                );
            }
        }
    }
}

#[test]
fn rapid_fold_and_order_failures_use_newest_success_in_every_reply_order() {
    check_rapid_intent(false, [true, false, true]);
    check_rapid_intent(
        vec!["a", "b", "c"],
        [
            vec!["b", "a", "c"],
            vec!["c", "b", "a"],
            vec!["a", "c", "b"],
        ],
    );
}

#[test]
fn queue_rejection_reconciles_predecessor_ack_without_undoing_next_intent() {
    let (_app, mut root, shell, requests) = fixture(1);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ProjectToggle(project().path), w, cx);
            s.action(Action::ProjectToggle(project().path), w, cx); // queue is full
        })
    });
    root.draw();
    assert!(session_bounds(&mut root).is_some());
    let first = revision(&requests);
    ack(&mut root, &shell, first, true);
    assert!(
        session_bounds(&mut root).is_none(),
        "rollback follows the last accepted write"
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.action(Action::ProjectToggle(project().path), w, cx)
        })
    });
    let next = revision(&requests);
    root.draw();
    assert!(session_bounds(&mut root).is_some());
    ack(&mut root, &shell, first, false);
    assert!(session_bounds(&mut root).is_some());
    ack(&mut root, &shell, next, true);
}

#[test]
fn selection_failure_and_late_snapshot_never_discard_retained_editor() {
    let (_app, mut root, shell, requests) = fixture(64);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            create(s, "a", "draft A", w, cx);
            create(s, "b", "draft B", w, cx);
            let mut selections = 0;
            let mut saves = 0;
            let mut starts = 0;
            while let Ok(command) = requests.try_recv() {
                match command {
                    Command::WorkspaceMutation {
                        revision,
                        mutation:
                            Mutation::Select {
                                project: path,
                                session,
                            },
                    } => {
                        assert_eq!(path, project().path);
                        assert!(session.is_empty());
                        selections += 1;
                        s.ingest(
                            Event::WorkspaceMutation {
                                revision,
                                success: true,
                            },
                            cx,
                        );
                    }
                    Command::Save {
                        revision,
                        preferences: Some(preferences),
                        shutdown: false,
                    } => {
                        assert!(
                            preferences
                                .drafts
                                .iter()
                                .any(|draft| draft.text == "draft A")
                        );
                        saves += 1;
                        s.ingest(
                            Event::Saved {
                                revision,
                                success: true,
                                stopped: vec![],
                            },
                            cx,
                        );
                    }
                    Command::Start { runtime, .. } => {
                        assert!(runtime == "a" || runtime == "b");
                        starts += 1;
                    }
                    _ => panic!("unexpected session creation side effect"),
                }
            }
            assert_eq!(selections, 2);
            assert!(saves > 0);
            assert_eq!(starts, 2);
            let live = s.sessions.get_mut("a").unwrap();
            live.source_id = "saved".into();
            live.source_path = "/saved.jsonl".into();
        })
    });
    let editors = root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let editors = (
                s.sessions["a"].composer.entity_id(),
                s.sessions["b"].composer.entity_id(),
            );
            s.select("a", w, cx);
            assert_eq!(s.active, "a", "retained view switches before disk ack");
            editors
        })
    });
    let selection = revision(&requests);
    root.draw();
    let painted = root.update(|_, w, cx| {
        assert!(
            gpui_kit::base::test_support::find(w, &[], &"shell-composer-area".into()).is_some()
        );
        shell.update(cx, |s, cx| {
            assert_eq!(s.sessions["a"].composer.entity_id(), editors.0);
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
        session_node_bounds(w)
    });
    assert!(
        painted.is_some(),
        "selected saved row paints before acknowledgement"
    );
    ack(&mut root, &shell, selection, false);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(Event::Workspace(Ok(workspace())), cx);
            s.sync(w, cx);
            assert_eq!(s.active, "a");
            assert_eq!(s.sessions["a"].composer.entity_id(), editors.0);
            assert_eq!(s.sessions["b"].composer.entity_id(), editors.1);
            assert_eq!(s.sessions["a"].controller.state().draft, "draft A");
            assert_eq!(s.sessions["b"].controller.state().draft, "draft B");
            assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].selected);
        })
    });
    root.draw();
    assert_eq!(session_bounds(&mut root), painted);
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.select("b", w, cx);
            assert_eq!(s.sessions["b"].composer.entity_id(), editors.1);
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
            assert_eq!(s.sessions["a"].controller.state().draft, "draft A");
        })
    });
    root.draw();
}
