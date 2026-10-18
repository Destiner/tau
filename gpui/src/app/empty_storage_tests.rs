use super::*;
use crate::app::tests::{app, create, project, shell};
use gpui_kit::{MouseButton, size, test::TestWindowExt};

fn press(root: &mut gpui_kit::TestAppWindow<Root>, key: &str) {
    let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
    root.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    root.simulate_event(gpui_kit::KeyUpEvent { keystroke });
}

#[test]
fn successful_empty_cleanup_never_paints_retry_save_and_failure_is_retryable() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(700.)));
    root.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
        shell.update(cx, |s, cx| {
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()],
                active_project_path: project().path,
                pi_path: None,
            });
            s.window = Some(w.window_handle());
            s.mount(w, cx);
            create(s, "target", "retained draft", w, cx);
            create(s, "empty", "", w, cx);
            s.select("target", w, cx);
            assert!(!s.sessions.contains_key("empty"));
            assert_eq!(s.empty_disposals.len(), 1);
            assert!(s.register_retries.is_empty());
            assert!(s.is_busy(), "cleanup acknowledgement still gates shutdown");
        });
    });
    root.draw();
    let before = root.update(|_, w, _| w.find("shell-composer-area").bounds());
    for _ in 0..3 {
        root.draw();
        root.update(|_, w, _| {
            assert!(gpui_kit::base::test_support::find(w, &[], &"retry-storage".into()).is_none());
            assert_eq!(w.find("shell-composer-area").bounds(), before);
        });
    }
    let disposals = std::iter::from_fn(|| requests.try_recv().ok())
        .filter_map(|c| match c {
            Command::DisposeEmpty {
                runtime,
                attempt,
                generation,
            } => Some((runtime, attempt, generation)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(disposals.len(), 1);
    let (runtime, attempt, generation) = disposals[0].clone();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(
                Event::EmptyDisposed {
                    runtime: runtime.clone(),
                    attempt,
                    generation,
                    success: false,
                },
                cx,
            );
            s.sync(w, cx);
            assert_eq!(s.register_retries.len(), 1);
        })
    });
    root.draw();
    // Dismiss the incident before retrying through the existing command palette.
    let close = root.update(|_, w, _| w.find("close").bounds().center());
    root.simulate_click(close, MouseButton::Left);
    for _ in 0..2 {
        root.update(|_, w, cx| w.simulate_next_frame(cx));
        root.draw();
    }
    root.update(|_, _, cx| assert_eq!(shell.read(cx).register_retries.len(), 1));
    press(&mut root, "cmd-k");
    root.draw();
    root.simulate_input("Retry Pending Operations");
    root.draw();
    press(&mut root, "enter");
    root.draw();
    root.update(|_, _, cx| {
        assert!(shell.read(cx).register_retries.is_empty());
        assert_eq!(shell.read(cx).empty_disposals.len(), 1);
    });
    assert_eq!(
        std::iter::from_fn(|| requests.try_recv().ok())
            .filter(|c| matches!(c, Command::DisposeEmpty { .. }))
            .count(),
        1
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(
                Event::EmptyDisposed {
                    runtime,
                    attempt,
                    generation,
                    success: true,
                },
                cx,
            );
            s.sync(w, cx);
            assert!(s.error.is_none());
            assert!(s.empty_disposals.is_empty());
            assert_eq!(
                s.sessions["target"].controller.state().draft,
                "retained draft"
            );
        })
    });
    root.draw();
    root.update(|_, w, _| {
        assert!(gpui_kit::base::test_support::find(w, &[], &"retry-storage".into()).is_none());
        assert_eq!(w.find("shell-composer-area").bounds(), before);
    });
}
