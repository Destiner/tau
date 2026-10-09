use super::super::tests::{app, shell};
use super::*;
use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent};
#[test]
fn mounted_idle_shell_archives_with_models_and_commands_reads_pending() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(128);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            super::tests::fixture_with_pending_reads(s, w, cx, true);
            assert!(!s.sessions["live"].controller.state().working);
            assert_eq!(s.sessions["live"].controller.pending_count(), 2);
            assert!(s.can_archive("live"));
            s.restored = true;
            s.mount(w, cx);
            s.sync(w, cx);
        })
    });
    root.draw();
    let position = root.update(|_, w, _| {
        gpui_kit::base::test_support::find(w, &[], &"archive-live".to_owned().into())
            .unwrap()
            .bounds()
            .center()
    });
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
    root.update(|_, _, cx| {
        shell.update(cx, |s, _| {
            assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
            assert_eq!(s.workspace.as_ref().unwrap().projects[0].sessions.len(), 1);
            assert_ne!(s.active, "live");
            assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
        })
    });
    assert!(
        std::iter::from_fn(|| requests.try_recv().ok())
            .any(|c| matches!(c,Command::Archive {id,archived:true,..} if id=="saved"))
    );
}
