use super::*;
use tau_core::pi::PiEvent;

fn release(s: &mut Shell, cx: &mut Context<Shell>) -> RuntimeToken {
    let live = &s.sessions["a"];
    let token = RuntimeToken {
        runtime: "a".into(),
        attempt: live.start_attempt,
        generation: 7,
    };
    s.runtime_lifecycle.releases.insert(
        token.clone(),
        Release {
            reason: ReleaseReason::Idle(live.controller.state().identity.clone()),
            failed: false,
        },
    );
    s.dispatch(
        Command::ReleaseRuntime {
            runtime: token.runtime.clone(),
            attempt: token.attempt,
            generation: 7,
        },
        cx,
    );
    token
}
fn released(s: &mut Shell, token: &RuntimeToken, cx: &mut Context<Shell>) {
    s.ingest(
        Event::RuntimeReleased {
            runtime: token.runtime.clone(),
            attempt: token.attempt,
            generation: token.generation,
            success: true,
        },
        cx,
    );
}

#[test]
fn physical_release_reconciles_intervening_dialog_work_replacement_and_error_in_both_orders() {
    for kind in ["dialog", "work", "replace", "error"] {
        for ack_first in [false, true] {
            with_shell(512, |s, w, cx, requests| {
                saved(s, "a", w, cx);
                reads(s, "a", "a", "/a.jsonl", cx, &requests);
                s.active = "other".into();
                let replacement = if kind == "replace" {
                    // Capture an already-dispatched read, not a fabricated response
                    // to a Send that cleanup admission would reject.
                    let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
                    s.effects("a", effects, cx);
                    std::iter::from_fn(|| requests.try_recv().ok()).find_map(|c| match c {
                        Command::Send { request, .. } if request["type"] == "get_state" => {
                            Some(request)
                        }
                        _ => None,
                    })
                } else {
                    None
                };
                let token = release(s, cx);
                if ack_first {
                    released(s, &token, cx);
                }
                match kind {
                    "dialog" => s.ingest(rpc("a", 7, json!({"type":"extension_ui_request", "id":"input", "method":"input", "title":"Input"})), cx),
                    "work" => {
                        s.ingest(rpc("a", 7, json!({"type":"agent_start"})), cx);
                        s.ingest(rpc("a", 7, json!({"type":"message_end", "message":{"role":"assistant", "content":[{"type":"text", "text":"Retained output"}], "stopReason":"stop"}})), cx);
                    }
                    "replace" => {
                        let request = replacement.unwrap();
                        s.ingest(rpc("a", 7, json!({"type":"response", "id":request["id"], "command":"get_state", "success":true,
                            "data":{"sessionId":"b", "sessionFile":"/b.jsonl", "isStreaming":false}})), cx);
                    }
                    _ => s.ingest(Event::Pi(PiEvent { runtime_id: "a".into(), generation: 7, kind: "error", line: None, message: None, code: None }), cx),
                }
                if !ack_first {
                    released(s, &token, cx);
                }
                let live = &s.sessions["a"];
                assert!(!live.controller.state().ready);
                assert!(!live.controller.state().working);
                assert!(live.controller.state().extension_dialogs.is_empty());
                assert_eq!(live.controller.state().draft, "draft a");
                if kind == "work" && !ack_first {
                    assert!(!live.controller.state().transcript.is_empty());
                    assert!(
                        live.controller.reconciliation_pending(),
                        "stopped accepted work remains uncertain"
                    );
                }
                assert_eq!(
                    live.controller.state().identity.id,
                    if kind == "replace" && !ack_first {
                        "b"
                    } else {
                        "a"
                    }
                );
                assert!(s.runtime_was_released("a"));
                assert!(!s.runtime_cleanup_pending("a"));
                while requests.try_recv().is_ok() {}
                s.select("a", w, cx);
                let automatic = std::iter::from_fn(|| requests.try_recv().ok())
                    .any(|c| matches!(c, Command::Start { .. }));
                assert_eq!(
                    automatic,
                    ack_first && kind != "replace",
                    "only the still-idle valid local saved route can automatically resume"
                );
            });
        }
    }
}

#[test]
fn physical_release_ack_cannot_disconnect_a_newer_runtime_or_consume_failed_registration() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        s.select("a", w, cx);
        let token = release(s, cx);
        let registration = Command::Register {
            runtime: "a".into(),
            generation: 7,
            project: project().path,
            id: "a".into(),
            path: "/a.jsonl".into(),
            name: "A".into(),
        };
        s.dispatch(registration.clone(), cx);
        s.ingest(Event::RegistrationFailed(registration.clone()), cx);
        s.start_live("a", cx);
        assert!(!s.sessions["a"].starting);
        assert_eq!(s.sessions["a"].start_attempt, token.attempt);
        assert!(s.sessions["a"].controller.state().ready);
        // Pending cleanup blocks new bootstrap writes. Its first physical receipt
        // permits the real local saved-source restart; only a duplicate old ack
        // can then arrive after the successor generation has become ready.
        released(s, &token, cx);
        assert!(s.sessions["a"].starting);
        assert!(s.sessions["a"].start_attempt > token.attempt);
        assert!(
            std::iter::from_fn(|| requests.try_recv().ok()).any(|c| matches!(c,
            Command::Start { runtime, attempt, source: Some(source), .. }
                if runtime == "a" && attempt > token.attempt && source == "/a.jsonl"))
        );
        started(s, "a", 8, cx);
        reads(s, "a", "b", "/b.jsonl", cx, &requests);
        assert!(s.sessions["a"].controller.state().ready);
        released(s, &token, cx);
        assert!(s.sessions["a"].controller.state().ready);
        assert!(!s.runtime_was_released("a"));
        s.retry_workflows(cx);
        assert!(std::iter::from_fn(|| requests.try_recv().ok()).any(|c| matches!(c, Command::Register { generation: 7, id, path, .. } if id == "a" && path == "/a.jsonl")));
        s.ingest(Event::RegistrationFailed(registration), cx);
        assert_eq!(s.register_retries.len(), 1);
        assert!(
            s.sessions["a"]
                .registration_pending
                .contains(&(7, "a".into(), "/a.jsonl".into()))
        );
        s.retry_workflows(cx);
        s.ingest(
            Event::RegistrationStored {
                runtime: "a".into(),
                generation: 7,
                id: "a".into(),
                path: "/a.jsonl".into(),
            },
            cx,
        );
        assert!(s.register_retries.is_empty());
        assert!(s.sessions["a"].registration_pending.is_empty());
        assert_eq!(s.sessions["a"].controller.generation(), 8);
        assert_eq!(s.sessions["a"].controller.state().identity.id, "b");
    });
}

#[test]
fn root_quit_drains_barrier_and_storage_before_stop_and_waits_for_late_storage_receipts() {
    for (stored_before_saved, revise_text) in [(false, false), (true, false), (true, true)] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        let mut replacement_state = None;
        root.update(|_, w, cx| {
            let owner = shell.downgrade();
            let host = NativeHost::install(cx, "Tau", Duration::from_secs(60), HostCallbacks {
                lifecycle: LifecycleCallbacks {
                    requested: Rc::new(move |request, cx| { let _ = owner.update(cx, |s, _| s.lifecycle = Some(request)); }),
                    settled: Rc::new(|_, _, _| {}),
                },
                new_session: Rc::new(|_| {}), create_window: Rc::new(|_| Err(anyhow::anyhow!("fixture"))), reveal_window: Rc::new(|_, _| {}), window_failed: Rc::new(|_| {}),
            });
            host.lifecycle().attach(w, cx);
            shell.update(cx, |s, cx| {
                create(s, "a", "keep", w, cx);
                started(s, "a", 7, cx);
                reads(s, "a", "a", "/a.jsonl", cx, &requests);
                s.ingest(rpc("a", 7, json!({"type":"agent_start"})), cx);
                s.ingest(rpc("a", 7, json!({"type":"message_end", "message":{"role":"assistant", "content":[{"type":"text", "text":"finished"}], "stopReason":"stop"}})), cx);
                // Keep the durability barrier genuinely outstanding at the Controller.
                while requests.try_recv().is_ok() {}
                assert!(s.sessions["a"].controller.durability_pending());
                let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
                s.effects("a", effects, cx);
                replacement_state = std::iter::from_fn(|| requests.try_recv().ok()).find_map(|c| {
                    match c {
                        Command::Send { request, .. } if request["type"] == "get_state" => Some(request),
                        _ => None,
                    }
                });
                assert!(replacement_state.is_some());
            });
            cx.quit();
        });
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                let request = s.lifecycle.unwrap();
                s.begin_exit(request, true, cx);
                s.sync(w, cx);
                assert_eq!(s.runtime_lifecycle.exit_wait, Some((request, true)));
                let first: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok()).collect();
                assert!(first.iter().any(
                    |c| matches!(c, Command::Register { generation: 7, id, .. } if id == "a")
                ));
                assert!(
                    !first
                        .iter()
                        .any(|c| matches!(c, Command::Save { shutdown: true, .. }))
                );
                assert!(s.exit_revision.is_none());
                s.ingest(
                    Event::RegistrationStored {
                        runtime: "a".into(),
                        generation: 7,
                        id: "a".into(),
                        path: "/a.jsonl".into(),
                    },
                    cx,
                );
                let (revision, _) = s.exit_revision.unwrap();
                assert!(
                    std::iter::from_fn(|| requests.try_recv().ok())
                        .any(|c| matches!(c, Command::Save { shutdown: true, .. }))
                );
                // A real queued state response adopts B while shutdown Save is in
                // flight; newly completed B output must get its own storage owner.
                let state = replacement_state.take().unwrap();
                s.ingest(rpc("a", 7, json!({"type":"response", "id":state["id"], "command":"get_state", "success":true,
                    "data":{"sessionId":"b", "sessionFile":"/b.jsonl", "isStreaming":false}})), cx);
                s.ingest(rpc("a", 7, json!({"type":"message_end", "message":{"role":"assistant", "content":[{"type":"text", "text":"B finished"}], "stopReason":"stop"}})), cx);
                let late = std::iter::from_fn(|| requests.try_recv().ok()).find(|c| {
                    matches!(c, Command::Register { generation: 7, id, path, .. } if id == "b" && path == "/b.jsonl")
                }).expect("new late output must hand off real Controller storage proof");
                assert_eq!(s.sessions["a"].controller.state().identity.id, "b");
                let ack = || Event::RegistrationStored {
                    runtime: "a".into(), generation: 7, id: "b".into(), path: "/b.jsonl".into(),
                };
                if stored_before_saved {
                    s.ingest(ack(), cx);
                }
                s.ingest(
                    Event::Saved {
                        revision,
                        success: true,
                        stopped: vec![("a".into(), 7)],
                    },
                    cx,
                );
                assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                assert!(s.runtime_lifecycle.exit_fence_pending.is_some());
                s.sync(w, cx);
                assert_eq!(s.runtime_lifecycle.exit_fence_pending, Some(revision));
                assert!(s.exit_revision.is_none());
                if revise_text {
                    s.sessions.get_mut("a").unwrap().controller.set_draft("newer");
                }
                s.ingest(Event::ExitDrained { revision }, cx);
                if stored_before_saved {
                    assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                    let (new_revision, _) = s.exit_revision.unwrap();
                    assert!(new_revision > revision);
                    s.ingest(Event::Saved { revision: new_revision, success: true, stopped: vec![] }, cx);
                    assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                    s.ingest(Event::ExitDrained { revision: new_revision }, cx);
                }
                if !stored_before_saved {
                    assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                    s.ingest(Event::RegistrationFailed(late), cx);
                    s.sync(w, cx);
                    assert_eq!(s.quit_error.as_ref().unwrap().0, request);
                    assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                    let failed_owner = s.runtime_lifecycle.exit_saved;
                    let failed_revision = s.save_revision;
                    s.begin_exit(request, true, cx);
                    s.sync(w, cx);
                    assert_eq!(s.runtime_lifecycle.exit_saved, failed_owner);
                    assert_eq!(s.save_revision, failed_revision);
                    assert!(s.quit_error.is_some());
                    assert!(!s.confirmation.as_ref().unwrap().read(cx).is_busy());
                    s.retry_exit(request, true, cx);
                    s.ingest(ack(), cx);
                    let (retry_revision, _) = s.exit_revision.unwrap();
                    s.ingest(
                        Event::Saved {
                            revision: retry_revision,
                            success: true,
                            stopped: vec![],
                        },
                        cx,
                    );
                    assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                    s.ingest(
                        Event::ExitDrained {
                            revision: retry_revision,
                        },
                        cx,
                    );
                }
                assert!(!cx.global::<NativeHost>().lifecycle().is_pending(request));
                assert!(s.preferences.drafts.iter().any(|draft| draft.key == "a" && draft.source_id == "b" && draft.source_path == "/b.jsonl"));
                assert_eq!(s.sessions["a"].controller.state().draft, if revise_text { "newer" } else { "keep" });
            });
        });
    }
}

#[test]
fn root_event_batches_preserve_automatic_and_confirmed_quit_save_fence_owners() {
    for busy in [false, true] {
        for resave in [false, true] {
            let mut app = app();
            let (commands, requests) = async_channel::bounded(512);
            let shell = app.new_entity(|_| shell(commands));
            let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
            let baseline = root.update(|_, w, cx| {
                let owner = shell.downgrade();
                let host = NativeHost::install(
                    cx,
                    "Tau",
                    Duration::from_secs(60),
                    HostCallbacks {
                        lifecycle: LifecycleCallbacks {
                            requested: Rc::new(move |request, cx| {
                                let _ = owner.update(cx, |s, _| s.lifecycle = Some(request));
                            }),
                            settled: Rc::new(|_, _, _| {}),
                        },
                        new_session: Rc::new(|_| {}),
                        create_window: Rc::new(|_| Err(anyhow::anyhow!("fixture"))),
                        reveal_window: Rc::new(|_, _| {}),
                        window_failed: Rc::new(|_| {}),
                    },
                );
                host.lifecycle().attach(w, cx);
                let baseline = shell.update(cx, |s, cx| {
                    s.mount(w, cx);
                    saved(s, "a", w, cx);
                    reads(s, "a", "a", "/a.jsonl", cx, &requests);
                    s.select("a", w, cx);
                    s.restored = true;
                    if busy {
                        s.ingest(rpc("a", 7, json!({"type":"agent_start"})), cx);
                    }
                    s.persist(false, cx);
                    s.save_revision
                });
                while requests.try_recv().is_ok() {}
                cx.quit();
                baseline
            });
            let request = root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    // The same post-batch sync used by the production service consumer
                    // starts idle quit automatically, but asks before stopping work.
                    s.sync(w, cx);
                    let request = s.lifecycle.unwrap();
                    assert_eq!(s.confirmation.is_some(), busy);
                    assert_eq!(s.exit_revision.is_some(), !busy);
                    request
                })
            });
            if busy {
                root.update(|_, _, cx| {
                    let dialog = shell.read(cx).confirmation.clone().unwrap();
                    dialog.update(cx, |_, cx| cx.emit(DialogEvent::Submitted(String::new())));
                });
            }
            let revision = root.update(|_, w, cx| shell.update(cx, |s, cx| {
                s.sync(w, cx);
                let (revision, owner) = s.exit_revision.unwrap();
                assert_eq!(owner, request);
                let saves: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok())
                    .filter_map(|c| match c { Command::Save { revision, shutdown: true, .. } => Some(revision), _ => None }).collect();
                assert_eq!(saves, vec![revision]);
                // Both receipts belong to actual dispatched Saves, and the fence
                // cannot already be in the batch collected before Saved dispatches it.
                for event in [
                    Event::Saved { revision: baseline, success: true, stopped: vec![] },
                    Event::Saved { revision, success: true, stopped: vec![("a".into(), 7)] },
                ] { s.ingest(event, cx); }
                s.sync(w, cx);
                assert!(s.exit_revision.is_none());
                assert_eq!(s.runtime_lifecycle.exit_saved, Some((revision, request, true)));
                assert_eq!(s.runtime_lifecycle.exit_fence_pending, Some(revision));
                assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                if busy { assert!(s.confirmation.as_ref().unwrap().read(cx).is_busy()); }
                let commands: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok()).collect();
                assert_eq!(commands.iter().filter(|c| matches!(c, Command::ExitFence { revision: r } if *r == revision)).count(), 1);
                assert!(!commands.iter().any(|c| matches!(c, Command::Save { .. })));
                if resave { s.sessions.get_mut("a").unwrap().controller.set_draft("changed during fence"); }
                revision
            }));
            root.update(|_, w, cx| {
                shell.update(cx, |s, cx| {
                    s.ingest(Event::ExitDrained { revision }, cx);
                    s.sync(w, cx);
                    if resave {
                        let (next, owner) = s.exit_revision.unwrap();
                        assert_eq!(owner, request);
                        assert_eq!(next, revision + 1);
                        let saves: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok())
                            .filter_map(|c| match c {
                                Command::Save {
                                    revision,
                                    shutdown: true,
                                    ..
                                } => Some(revision),
                                _ => None,
                            })
                            .collect();
                        assert_eq!(saves, vec![next]);
                        // A duplicate old receipt in a later batch cannot consume the
                        // deliberate resave's owner, even beside its real Saved receipt.
                        for event in [
                            Event::ExitDrained { revision },
                            Event::Saved {
                                revision: next,
                                success: true,
                                stopped: vec![],
                            },
                        ] {
                            s.ingest(event, cx);
                        }
                        s.sync(w, cx);
                        assert_eq!(s.save_revision, next);
                        assert_eq!(s.runtime_lifecycle.exit_fence_pending, Some(next));
                        assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
                        assert!(
                            !std::iter::from_fn(|| requests.try_recv().ok())
                                .any(|c| matches!(c, Command::Save { .. }))
                        );
                        s.ingest(Event::ExitDrained { revision: next }, cx);
                        s.sync(w, cx);
                        assert_eq!(s.preferences.drafts[0].text, "changed during fence");
                    }
                    assert!(!cx.global::<NativeHost>().lifecycle().is_pending(request));
                    assert_eq!(s.save_revision, revision + u64::from(resave));
                    assert!(
                        !std::iter::from_fn(|| requests.try_recv().ok())
                            .any(|c| matches!(c, Command::Save { .. }))
                    );
                })
            });
        }
    }
}
