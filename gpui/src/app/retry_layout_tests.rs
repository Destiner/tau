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
fn pending_retry_has_no_footer_and_palette_dispatches_owned_cleanup_once() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(700.)));
    root.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()],
                active_project_path: project().path,
                pi_path: None,
            });
            s.mount(w, cx);
            create(s, "target", "retained draft", w, cx);
            create(s, "empty", "", w, cx);
            s.select("target", w, cx);
        });
    });
    root.draw();
    let before = root.update(|_, w, _| w.find("shell-composer-area").bounds());
    let disposals = std::iter::from_fn(|| requests.try_recv().ok())
        .filter_map(|command| match command {
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
            assert!(s.workflow_failure(&s.active).is_some());
            assert!(s.retry_workflow_pending());
            assert_eq!(s.register_retries.len(), 1);
            assert_eq!(s.empty_disposals.len(), 1);
        });
    });
    for _ in 0..3 {
        root.draw();
        root.update(|_, w, cx| {
            assert!(w.try_find("retry-storage").is_none());
            assert_eq!(w.find("shell-composer-area").bounds(), before);
            assert_eq!(before.bottom(), w.viewport_size().height);
            let s = shell.read(cx);
            assert_eq!(s.register_retries.len(), 1);
            assert_eq!(s.empty_disposals.len(), 1);
            assert!(s.workflow_failure(&s.active).is_some());
        });
    }
    assert!(requests.try_recv().is_err(), "render must not retry work");
    let close = root.update(|_, w, _| w.find("close").bounds().center());
    root.simulate_click(close, MouseButton::Left);
    for _ in 0..2 {
        root.update(|_, w, cx| w.simulate_next_frame(cx));
        root.draw();
    }
    root.update(|_, w, cx| {
        let s = shell.read(cx);
        assert!(w.focused(cx).is_some(), "shell focus restored");
        assert!(
            s.lifecycle.is_none()
                && s.confirmation.is_none()
                && !s.fallback_is_open(cx)
                && !s.preview_open()
                && !s.picker_busy
                && !s.remote.is_open(),
            "no modal blocks palette"
        );
        assert!(
            s.error.is_none(),
            "unowned error remains clear: {:?}",
            s.error
        );
        assert!(
            s.workflow_failure(&s.active).is_none(),
            "incident acknowledged"
        );
        assert!(s.loaded);
        assert!(s.palette.view.is_some());
    });
    press(&mut root, "cmd-k");
    root.draw();
    root.read(|_, cx| assert!(shell.read(cx).palette.is_open(cx), "Cmd-K opens palette"));
    root.simulate_input("Retry Pending Operations");
    root.draw();
    press(&mut root, "enter");
    root.draw();
    root.update(|_, _, cx| {
        let s = shell.read(cx);
        assert!(s.register_retries.is_empty());
        assert_eq!(s.empty_disposals.len(), 1);
        assert_eq!(
            s.sessions["target"].controller.state().draft,
            "retained draft"
        );
    });
    let retries = std::iter::from_fn(|| requests.try_recv().ok())
        .filter_map(|command| match command {
            Command::DisposeEmpty {
                runtime,
                attempt,
                generation,
            } => Some((runtime, attempt, generation)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        retries, disposals,
        "retry preserves the cleanup snapshot owner"
    );
}
