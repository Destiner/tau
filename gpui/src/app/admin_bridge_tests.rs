use super::super::tests::with_shell;
use super::*;

fn enable(s: &mut Shell, cx: &mut Context<Shell>) {
    let admin::Command::SetMode { revision, .. } = s.admin.set_mode(true) else {
        unreachable!()
    };
    s.admin_event(
        admin::Reply::ModeSet {
            revision,
            enabled: true,
            saved: true,
        },
        cx,
    );
}

#[test]
fn production_shell_mounts_shared_load_and_reconciles_actual_failed_mode() {
    with_shell(16, |s, w, cx, commands| {
        s.window = Some(w.window_handle());
        s.mount_admin(w, cx);
        let Command::Admin(admin::Command::Load { revision }) = commands.try_recv().unwrap() else {
            panic!("expected admin load")
        };
        s.admin_event(
            admin::Reply::Loaded {
                revision,
                enabled: false,
            },
            cx,
        );
        let admin::Command::SetMode { revision, .. } = s.admin.set_mode(true) else {
            unreachable!()
        };
        // Failed disk persistence still enabled the real backend's live switch.
        s.admin_event(
            admin::Reply::ModeSet {
                revision,
                enabled: true,
                saved: false,
            },
            cx,
        );
        assert!(s.admin.enabled());
        assert!(s.admin.mode_failed());
        assert_eq!(s.error.as_deref(), Some(admin::MODE_ERROR));
        s.sync_admin(w, cx);
        assert!(s.admin.view.as_ref().unwrap().read(cx).enabled());
        s.open_admin_reporter(w, cx);
        assert!(
            !s.admin.view.as_ref().unwrap().read(cx).is_open(),
            "error modal owns focus"
        );
    });
}

#[test]
fn production_shell_closed_enqueue_and_stale_submission_settle_without_window() {
    with_shell(16, |s, w, cx, commands| {
        s.mount_admin(w, cx);
        commands.try_recv().unwrap();
        enable(s, cx);
        let admin::Command::Submit {
            revision,
            session_id,
            ..
        } = s
            .admin
            .submit("report".into(), true, Some(String::new()))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(session_id, None);
        let command = s.admin.set_mode(false);
        commands.close();
        s.dispatch_admin(command, cx);
        assert!(
            s.admin.enabled(),
            "failed enqueue must restore the actual acknowledged gate"
        );
        assert!(s.admin.mode_failed());
        s.admin_event(
            admin::Reply::Submitted {
                revision,
                saved: true,
            },
            cx,
        );
        assert!(!s.admin.pending(revision));
        s.sync_admin(w, cx);
        assert!(s.admin.submit("retry".into(), false, None).is_some());
    });
}

#[test]
fn production_report_attachment_uses_current_controller_identity_only_with_opt_in() {
    with_shell(32, |s, w, cx, _| {
        super::super::tests::create(s, "runtime-key", "composer draft", w, cx);
        s.active = "runtime-key".into();
        s.sessions
            .get_mut("runtime-key")
            .unwrap()
            .controller
            .bind_source("actual-pi-id", "/not-an-attachment");
        enable(s, cx);
        let id = s.admin_session_id();
        let admin::Command::Submit {
            revision,
            session_id,
            ..
        } = s.admin.submit("report".into(), false, id).unwrap()
        else {
            unreachable!()
        };
        assert!(session_id.is_none());
        s.admin_event(
            admin::Reply::Submitted {
                revision,
                saved: false,
            },
            cx,
        );
        s.sync_admin(w, cx);
        s.sessions
            .get_mut("runtime-key")
            .unwrap()
            .controller
            .bind_source("new-pi-id", "/also-not-an-attachment");
        let id = s.admin_session_id();
        let admin::Command::Submit { session_id, .. } =
            s.admin.submit("report".into(), true, id).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(session_id.as_deref(), Some("new-pi-id"));
    });
}

#[test]
fn production_shell_palette_priority_suspends_and_retains_report_draft() {
    with_shell(16, |s, w, cx, commands| {
        s.mount_palette(w, cx);
        s.mount_admin(w, cx);
        commands.try_recv().unwrap();
        enable(s, cx);
        s.open_admin_reporter(w, cx);
        let view = s.admin.view.clone().unwrap();
        let editor = view.read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("retained report", w, cx));
        s.show_palette_page("root", w, cx);
        assert!(s.palette.is_open(cx));
        assert!(view.read(cx).is_open());
        let focus = w.focused(cx);
        s.open_admin_reporter(w, cx);
        assert_eq!(w.focused(cx), focus);
        assert_eq!(editor.read(cx).value().as_ref(), "retained report");
    });
}

#[test]
fn production_full_disable_queue_preserves_accepted_report_until_reply() {
    with_shell(1, |s, w, cx, commands| {
        s.mount_admin(w, cx);
        commands.try_recv().unwrap();
        enable(s, cx);
        let command = s
            .admin
            .submit("accepted report".into(), false, None)
            .unwrap();
        let admin::Command::Submit { revision, .. } = &command else {
            unreachable!();
        };
        let revision = *revision;
        s.dispatch_admin(command, cx);
        let disable = s.admin.set_mode(false);
        s.dispatch_admin(disable, cx);
        assert!(s.admin.enabled());
        assert!(s.admin.mode_failed());
        assert!(s.admin.pending(revision));
        assert!(s.admin.submit("unsafe retry".into(), false, None).is_none());
        assert!(
            matches!(commands.try_recv().unwrap(), Command::Admin(admin::Command::Submit { revision: queued, .. }) if queued == revision)
        );
        s.admin_event(
            admin::Reply::Submitted {
                revision,
                saved: true,
            },
            cx,
        );
        s.sync_admin(w, cx);
        assert!(!s.admin.pending(revision));
    });
}

#[test]
fn production_busy_component_full_disable_enqueue_consumes_saved_reply() {
    let mut app = super::super::tests::app();
    let (sender, commands) = async_channel::bounded(1);
    let shell = app.new_entity(|_| super::super::tests::shell(sender));
    let mut window = app.open_window(|w, cx| {
        let view = shell.update(cx, |s, cx| {
            s.mount_admin(w, cx);
            commands.try_recv().unwrap();
            enable(s, cx);
            s.sync_admin(w, cx);
            s.open_admin_reporter(w, cx);
            let view = s.admin.view.clone().unwrap();
            let editor = view.read(cx).editor();
            editor.update(cx, |editor, cx| editor.set_value("accepted draft", w, cx));
            view
        });
        gpui_kit::base::Root::new(view, w, cx)
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let view = s.admin.view.clone().unwrap();
            assert!(view.read(cx).is_busy(), "actual component submitted");
            // The accepted report occupies the sole queue slot. Disable is
            // rejected synchronously, before the view ever observes off.
            let disable = s.admin.set_mode(false);
            s.dispatch_admin(disable, cx);
            s.sync_admin(w, cx);
            assert!(s.admin.enabled());
            assert!(s.admin.mode_failed());
            assert!(view.read(cx).is_busy());
            let Command::Admin(admin::Command::Submit { revision, .. }) =
                commands.try_recv().unwrap()
            else {
                panic!("expected the accepted report, not disable");
            };
            assert!(s.admin.pending(revision));
            s.admin_event(
                admin::Reply::Submitted {
                    revision,
                    saved: true,
                },
                cx,
            );
            s.sync_admin(w, cx);
            assert!(!view.read(cx).is_busy());
            assert_eq!(view.read(cx).editor().read(cx).value().as_ref(), "");
        });
    });
}
