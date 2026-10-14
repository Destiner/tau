use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, point};
use std::{cell::RefCell, rc::Rc};

type Events = Rc<RefCell<Vec<UiEvent>>>;
fn dialog() -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<RemoteDialog>,
    Events,
    Subscription,
) {
    let mut app = super::super::tests::app();
    let mut dialog = None;
    let window = app.open_window(|w, cx| {
        let view = cx.new(|cx| RemoteDialog::new(String::new(), false, w, cx));
        dialog = Some(view.clone());
        Root::new(view, w, cx)
    });
    let dialog = dialog.unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let received = events.clone();
    let subscription = app.update(|cx| {
        cx.subscribe(&dialog, move |_, event: &UiEvent, _| {
            received.borrow_mut().push(event.clone())
        })
    });
    (app, window, dialog, events, subscription)
}
fn rows() -> Vec<OptionRow> {
    vec![
        OptionRow {
            label: "Go Back".into(),
            path: "/".into(),
            select: false,
            back: true,
        },
        OptionRow {
            label: "Select /home".into(),
            path: "/home".into(),
            select: true,
            back: false,
        },
        OptionRow {
            label: "Alpha".into(),
            path: "/home/Alpha".into(),
            select: false,
            back: false,
        },
    ]
}
#[test]
fn root_mouse_selection_tab_containment_escape_and_backdrop() {
    let (_app, mut window, dialog, events, _sub) = dialog();
    window.update(|_, _, cx| {
        dialog.update(cx, |s, _| {
            s.directory = true;
            s.rows = rows();
        })
    });
    window.draw();
    let position = window.update(|_, w, _| w.find("remote-option-2").bounds().center());
    window.simulate_click(position, MouseButton::Left);
    assert!(
        matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.path == "/home/Alpha")
    );
    for key in ["tab", "shift-tab"] {
        window.simulate_keystroke(key);
        window.update(|_, w, cx| {
            assert!(
                dialog
                    .read(cx)
                    .input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            )
        });
    }
    window.simulate_keystroke("ctrl-enter");
    assert!(matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.select));
    window.simulate_keystroke("ctrl-up");
    assert!(matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.path == "/"));
    window.simulate_keystroke("escape");
    assert!(matches!(events.borrow().last(), Some(UiEvent::Cancel)));
    let count = events.borrow().len();
    window.simulate_click(point(px(5.), px(80.)), MouseButton::Left);
    assert_eq!(events.borrow().len(), count + 1);
    assert!(matches!(events.borrow().last(), Some(UiEvent::Cancel)));
}
#[test]
fn root_scoped_actions_never_navigate_submit_or_cancel_marked_text() {
    let (_app, mut window, dialog, events, _sub) = dialog();
    window.update(|_, w, cx| {
        dialog.update(cx, |s, cx| {
            s.directory = true;
            s.rows = rows();
            s.input.update(cx, |i, cx| {
                i.replace_and_mark_text_in_range(Some(0..0), "al", Some(2..2), w, cx)
            });
        })
    });
    window.draw();
    for key in [
        "down",
        "up",
        "cmd-up",
        "ctrl-up",
        "enter",
        "cmd-enter",
        "ctrl-enter",
        "escape",
    ] {
        window.simulate_keystroke(key);
        window.draw();
    }
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| matches!(event, UiEvent::Changed(_)))
    );
    window.update(|_, w, cx| {
        dialog.update(cx, |s, cx| {
            assert_eq!(s.cursor, 0);
            s.input.update(cx, |i, cx| i.unmark_text(w, cx));
        })
    });
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    assert!(matches!(events.borrow().last(), Some(UiEvent::Choose(row)) if row.select));
}
#[test]
fn root_connection_modified_enter_busy_and_dimensions_match_vue() {
    let (_app, mut window, dialog, events, _sub) = dialog();
    window.draw();
    window.update(|_, w, _| {
        let bounds = w.find("remote-dialog").bounds();
        assert!((f32::from(bounds.size.width) - 440.).abs() < 1.);
        assert!((f32::from(bounds.top()) - 68.).abs() < 1.);
    });
    for key in ["enter", "cmd-enter", "ctrl-enter"] {
        window.simulate_keystroke(key);
    }
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|e| matches!(e, UiEvent::Submit))
            .count(),
        3
    );
    window.update(|_, _, cx| {
        dialog.update(cx, |s, cx| {
            s.busy = true;
            s.set_status("Connecting…", Some(1), cx);
        })
    });
    window.draw();
    let count = events.borrow().len();
    for key in ["enter", "cmd-enter", "ctrl-enter", "escape"] {
        window.simulate_keystroke(key);
    }
    window.simulate_click(point(px(5.), px(80.)), MouseButton::Left);
    assert_eq!(events.borrow().len(), count);
    window.update(|_, _, cx| {
        dialog.update(cx, |s, cx| {
            s.busy = false;
            s.set_status("", None, cx);
            s.directory = true;
            s.rows = rows();
        })
    });
    window.draw();
    window.update(|_, w, _| {
        assert!((f32::from(w.find("remote-dialog").bounds().size.width) - 480.).abs() < 1.)
    });
}
#[test]
fn shell_retry_dispatch_failure_and_remount_fence_old_controls() {
    let mut app = super::super::tests::app();
    let (sender, commands) = async_channel::bounded(16);
    let shell = app.new_entity(|_| super::super::tests::shell(sender));
    let mut window = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    let old = window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.remote.connection = "ssh fixture".into();
            s.open_remote(w, cx);
            s.remote.view.clone().unwrap()
        })
    });
    window.update(|_, w, _| w.remove_window());
    let mut window = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.mount_remote(w, cx);
        })
    });
    app.update(|cx| old.update(cx, |_, cx| cx.emit(UiEvent::Changed("ssh stale".into()))));
    let current = app.update(|cx| {
        assert_eq!(shell.read(cx).remote.connection, "ssh fixture");
        shell.read(cx).remote.view.clone().unwrap()
    });
    window.draw();
    window.simulate_keystroke("enter");
    let Command::Remote(first) = commands.try_recv().unwrap() else {
        panic!("probe");
    };
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.sync_remote(w, cx);
            assert!(current.read(cx).input.read(cx).presentation().is_readonly());
            s.remote_failed(first.clone(), cx);
            s.sync_remote(w, cx);
            assert!(!current.read(cx).input.read(cx).presentation().is_readonly());
        })
    });
    window.draw();
    let retry_position = window.update(|_, w, _| w.find("remote-retry").bounds().center());
    window.simulate_click(retry_position, MouseButton::Left);
    let Command::Remote(retry) = commands.try_recv().unwrap() else {
        panic!("retry");
    };
    assert!(retry.revision > first.revision);
    assert_eq!(retry.connection, first.connection);
    assert_eq!(retry.operation, first.operation);
    app.update(|cx| {
        shell.update(cx, |s, cx| {
            s.remote_failed(first, cx);
            assert!(s.remote.is_busy());
            s.remote_failed(retry, cx);
        })
    });
    window.update(|_, w, cx| shell.update(cx, |s, cx| s.sync_remote(w, cx)));
    window.draw();
    window.simulate_keystroke("escape");
    app.update(|cx| assert!(!shell.read(cx).remote.is_open()));
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.open_remote(w, cx);
            assert_eq!(s.remote.connection, "ssh fixture");
            assert!(!s.remote.is_busy());
        })
    });
}
#[test]
fn root_shell_filter_navigation_and_failed_lookup_retain_path_and_draft() {
    let mut app = super::super::tests::app();
    let (sender, commands) = async_channel::bounded(16);
    let shell = app.new_entity(|_| super::super::tests::shell(sender));
    let mut window = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.remote.connection = "ssh fixture".into();
            s.open_remote(w, cx);
            s.remote_request(RemoteOperation::Probe, String::new(), cx);
        })
    });
    let Command::Remote(probe) = commands.try_recv().unwrap() else {
        panic!("probe");
    };
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.remote_event(
                RemoteEvent {
                    command: probe,
                    result: Ok(RemoteResult::Listing(RemoteDirectoryListing {
                        connection_string: "ssh fixture".into(),
                        host: "fixture".into(),
                        working_directory: "/home".into(),
                        directories: vec![tau_core::models::RemoteDirectoryEntry {
                            name: "Alpha".into(),
                            path: "/home/Alpha".into(),
                        }],
                    })),
                },
                cx,
            );
            s.sync_remote(w, cx);
        })
    });
    window.draw();
    window.simulate_input("al");
    app.update(|cx| assert_eq!(shell.read(cx).remote.filter, "al"));
    window.simulate_keystroke("down");
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    let Command::Remote(list) = commands.try_recv().unwrap() else {
        panic!("list");
    };
    assert_eq!(list.operation, RemoteOperation::List);
    assert_eq!(list.path, "/home/Alpha");
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.sync_remote(w, cx);
            assert_eq!(
                s.remote.view.as_ref().unwrap().read(cx).status,
                "Loading directories…"
            );
            s.remote_failed(list, cx);
            s.sync_remote(w, cx);
            assert_eq!(s.remote.filter, "al");
            assert_eq!(
                s.remote.listing.as_ref().unwrap().working_directory,
                "/home"
            );
            assert_eq!(
                s.remote
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .input
                    .read(cx)
                    .value()
                    .as_ref(),
                "al"
            );
        })
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    let Command::Remote(import) = commands.try_recv().unwrap() else {
        panic!("import");
    };
    assert_eq!(import.operation, RemoteOperation::Import);
    assert_eq!(import.path, "/home");
}

#[test]
fn shell_lifecycle_confirmation_cancel_focus_hook_restores_remote_without_losing_draft() {
    use crate::native::{Lifecycle, LifecycleCallbacks};
    let mut app = super::super::tests::app();
    let (sender, _) = async_channel::bounded(16);
    let shell = app.new_entity(|_| super::super::tests::shell(sender));
    let request = Rc::new(RefCell::new(None));
    let received = request.clone();
    let lifecycle = app.update(|cx| {
        Lifecycle::install(
            cx,
            std::time::Duration::from_secs(60),
            LifecycleCallbacks {
                requested: Rc::new(move |value, _| *received.borrow_mut() = Some(value)),
                settled: Rc::new(|_, _, _| {}),
            },
        )
    });
    assert!(!app.simulate_quit());
    let request = request.borrow().unwrap();
    let mut window = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.remote.connection = "ssh retained".into();
            s.open_remote(w, cx);
            let input = s.remote.view.as_ref().unwrap().read(cx).input.clone();
            let higher = cx.focus_handle();
            higher.focus(w, cx);
            s.lifecycle = Some(request);
            s.focus_remote(w, cx);
            assert!(higher.is_focused(w));
            let confirmation = cx.new(|cx| {
                NativeDialog::new(
                    DialogProps {
                        kind: DialogKind::Quit,
                        title: "Close Tau?".into(),
                        description: String::new(),
                        placeholder: String::new(),
                        options: vec![],
                    },
                    false,
                    w,
                    cx,
                )
            });
            s.confirmation = Some(confirmation.clone());
            s.lifecycle = None;
            confirmation.update(cx, |d, cx| d.focus(w, cx));
            s.focus_remote(w, cx);
            assert!(!input.read(cx).focus_handle(cx).is_focused(w));
            s.confirmation = None;
            assert!(lifecycle.resolve(request, false, cx));
            s.focus_remote(w, cx);
            assert!(input.read(cx).focus_handle(cx).is_focused(w));
            assert_eq!(input.read(cx).value().as_ref(), "ssh retained");
        })
    });
    assert_eq!(app.accepted_quits(), 0);
}

#[test]
fn cancel_host_a_reopens_connection_step_and_probes_host_b() {
    super::super::tests::with_shell(16, |s, w, cx, commands| {
        s.remote.connection = "ssh host-a".into();
        s.open_remote(w, cx);
        s.remote_request(RemoteOperation::Probe, String::new(), cx);
        let Command::Remote(probe) = commands.try_recv().unwrap() else {
            panic!("probe")
        };
        s.remote_event(
            RemoteEvent {
                command: probe.clone(),
                result: Ok(RemoteResult::Listing(RemoteDirectoryListing {
                    connection_string: "ssh host-a".into(),
                    host: "host-a".into(),
                    working_directory: "/home/a".into(),
                    directories: vec![],
                })),
            },
            cx,
        );
        s.remote.filter = "old filter".into();
        s.remote.error = "old error".into();
        s.remote.retry = Some(probe.clone());
        s.remote.cancel();
        assert!(!s.remote.is_open());
        s.open_remote(w, cx);
        assert!(s.remote.listing.is_none());
        assert!(s.remote.filter.is_empty());
        assert!(s.remote.error.is_empty());
        assert!(s.remote.retry.is_none());
        assert_eq!(s.remote.cursor, 0);
        assert_eq!(s.remote.connection, "ssh host-a");
        assert!(!s.remote.view.as_ref().unwrap().read(cx).directory);
        s.remote.connection = "ssh host-b".into();
        s.remote_request(RemoteOperation::Probe, String::new(), cx);
        let Command::Remote(next) = commands.try_recv().unwrap() else {
            panic!("new probe")
        };
        assert_eq!(next.connection, "ssh host-b");
        assert!(next.host.is_empty());
        assert!(next.revision > probe.revision);
        s.remote_failed(probe, cx);
        assert_eq!(s.remote.pending.as_ref(), Some(&next));
        s.remote.cancel();
        assert!(s.remote.is_open());
        assert_eq!(s.remote.pending.as_ref(), Some(&next));
        // Even a forced closed surface cannot reopen and clear an existing job.
        s.remote.open = false;
        s.open_remote(w, cx);
        assert!(!s.remote.is_open());
        assert_eq!(s.remote.pending.as_ref(), Some(&next));
        assert!(commands.try_recv().is_err());
    });
}

#[test]
fn imported_project_merges_without_replacing_workspace_session_or_draft() {
    super::super::tests::with_shell(16, |s, w, cx, commands| {
        let existing = super::super::tests::project();
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![existing.clone()],
            active_project_path: existing.path.clone(),
            pi_path: Some("retained-pi".into()),
        });
        s.create_retained(
            "retained".into(),
            existing.clone(),
            String::new(),
            String::new(),
            "draft text".into(),
            w,
            cx,
        );
        s.active = "retained".into();
        assert!(commands.try_recv().is_err());
        s.remote.connection = "ssh fixture".into();
        s.open_remote(w, cx);
        s.remote_request(RemoteOperation::Import, "/remote".into(), cx);
        let Command::Remote(command) = commands.try_recv().unwrap() else {
            panic!("import")
        };
        let mut imported = existing.clone();
        imported.path = "ssh:fixture".into();
        imported.selected = false;
        s.remote_event(
            RemoteEvent {
                command: RemoteCommand {
                    revision: command.revision - 1,
                    ..command.clone()
                },
                result: Ok(RemoteResult::Imported(imported.clone())),
            },
            cx,
        );
        assert_eq!(s.workspace.as_ref().unwrap().projects.len(), 1);
        assert!(s.remote.is_busy());
        s.remote_event(
            RemoteEvent {
                command,
                result: Ok(RemoteResult::Imported(imported.clone())),
            },
            cx,
        );
        let workspace = s.workspace.as_ref().unwrap();
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.active_project_path, existing.path);
        assert_eq!(workspace.pi_path.as_deref(), Some("retained-pi"));
        assert_eq!(s.active, "retained");
        assert_eq!(s.sessions["retained"].shown_draft, "draft text");
        assert_eq!(s.sessions["retained"].project.path, existing.path);
        assert!(commands.try_recv().is_err(), "import must not start Pi");
        imported.selected = true;
        s.merge_imported_project(imported.clone());
        let workspace = s.workspace.as_ref().unwrap();
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.active_project_path, imported.path);
        assert!(!workspace.projects[0].selected);
        assert!(workspace.projects[1].selected);
        assert_eq!(s.active, "retained");
        assert_eq!(s.sessions["retained"].shown_draft, "draft text");
        assert!(commands.try_recv().is_err());
    });
}

#[test]
fn shell_focus_remote_only_when_no_higher_modal_owns_focus() {
    super::super::tests::with_shell(16, |s, w, cx, _| {
        s.open_remote(w, cx);
        let input = s.remote.view.as_ref().unwrap().read(cx).input.clone();
        let higher = cx.focus_handle();
        higher.focus(w, cx);
        s.error = Some("fixture".into());
        s.focus_remote(w, cx);
        assert!(higher.is_focused(w));
        s.error = None;
        s.focus_remote(w, cx);
        assert!(input.read(cx).focus_handle(cx).is_focused(w));
    });
}

#[test]
fn progress_delay_fast_clear_slow_superseded_and_dropped_view() {
    use std::time::Duration;
    let (app, mut window, view, _events, _sub) = dialog();
    let set = |window: &mut TestAppWindow<Root>, status, revision| {
        window.update(|_, _, cx| view.update(cx, |s, cx| s.set_status(status, revision, cx)));
    };
    let painted = |window: &mut TestAppWindow<Root>| {
        window.draw();
        window.update(|_, w, _| {
            w.try_find("remote-status")
                .is_some_and(|status| status.visible())
        })
    };
    set(&mut window, "Connecting…", Some(1));
    app.run_until_parked();
    assert!(!painted(&mut window));
    app.advance_clock(Duration::from_millis(100));
    set(&mut window, "", None);
    app.advance_clock(Duration::from_millis(200));
    app.run_until_parked();
    assert!(!painted(&mut window));

    set(&mut window, "Connecting…", Some(2));
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(100));
    set(&mut window, "Connecting…", Some(3));
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(100));
    app.run_until_parked();
    assert!(!painted(&mut window));
    app.advance_clock(Duration::from_millis(80));
    app.run_until_parked();
    assert!(painted(&mut window));
    set(&mut window, "", None);
    assert!(!painted(&mut window));

    set(&mut window, "Importing project…", Some(4));
    app.run_until_parked();
    set(&mut window, "", None);
    app.advance_clock(Duration::from_millis(200));
    app.run_until_parked();
    assert!(!painted(&mut window));

    let weak = window.update(|_, w, cx| {
        let abandoned = cx.new(|cx| RemoteDialog::new(String::new(), false, w, cx));
        abandoned.update(cx, |s, cx| s.set_status("Connecting…", Some(5), cx));
        abandoned.downgrade()
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(200));
    app.run_until_parked();
    assert!(weak.upgrade().is_none());
}
