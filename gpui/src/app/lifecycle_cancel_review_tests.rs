use super::*;

#[test]
fn root_quit_cancel_during_storage_drain_retains_context_and_late_ack_cannot_stop_or_approve() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        let requested = shell.downgrade();
        let settled = shell.downgrade();
        let host = NativeHost::install(cx, "Tau", Duration::from_secs(60), HostCallbacks {
            lifecycle: LifecycleCallbacks {
                requested: Rc::new(move |request, cx| { let _ = requested.update(cx, |s, _| s.lifecycle = Some(request)); }),
                settled: Rc::new(move |request, _, cx| { let _ = settled.update(cx, |s, _| {
                    if s.lifecycle == Some(request) {
                        s.lifecycle = None;
                        s.quit_error = None;
                        s.exit_revision = None;
                        s.runtime_lifecycle.exit_wait = None;
                        s.runtime_lifecycle.exit_saved = None;
                        s.runtime_lifecycle.exit_fence_pending = None;
                    }
                }); }),
            },
            new_session: Rc::new(|_| {}), create_window: Rc::new(|_| Err(anyhow::anyhow!("fixture"))), reveal_window: Rc::new(|_, _| {}), window_failed: Rc::new(|_| {}),
        });
        host.lifecycle().attach(w, cx);
        shell.update(cx, |s, cx| {
            create(s, "a", "keep", w, cx);
            started(s, "a", 7, cx);
            reads(s, "a", "a", "/a.jsonl", cx, &requests);
            s.ingest(rpc("a", 7, json!({"type":"message_end", "message":{"role":"assistant", "content":[{"type":"text", "text":"finished"}], "stopReason":"stop"}})), cx);
            while requests.try_recv().is_ok() {}
        });
        cx.quit();
    });
    let request = root.update(|_, _, cx| {
        shell.update(cx, |s, cx| {
            let request = s.lifecycle.unwrap();
            s.begin_exit(request, true, cx);
            assert!(s.exit_revision.is_none());
            assert!(s.runtime_lifecycle.exit_wait.is_some());
            assert!(
                !std::iter::from_fn(|| requests.try_recv().ok())
                    .any(|c| matches!(c, Command::Save { shutdown: true, .. }))
            );
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .set_draft("newer context");
            request
        })
    });
    root.update(|_, _, cx| {
        cx.global::<NativeHost>()
            .lifecycle()
            .resolve(request, false, cx)
    });
    root.update(|_, _, cx| {
        shell.update(cx, |s, cx| {
            assert!(s.lifecycle.is_none());
            s.ingest(
                Event::RegistrationStored {
                    runtime: "a".into(),
                    generation: 7,
                    id: "a".into(),
                    path: "/a.jsonl".into(),
                },
                cx,
            );
            assert!(s.exit_revision.is_none());
            assert!(s.runtime_lifecycle.exit_wait.is_none());
            assert_eq!(s.sessions["a"].controller.state().draft, "newer context");
            assert!(s.sessions["a"].controller.state().ready);
            assert!(
                !std::iter::from_fn(|| requests.try_recv().ok()).any(|c| matches!(
                    c,
                    Command::Save { shutdown: true, .. } | Command::Start { .. }
                ))
            );
        });
    });
}
