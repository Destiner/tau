use super::*;

fn production() -> Fixture {
    let f = Fixture::new();
    f.host.lifecycle.set_quit_on_last_window_close(true);
    f
}

fn close_second(f: &mut Fixture) {
    assert!(!f.second.simulate_close());
    let request = f.latest();
    assert!(matches!(request.intent(), Intent::Close(_)));
    assert!(f.resolve(request, true));
    assert_eq!(f.cleanup.get(), 0);
    assert_eq!(f.app.accepted_quits(), 0);
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
}

#[test]
fn last_red_close_cmd_w_and_file_close_share_quit_gate_and_keep_focus_on_cancel() {
    for route in 0..3 {
        let mut f = production();
        close_second(&mut f);
        let root = f.first.handle();
        let editor = f.editors.borrow()[0].clone();
        f.first.simulate_activation(true);
        match route {
            0 => assert!(!f.first.simulate_close()),
            1 => f.first.simulate_keystroke("cmd-w"),
            _ => f.dispatch(&CloseWindow),
        }
        let request = f.latest();
        assert_eq!(request.intent(), Intent::Quit);
        assert!(!f.app.simulate_quit());
        assert_eq!(
            f.requests.borrow().len(),
            2,
            "CmdQ coalesces with last close"
        );
        assert!(!f.host.lifecycle.can_open_window());
        // Changes made during asynchronous persistence are owned by the same root.
        f.first.update(|_, w, cx| {
            editor.update(cx, |input, cx| {
                input.set_value("Changed while saving café 😀", w, cx);
            })
        });
        assert!(f.resolve(request, false));
        assert_eq!(f.first.handle(), root);
        f.first.update(|_, w, cx| {
            assert_eq!(
                editor.read(cx).value().as_ref(),
                "Changed while saving café 😀"
            );
            assert!(editor.read(cx).focus_handle(cx).is_focused(w));
        });
        f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
        assert_eq!(f.cleanup.get(), 0);
        assert_eq!(f.app.accepted_quits(), 0);
    }
}

#[test]
fn failed_save_or_timeout_preserves_changed_draft_and_root_without_shutdown() {
    for fail_save in [true, false] {
        let mut f = production();
        close_second(&mut f);
        assert!(!f.first.simulate_close());
        let request = f.latest();
        let root = f.first.handle();
        let editor = f.editors.borrow()[0].clone();
        f.first.update(|_, w, cx| {
            editor.update(cx, |input, cx| {
                input.set_value("New draft during exit", w, cx);
            })
        });
        f.dispatch(&NewWindow);
        f.dispatch(&NewSession);
        assert_eq!(f.created.get(), 0);
        assert_eq!(f.sessions.get(), 0);
        if fail_save {
            // Persistence callback must withhold stop/approval after a failed write.
            assert!(f.resolve(request, false));
        } else {
            f.app.advance_clock(Duration::from_secs(60));
            assert!(!f.resolve(request, true));
            assert!(f.settled.borrow().contains(&(request, Outcome::Expired)));
        }
        assert_eq!(f.first.handle(), root);
        f.first.update(|_, w, cx| {
            assert_eq!(editor.read(cx).value().as_ref(), "New draft during exit");
            assert!(editor.read(cx).focus_handle(cx).is_focused(w));
        });
        assert_eq!(f.cleanup.get(), 0);
        assert_eq!(f.app.accepted_quits(), 0);
        assert!(f.host.lifecycle.can_open_window());
    }
}

#[test]
fn racing_pending_closes_promote_remaining_owner_without_renewing_deadline() {
    let mut f = production();
    assert!(!f.first.simulate_close());
    let first = f.latest();
    assert!(!f.second.simulate_close());
    let second = f.latest();
    f.app.advance_clock(Duration::from_secs(40));
    assert!(f.resolve(second, true));
    assert!(
        !f.resolve(first, true),
        "Close approval cannot authorize Quit"
    );
    let quit = f.latest();
    assert_eq!(quit.intent(), Intent::Quit);
    assert!(f.settled.borrow().contains(&(first, Outcome::Superseded)));
    assert!(!f.resolve(first, true));
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
    f.app.advance_clock(Duration::from_secs(20));
    assert!(!f.host.lifecycle.is_pending(quit));
    assert!(!f.resolve(quit, true));
    assert!(f.settled.borrow().contains(&(quit, Outcome::Expired)));
    assert_eq!(f.cleanup.get(), 0);
    assert_eq!(f.app.accepted_quits(), 0);
    f.first.update(|_, w, cx| {
        assert!(
            f.editors.borrow()[0]
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
    f.drafts();
}

#[test]
fn two_approvals_in_one_turn_recheck_before_removing_last_root() {
    let mut f = production();
    let remaining_root = f.second.handle();
    let remaining_editor = f.editors.borrow()[1].clone();
    f.second
        .update(|_, w, cx| remaining_editor.read(cx).focus_handle(cx).focus(w, cx));
    f.second.simulate_activation(true);
    assert!(!f.first.simulate_close());
    let first = f.latest();
    assert!(!f.second.simulate_close());
    let second = f.latest();
    f.app.update(|cx| {
        assert!(f.host.lifecycle.resolve(first, true, cx));
        assert!(f.host.lifecycle.resolve(second, true, cx));
    });
    let quit = f.latest();
    assert_eq!(quit.intent(), Intent::Quit);
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
    assert_eq!(f.cleanup.get(), 0);
    assert!(f.resolve(quit, false));
    assert_eq!(f.second.handle(), remaining_root);
    f.second
        .update(|_, w, cx| assert!(remaining_editor.read(cx).focus_handle(cx).is_focused(w)));
    f.drafts();
    assert_eq!(f.app.accepted_quits(), 0);
}

#[test]
fn persistence_and_runtime_acknowledgement_precede_last_close_approval() {
    let mut f = production();
    close_second(&mut f);
    assert!(!f.first.simulate_close());
    let request = f.latest();
    assert_eq!(request.intent(), Intent::Quit);
    let saved = Rc::new(RefCell::new(Vec::new()));
    let phases = Rc::new(RefCell::new(Vec::new()));
    let (ack, receive) = async_channel::bounded(1);
    f.app.update(|cx| {
        let host = f.host.lifecycle.clone();
        let editors = f.editors.clone();
        let saved = saved.clone();
        let phases = phases.clone();
        cx.spawn(async move |cx| {
            cx.update(|cx| {
                assert!(host.is_pending(request));
                *saved.borrow_mut() = editors
                    .borrow()
                    .iter()
                    .map(|editor| editor.read(cx).value().to_string())
                    .collect();
                phases.borrow_mut().push("all drafts saved");
                phases.borrow_mut().push("runtime stop requested");
            });
            receive.recv().await.unwrap();
            cx.update(|cx| {
                assert!(host.is_pending(request));
                assert_eq!(cx.windows().len(), 1, "root survives until shutdown ack");
                phases.borrow_mut().push("runtime shutdown acknowledged");
                assert!(host.resolve(request, true, cx));
            });
        })
        .detach();
    });
    f.app.run_until_parked();
    assert_eq!(*saved.borrow(), [DRAFT, DRAFT]);
    assert_eq!(f.cleanup.get(), 0);
    assert_eq!(f.app.accepted_quits(), 0);
    ack.try_send(()).unwrap();
    f.app.run_until_parked();
    assert_eq!(
        *phases.borrow(),
        [
            "all drafts saved",
            "runtime stop requested",
            "runtime shutdown acknowledged"
        ]
    );
    assert_eq!(f.cleanup.get(), 1);
    assert_eq!(f.app.accepted_quits(), 1);
}
