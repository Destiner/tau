use super::super::tests::{app, shell};
use super::*;
use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent};
#[test]
fn shell_receives_actual_header_click_enter_and_escape() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(128);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            super::tests::fixture(s, w, cx);
            s.restored = true;
            s.mount(w, cx);
            s.sync(w, cx);
        })
    });
    root.draw();
    let position = root.update(|_, w, _| {
        gpui_kit::base::test_support::find(w, &[], &"session-name".to_owned().into())
            .unwrap()
            .bounds()
            .center()
    });
    for commit in [false, true] {
        root.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Default::default(),
            click_count: 1,
            first_mouse: false,
        });
        root.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Default::default(),
            click_count: 1,
        });
        root.draw();
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                let h = s.header.as_ref().unwrap();
                assert!(h.read(cx).renaming());
                h.read(cx)
                    .editor()
                    .update(cx, |input, cx| input.set_value("Edited", w, cx));
            })
        });
        root.simulate_keystroke(if commit { "enter" } else { "escape" });
        root.draw();
        root.update(|_, _, cx| {
            shell.update(cx, |s, cx| {
                assert!(!s.header.as_ref().unwrap().read(cx).renaming());
                assert_eq!(
                    s.sessions["live"].controller.state().identity.name,
                    if commit { "Edited" } else { "Original" }
                );
                assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
            })
        });
        let sends: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok())
            .filter(
                |c| matches!(c,Command::Send {request,..} if request["type"]=="set_session_name"),
            )
            .collect();
        assert_eq!(sends.len(), usize::from(commit));
    }
}
