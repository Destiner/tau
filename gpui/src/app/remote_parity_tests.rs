use super::*;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, base::Root, test::TestWindowExt};
use std::{cell::RefCell, rc::Rc, time::Duration};

fn fixture() -> (TestApp, TestAppWindow<Root>, Entity<RemoteDialog>) {
    let mut app = super::super::tests::app();
    let mut view = None;
    let window = app.open_window(|w, cx| {
        let dialog = cx.new(|cx| RemoteDialog::new("ssh fixture".into(), false, w, cx));
        dialog.update(cx, |s, cx| {
            s.directory = true;
            s.rows = vec![
                OptionRow {
                    label: "Go Back".into(),
                    path: "/home".into(),
                    select: false,
                    back: true,
                },
                OptionRow {
                    label: "Select /home/me".into(),
                    path: "/home/me".into(),
                    select: true,
                    back: false,
                },
                OptionRow {
                    label: "Alpha".into(),
                    path: "/home/me/very long directory/Alpha".into(),
                    select: false,
                    back: false,
                },
            ];
            cx.notify();
        });
        view = Some(dialog.clone());
        Root::new(dialog, w, cx)
    });
    (app, window, view.unwrap())
}

#[test]
fn directory_navigation_uses_semantics_not_display_labels() {
    let (_, mut window, view) = fixture();
    let received = Rc::new(RefCell::new(vec![]));
    let events = received.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&view, move |_, _, e: &UiEvent, _| {
            events.borrow_mut().push(e.clone())
        })
    });
    window.update(|_, _, cx| {
        view.update(cx, |s, cx| {
            s.rows[0].label = "Localized back label".into();
            s.rows[2].label = "Go Back".into();
            cx.notify();
        });
    });
    window.draw();
    window.simulate_keystroke("cmd-up");
    assert!(
        matches!(received.borrow().last(), Some(UiEvent::Choose(row))
        if row.back && row.path == "/home")
    );
    window.simulate_keystroke("cmd-enter");
    assert!(
        matches!(received.borrow().last(), Some(UiEvent::Choose(row))
        if row.select && row.path == "/home/me")
    );
}

#[test]
fn root_pointer_highlight_full_path_hint_click_and_busy_guard() {
    let (app, mut window, view) = fixture();
    let received = Rc::new(RefCell::new(vec![]));
    let events = received.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&view, move |_, _, e: &UiEvent, _| {
            events.borrow_mut().push(e.clone())
        })
    });
    window.draw();
    let point = window.update(|_, w, _| w.find("remote-option-2").bounds().center());
    window.simulate_mouse_move(point);
    window.draw();
    window.update(|_, w, cx| {
        assert_eq!(view.read(cx).cursor, 2);
        assert!(w.find("remote-cursor-rail-2").visible());
        assert!(w.try_find("remote-cursor-rail-0").is_none());
        assert!(view.read(cx).input.read(cx).focus_handle(cx).is_focused(w));
    });
    app.advance_clock(Duration::from_millis(1000));
    app.run_until_parked();
    window.draw();
    window.update(|_, w, _| assert!(w.find("remote-path-tooltip").visible()));
    window.simulate_click(point, MouseButton::Left);
    assert!(
        matches!(received.borrow().last(), Some(UiEvent::Choose(row)) if row.path == "/home/me/very long directory/Alpha")
    );
    window.update(|_, _, cx| {
        view.update(cx, |s, cx| {
            s.busy = true;
            cx.notify();
        })
    });
    window.draw();
    let count = received.borrow().len();
    let parent = window.update(|_, w, _| w.find("remote-option-0").bounds().center());
    window.simulate_mouse_move(parent);
    window.simulate_click(parent, MouseButton::Left);
    window.simulate_keystroke("cmd-up");
    window.simulate_keystroke("cmd-enter");
    window.simulate_keystroke("escape");
    assert_eq!(received.borrow().len(), count);
    assert_eq!(window.update(|_, _, cx| view.read(cx).cursor), 2);
}

#[test]
fn root_error_retry_and_focus_keyboard_do_not_choose_on_hover() {
    let (_, mut window, view) = fixture();
    let received = Rc::new(RefCell::new(vec![]));
    let events = received.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&view, move |_, _, e: &UiEvent, _| {
            events.borrow_mut().push(e.clone())
        })
    });
    window.update(|_, _, cx| {
        view.update(cx, |s, cx| {
            s.error = "Could not read the remote directory. Try again.".into();
            cx.notify();
        })
    });
    window.draw();
    let field = window.update(|_, w, _| w.find("remote-field").bounds().center());
    window.simulate_mouse_move(field);
    assert!(received.borrow().is_empty());
    let retry = window.update(|_, w, _| w.find("remote-retry").bounds().center());
    window.simulate_click(retry, MouseButton::Left);
    assert!(matches!(received.borrow().last(), Some(UiEvent::Retry)));
    window.update(|_, w, cx| view.read(cx).input.read(cx).focus_handle(cx).focus(w, cx));
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    assert!(matches!(received.borrow().last(), Some(UiEvent::Choose(row)) if row.select));
}
