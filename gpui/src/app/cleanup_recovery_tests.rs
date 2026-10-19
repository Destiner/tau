use super::*;

fn release(s: &mut Shell, key: &str) -> RuntimeToken {
    let live = &s.sessions[key];
    let token = RuntimeToken {
        runtime: key.into(),
        attempt: live.start_attempt,
        generation: live.controller.generation(),
    };
    s.runtime_lifecycle.releases.insert(
        token.clone(),
        Release {
            reason: ReleaseReason::Idle(live.controller.state().identity.clone()),
            failed: false,
        },
    );
    token
}

fn failed(s: &mut Shell, token: &RuntimeToken, cx: &mut Context<Shell>) {
    s.ingest(
        Event::RuntimeReleased {
            runtime: token.runtime.clone(),
            attempt: token.attempt,
            generation: token.generation,
            success: false,
        },
        cx,
    );
}

#[test]
fn remote_archive_is_registry_only_and_does_not_stop_the_new_active_runtime() {
    with_shell(512, |s, w, cx, requests| {
        for key in ["a", "b"] {
            saved(s, key, w, cx);
            reads(s, key, key, &format!("/{key}.jsonl"), cx, &requests);
            s.sessions.get_mut(key).unwrap().project.connection_string = Some("unused-fake".into());
        }
        s.active = "a".into();
        assert!(s.session_action(&Action::Archive("a".into()), w, cx));
        assert_eq!(s.active, "b");
        let commands: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok()).collect();
        assert!(!commands.iter().any(|command| matches!(
            command,
            Command::ReleaseRuntime { .. }
                | Command::CancelStart { .. }
                | Command::DisposeEmpty { .. }
        )));
        let (project, id, revision) = commands
            .into_iter()
            .find_map(|command| match command {
                Command::Archive {
                    project,
                    id,
                    revision,
                    archived: true,
                } => Some((project, id, revision)),
                _ => None,
            })
            .unwrap();
        s.ingest(
            Event::Archived {
                project,
                id,
                revision,
                archived: true,
                success: true,
            },
            cx,
        );
        let row = &s.workspace.as_ref().unwrap().projects[0].sessions[0];
        assert!(row.archived);
        assert_eq!(row.path, "/a.jsonl");
        assert!(s.sessions["b"].controller.state().ready);
        assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
        assert!(s.error.is_none());
        assert!(!s.retry_workflow_pending());
    });
}

#[test]
fn uncertain_release_surfaces_once_at_owner_and_never_offers_or_dispatches_retry() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        saved(s, "b", w, cx);
        reads(s, "b", "b", "/b.jsonl", cx, &requests);
        s.active = "b".into();
        let token = release(s, "a");
        s.ingest(
            Event::Pi(tau_core::pi::PiEvent {
                runtime_id: "a".into(),
                generation: token.generation,
                kind: "exited",
                line: None,
                message: None,
                code: Some(255),
            }),
            cx,
        );
        s.ingest(
            Event::CleanupBlocked {
                runtime: "a".into(),
                attempt: token.attempt,
                generation: Some(token.generation),
            },
            cx,
        );
        failed(s, &token, cx);
        assert!(s.error.is_none(), "background failure waits for its owner");
        s.select("b", w, cx);
        assert!(s.error.is_none());
        s.select("a", w, cx);
        assert_eq!(
            s.workflow_failure("a").unwrap().1,
            runtimes::REMOTE_CLEANUP_ERROR
        );
        assert!(!s.fallback_retry_pending());
        assert!(!s.retry_workflow_pending());
        assert!(s.runtime_cleanup_pending("a"));
        assert!(
            s.workflow_operations_pending(),
            "closing still awaits cleanup"
        );
        // UI acknowledgement is exercised through a real Root below.
        let occurrence = s.workflow_failure("a").unwrap().0;
        assert!(s.acknowledge_workflow_failure(&occurrence));
        s.sync(w, cx);
        assert!(
            !s.fallback_is_open(cx),
            "cleanup must not become a second, inert Reconnect incident"
        );
        assert!(
            s.sessions["a"].controller.state().error.is_some(),
            "the protocol failure is not falsely acknowledged"
        );
        s.select("b", w, cx);
        s.select("a", w, cx);
        failed(s, &token, cx); // duplicate receipt, not another occurrence
        assert!(s.workflow_failure("a").is_none());
        s.retry_workflows(cx); // direct ingress, not just a hidden button
        s.retry_pi("a", cx);
        assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(|c|
            matches!(c, Command::ReleaseRuntime { .. } | Command::Start { .. })
                || matches!(c, Command::Send { request, .. } if request["type"] == "prompt" || request["type"] == "extension_ui_response")));
        assert!(s.runtime_cleanup_pending("a"));
        assert!(!s.runtime_cleanup_pending("b"));
        assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
    });
}

#[test]
fn undismissed_cleanup_feedback_cannot_follow_selection_to_healthy_runtime() {
    with_shell(512, |s, w, cx, requests| {
        for key in ["a", "b"] {
            saved(s, key, w, cx);
            reads(s, key, key, &format!("/{key}.jsonl"), cx, &requests);
        }
        s.active = "a".into();
        let token = release(s, "a");
        failed(s, &token, cx);
        let occurrence = s.workflow_failure("a").unwrap().0;
        s.select("b", w, cx);
        assert!(s.error.is_none());
        assert!(s.workflow_failure("b").is_none());
        s.select("a", w, cx);
        assert_eq!(
            s.workflow_failure("a").unwrap().0,
            occurrence,
            "navigation must not acknowledge an unseen failure"
        );
        assert!(
            s.retry_workflow_pending(),
            "recoverable cleanup remains retryable"
        );
        s.retry_workflows(cx);
        failed(s, &token, cx);
        assert_eq!(s.workflow_failure("a").unwrap().1, runtimes::CLEANUP_ERROR);
        assert!(
            s.fallback_retry_pending(),
            "a new failed attempt is a new occurrence"
        );
    });
}

#[test]
fn drained_normal_exit_is_reconciled_by_exact_release_receipt_not_transport_failure() {
    for remote in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            saved(s, "a", w, cx);
            reads(s, "a", "a", "/a.jsonl", cx, &requests);
            if remote {
                s.sessions.get_mut("a").unwrap().project.connection_string =
                    Some("unused-fake".into());
            }
            s.active = "other".into();
            let token = release(s, "a");
            s.ingest(
                Event::Pi(tau_core::pi::PiEvent {
                    runtime_id: "a".into(),
                    generation: token.generation,
                    kind: "exited",
                    line: None,
                    message: None,
                    code: Some(0),
                }),
                cx,
            );
            assert!(
                s.sessions["a"].controller.state().ready,
                "terminal event is not a stop receipt"
            );
            s.ingest(
                Event::RuntimeReleased {
                    runtime: "a".into(),
                    attempt: token.attempt,
                    generation: token.generation,
                    success: true,
                },
                cx,
            );
            assert!(!s.sessions["a"].controller.state().ready);
            assert!(!s.runtime_cleanup_pending("a"));
            assert!(
                !s.runtime_lifecycle.reconnect_required.contains("a"),
                "an ordinary drained idle stop is not intervening work"
            );
            assert!(!s.retry_workflow_pending());
            assert!(s.error.is_none());
            assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
        });
    }
}

#[test]
fn stale_cleanup_block_cannot_poison_successor_or_unrelated_retry() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        let token = release(s, "a");
        s.ingest(
            Event::CleanupBlocked {
                runtime: "a".into(),
                attempt: token.attempt + 1,
                generation: Some(token.generation),
            },
            cx,
        );
        s.ingest(
            Event::CleanupBlocked {
                runtime: "a".into(),
                attempt: token.attempt,
                generation: Some(token.generation + 1),
            },
            cx,
        );
        s.active = "a".into();
        failed(s, &token, cx);
        assert!(s.fallback_retry_pending());
        assert!(s.runtime_lifecycle.blocked_cleanup.is_empty());
        s.ingest(
            Event::CleanupBlocked {
                runtime: "a".into(),
                attempt: token.attempt,
                generation: Some(token.generation),
            },
            cx,
        );
        s.save_failed = true;
        assert!(
            s.retry_workflow_pending(),
            "independent storage failure is still recoverable"
        );
        assert!(
            !s.fallback_retry_pending(),
            "it cannot supply Retry to blocked cleanup"
        );
    });
}

#[test]
fn obsolete_save_failure_does_not_resurrect_phantom_retry_or_replace_current_preferences() {
    with_shell(512, |s, _, cx, _| {
        s.save_revision = 12;
        s.preferences.selected_draft = "current".into();
        let old = service::Preferences {
            selected_draft: "old".into(),
            ..Default::default()
        };
        s.pending_saves.insert(11, old);
        s.ingest(
            Event::Saved {
                revision: 12,
                success: true,
                stopped: vec![],
            },
            cx,
        );
        s.ingest(
            Event::Saved {
                revision: 11,
                success: false,
                stopped: vec![],
            },
            cx,
        );
        assert!(!s.retry_workflow_pending());
        assert!(s.error.is_none());
        assert_eq!(s.preferences.selected_draft, "current");
        s.pending_saves.insert(10, service::Preferences::default());
        s.ingest(
            Event::Saved {
                revision: 10,
                success: true,
                stopped: vec![],
            },
            cx,
        );
        assert_eq!(s.preferences.selected_draft, "current");
        s.ingest(
            Event::Saved {
                revision: 12,
                success: false,
                stopped: vec![],
            },
            cx,
        );
        assert!(s.retry_workflow_pending());
    });
}

#[test]
fn late_exit_after_failed_stop_disconnects_without_waiting_for_another_receipt() {
    for blocked in [false, true] {
        for early_exit in [false, true] {
            with_shell(512, |s, w, cx, requests| {
                saved(s, "a", w, cx);
                reads(s, "a", "a", "/a.jsonl", cx, &requests);
                s.sessions.get_mut("a").unwrap().project.connection_string =
                    Some("unused-fake".into());
                let token = release(s, "a");
                let exit = || {
                    Event::Pi(tau_core::pi::PiEvent {
                        runtime_id: "a".into(),
                        generation: token.generation,
                        kind: "exited",
                        line: None,
                        message: None,
                        code: Some(255),
                    })
                };
                if early_exit {
                    s.ingest(exit(), cx);
                }
                if blocked {
                    s.ingest(
                        Event::CleanupBlocked {
                            runtime: "a".into(),
                            attempt: token.attempt,
                            generation: Some(token.generation),
                        },
                        cx,
                    );
                }
                failed(s, &token, cx);
                if !early_exit {
                    assert!(s.sessions["a"].controller.state().ready);
                }
                s.ingest(exit(), cx);
                assert!(!s.sessions["a"].controller.state().ready);
                assert!(s.sessions["a"].controller.state().error.is_some());
                assert!(!s.runtime_lifecycle.release_exits.contains(&token));
                assert!(s.runtime_cleanup_pending("a"));
                assert_eq!(s.retry_workflow_pending(), !blocked);
                s.retry_pi("a", cx);
                assert!(
                    !std::iter::from_fn(|| requests.try_recv().ok())
                        .any(|c| matches!(c, Command::Start { .. }))
                );
                assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
            });
        }
    }
}

#[test]
fn failed_release_late_exit_retains_intervening_work_uncertainty_and_output_without_replay() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        let token = release(s, "a");
        s.ingest(
            rpc("a", token.generation, json!({"type":"agent_start"})),
            cx,
        );
        s.ingest(rpc("a", token.generation, json!({"type":"message_end", "message":{
            "role":"assistant", "content":[{"type":"text", "text":"Retained output"}], "stopReason":"stop"
        }})), cx);
        s.ingest(
            Event::CleanupBlocked {
                runtime: "a".into(),
                attempt: token.attempt,
                generation: Some(token.generation),
            },
            cx,
        );
        failed(s, &token, cx);
        s.ingest(
            Event::Pi(tau_core::pi::PiEvent {
                runtime_id: "a".into(),
                generation: token.generation,
                kind: "exited",
                line: None,
                message: None,
                code: Some(255),
            }),
            cx,
        );
        let live = &s.sessions["a"];
        assert!(!live.controller.state().ready);
        assert!(live.controller.reconciliation_pending());
        assert!(!live.controller.state().transcript.is_empty());
        assert_eq!(live.controller.state().draft, "draft a");
        assert!(s.runtime_cleanup_pending("a"));
        s.retry_workflows(cx);
        s.retry_pi("a", cx);
        assert!(!std::iter::from_fn(|| requests.try_recv().ok()).any(|c|
            matches!(c, Command::Start { .. } | Command::ReleaseRuntime { .. })
                || matches!(c, Command::Send { request, .. } if request["type"] == "prompt" || request["type"] == "extension_ui_response")));
    });
}

fn workflow_failure_for_test(
    s: &mut Shell,
    recovery: bool,
    cx: &mut Context<Shell>,
) -> WorkflowErrorToken {
    if recovery {
        let original = s.sessions["a"].controller.state().identity.clone();
        s.runtime_lifecycle.recoveries.insert(
            (project().path, original.id.clone(), 31),
            Recovery {
                runtime: "a".into(),
                generation: 7,
                original,
                failed: false,
            },
        );
        assert!(s.finish_source_recovery(&project().path, "a", "/a.jsonl", 31, false, cx));
    } else {
        let token = release(s, "a");
        failed(s, &token, cx);
    }
    s.workflow_failure("a").unwrap().0
}

#[test]
fn actual_root_suppressed_and_overwritten_workflow_error_survives_until_exact_dismissal() {
    use gpui_kit::base::Root;
    for recovery in [false, true] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        let occurrence = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                for key in ["a", "b"] {
                    saved(s, key, w, cx);
                    reads(s, key, key, &format!("/{key}.jsonl"), cx, &requests);
                }
                s.active = "a".into();
                s.confirmation = Some(cx.new(|cx| {
                    NativeDialog::new(
                        DialogProps {
                            kind: DialogKind::Quit,
                            title: "Other operation".into(),
                            description: String::new(),
                            placeholder: String::new(),
                            options: vec![],
                        },
                        false,
                        w,
                        cx,
                    )
                }));
                let occurrence = workflow_failure_for_test(s, recovery, cx);
                s.sync(w, cx);
                assert!(
                    s.fallback.is_none(),
                    "another surface suppresses presentation"
                );
                // Model a programmatic selection race while another modal owns input.
                s.active = "b".into();
                s.confirmation = None;
                s.sync(w, cx);
                assert!(s.fallback.is_none());
                s.select("a", w, cx);
                assert_eq!(s.workflow_failure("a").unwrap().0, occurrence);
                occurrence
            })
        });
        let old = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.sync(w, cx);
                s.fallback.as_ref().unwrap().1.clone()
            })
        });
        let message = root.update(|_, _, cx| shell.read(cx).workflow_failure("a").unwrap().1);
        // An unowned, identical app error replaces the slot before sync. The old
        // workflow dialog must neither clear it nor acknowledge its own incident.
        root.update(|_, _, cx| shell.update(cx, |s, _| s.error = Some(message.into())));
        root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        let app_dialog = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                assert_eq!(s.error.as_deref(), Some(message));
                assert_eq!(s.workflow_failure("a").unwrap().0, occurrence);
                s.active = "b".into();
                s.sync(w, cx);
                assert_eq!(
                    s.error.as_deref(),
                    Some(message),
                    "navigation cannot clear unowned identical text"
                );
                s.active = "a".into();
                s.sync(w, cx);
                s.fallback.as_ref().unwrap().1.clone()
            })
        });
        root.update(|_, _, cx| app_dialog.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        let current = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                assert!(s.error.is_none());
                assert_eq!(s.workflow_failure("a").unwrap().0, occurrence);
                s.sync(w, cx);
                s.fallback.as_ref().unwrap().1.clone()
            })
        });
        root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        root.update(|_, _, cx| assert!(shell.read(cx).workflow_failure("a").is_some()));
        root.update(|_, _, cx| current.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                assert!(s.workflow_failure("a").is_none());
                s.select("b", w, cx);
                s.select("a", w, cx);
                s.sync(w, cx);
                assert!(s.fallback.is_none(), "explicit dismissal does not replay");
                assert!(
                    s.workflow_operations_pending(),
                    "UI dismissal is not physical/storage acknowledgement"
                );
                assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
            })
        });
        root.update(|_, w, cx| w.blur(cx));
        app.run_until_parked();
    }
}

#[test]
fn actual_root_old_cleanup_dialog_cannot_acknowledge_retry_or_clear_identical_unowned_error() {
    use gpui_kit::base::Root;
    for recovery in [false, true] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(512);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        let (token, old) = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                saved(s, "a", w, cx);
                reads(s, "a", "a", "/a.jsonl", cx, &requests);
                s.active = "a".into();
                let token = workflow_failure_for_test(s, recovery, cx);
                s.sync(w, cx);
                (token, s.fallback.as_ref().unwrap().1.clone())
            })
        });
        // Retry and fail again before any intervening render. The old entity is
        // not the new occurrence even though its owner and message are identical.
        root.update(|_, _, cx| {
            shell.update(cx, |s, cx| {
                s.retry_workflows(cx);
                match &token {
                    WorkflowErrorToken::Release(token) => failed(s, token, cx),
                    WorkflowErrorToken::Recovery(project, id, revision) => {
                        assert!(
                            s.finish_source_recovery(project, id, "/a.jsonl", *revision, false, cx)
                        );
                    }
                    WorkflowErrorToken::Cleanup(..) => unreachable!(),
                }
            })
        });
        root.update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        let current = root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                assert_eq!(s.workflow_failure("a").unwrap().0, token);
                s.sync(w, cx);
                let current = s.fallback.as_ref().unwrap().1.clone();
                assert_ne!(current, old);
                current
            })
        });
        let message = root.update(|_, _, cx| {
            shell.update(cx, |s, cx| {
                let message = s.workflow_failure("a").unwrap().1;
                s.error = Some(message.into());
                match &token {
                    WorkflowErrorToken::Release(token) => s.finish_runtime_release(
                        token.runtime.clone(),
                        token.attempt,
                        token.generation,
                        true,
                        cx,
                    ),
                    WorkflowErrorToken::Recovery(project, id, revision) => {
                        assert!(
                            s.finish_source_recovery(project, id, "/a.jsonl", *revision, true, cx)
                        );
                    }
                    WorkflowErrorToken::Cleanup(..) => unreachable!(),
                }
                message
            })
        });
        root.update(|_, _, cx| current.update(cx, |_, cx| cx.emit(DialogEvent::Cancelled)));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                assert_eq!(s.error.as_deref(), Some(message));
                assert!(s.workflow_failure("a").is_none());
                s.sync(w, cx);
                assert!(
                    s.fallback.is_some(),
                    "unowned identical error remains presentable"
                );
            })
        });
        root.update(|_, w, cx| w.blur(cx));
        app.run_until_parked();
    }
}

#[test]
fn successful_workflow_receipt_never_clears_unowned_identical_app_error() {
    for recovery in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            saved(s, "a", w, cx);
            reads(s, "a", "a", "/a.jsonl", cx, &requests);
            s.active = "a".into();
            let token = workflow_failure_for_test(s, recovery, cx);
            let message = s.workflow_failure("a").unwrap().1;
            assert!(s.acknowledge_workflow_failure(&token));
            s.retry_workflows(cx);
            match &token {
                WorkflowErrorToken::Release(token) => failed(s, token, cx),
                WorkflowErrorToken::Recovery(project, id, revision) => {
                    assert!(
                        s.finish_source_recovery(project, id, "/a.jsonl", *revision, false, cx)
                    );
                }
                WorkflowErrorToken::Cleanup(..) => unreachable!(),
            }
            assert!(
                s.workflow_failure("a").is_some(),
                "retry retires the previous acknowledgement"
            );
            assert!(s.acknowledge_workflow_failure(&token));
            s.error = Some(message.into());
            match token {
                WorkflowErrorToken::Release(token) => s.finish_runtime_release(
                    token.runtime,
                    token.attempt,
                    token.generation,
                    true,
                    cx,
                ),
                WorkflowErrorToken::Recovery(project, id, revision) => {
                    assert!(
                        s.finish_source_recovery(&project, &id, "/a.jsonl", revision, true, cx)
                    );
                }
                WorkflowErrorToken::Cleanup(..) => unreachable!(),
            }
            assert_eq!(s.error.as_deref(), Some(message));
            assert!(s.workflow_failure("a").is_none());
            assert!(s.runtime_lifecycle.acknowledged_releases.is_empty());
            assert!(s.runtime_lifecycle.acknowledged_recoveries.is_empty());
        });
    }
}

fn save_snapshot(text: &str) -> service::Preferences {
    service::Preferences {
        selected_draft: text.into(),
        sidebar_width: Some(200.0),
        drafts: vec![service::Draft {
            key: "a".into(),
            text: text.into(),
            recovered: vec![format!("recovered {text}")],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn fifo_old_save_success_advances_durable_baseline_even_when_newer_save_fails() {
    for prior_failure in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            saved(s, "a", w, cx);
            reads(s, "a", "a", "/a.jsonl", cx, &requests);
            s.sessions.get_mut("a").unwrap().persisted_draft = "A".into();
            // The user has temporarily returned to B while B/C writes are
            // pending, then restores C below. This keeps the receipt assertion
            // independent of automatic follow-up save dispatch.
            s.sessions.get_mut("a").unwrap().controller.set_draft("B");
            s.sessions.get_mut("a").unwrap().restored_recovered = vec!["recovered B".into()];
            s.preferences.selected_draft = "latest preferences".into();
            s.preferences.sidebar_width = Some(321.0);
            s.save_revision = 2;
            s.pending_saves.insert(1, save_snapshot("B"));
            s.pending_saves.insert(2, save_snapshot("C"));
            s.save_failed = prior_failure;
            s.error = prior_failure.then(|| "Drafts could not be saved. Try again.".into());
            s.ingest(
                Event::Saved {
                    revision: 1,
                    success: true,
                    stopped: vec![],
                },
                cx,
            );
            assert_eq!(s.sessions["a"].persisted_draft, "B");
            assert_eq!(s.sessions["a"].persisted_recovered, 1);
            assert_eq!(
                s.save_failed, prior_failure,
                "stale success cannot retire latest failure state"
            );
            assert_eq!(
                s.error.as_deref(),
                prior_failure.then_some("Drafts could not be saved. Try again.")
            );
            s.sessions.get_mut("a").unwrap().controller.set_draft("C");
            s.ingest(
                Event::Saved {
                    revision: 2,
                    success: false,
                    stopped: vec![],
                },
                cx,
            );
            assert_eq!(s.saved_revision, 1);
            assert_eq!(s.sessions["a"].persisted_draft, "B");
            assert_eq!(s.sessions["a"].controller.state().draft, "C");
            assert_eq!(s.preferences.selected_draft, "latest preferences");
            assert_eq!(s.preferences.sidebar_width, Some(321.0));
            assert!(s.save_failed);
            assert!(s.error.is_some());
        });
    }
}

#[test]
fn successful_newer_save_and_stale_failure_cannot_roll_baseline_or_feedback_back() {
    for fifo in [false, true] {
        with_shell(512, |s, w, cx, requests| {
            saved(s, "a", w, cx);
            reads(s, "a", "a", "/a.jsonl", cx, &requests);
            s.sessions.get_mut("a").unwrap().controller.set_draft("D");
            s.save_revision = 2;
            s.pending_saves.insert(1, save_snapshot("B"));
            s.pending_saves.insert(2, save_snapshot("C"));
            s.save_failed = true;
            s.error = Some("Drafts could not be saved. Try again.".into());
            for revision in if fifo { [1, 2] } else { [2, 1] } {
                s.ingest(
                    Event::Saved {
                        revision,
                        success: true,
                        stopped: vec![],
                    },
                    cx,
                );
            }
            s.ingest(
                Event::Saved {
                    revision: 1,
                    success: false,
                    stopped: vec![],
                },
                cx,
            );
            assert_eq!(s.saved_revision, 2);
            assert_eq!(s.sessions["a"].persisted_draft, "C");
            assert_eq!(s.sessions["a"].persisted_recovered, 1);
            assert_eq!(s.sessions["a"].controller.state().draft, "D");
            assert_eq!(s.preferences.selected_draft, "C");
            assert!(!s.save_failed);
            assert!(s.error.is_none());
        });
    }
}

#[test]
fn nonrelease_feedback_acknowledgements_are_owned_and_retired_without_string_clearing() {
    for empty in [false, true] {
        with_shell(512, |s, _, cx, _| {
            if empty {
                s.empty_disposals.insert(("gone".into(), 4, 7));
                s.empty_disposed("gone".into(), 4, 7, false, cx);
            } else {
                s.runtime_lifecycle_track_cancel("gone", 4);
                s.finish_start_cancel("gone".into(), 4, false, cx);
            }
            let (token, message) = s.workflow_failure(&s.active).unwrap();
            assert!(s.acknowledge_workflow_failure(&token));
            assert!(s.workflow_failure(&s.active).is_none());
            s.retry_workflows(cx);
            assert!(s.runtime_lifecycle.acknowledged_cleanup.is_empty());
            if empty {
                s.empty_disposed("gone".into(), 4, 7, false, cx);
            } else {
                s.finish_start_cancel("gone".into(), 4, false, cx);
            }
            assert!(s.acknowledge_workflow_failure(&token));
            s.error = Some(message.into());
            if empty {
                s.empty_disposed("gone".into(), 5, 7, true, cx);
                assert_eq!(s.runtime_lifecycle.acknowledged_cleanup.len(), 1);
                s.empty_disposed("gone".into(), 4, 7, true, cx);
            } else {
                s.finish_start_cancel("gone".into(), 5, true, cx);
                assert_eq!(s.runtime_lifecycle.acknowledged_cleanup.len(), 1);
                s.finish_start_cancel("gone".into(), 4, true, cx);
            }
            assert!(s.runtime_lifecycle.acknowledged_cleanup.is_empty());
            assert!(s.register_retries.is_empty());
            assert_eq!(s.error.as_deref(), Some(message));
        });
    }
}

#[test]
fn blocked_empty_and_cancel_cleanup_keep_ack_owners_without_actionable_retry() {
    with_shell(512, |s, _, cx, requests| {
        s.empty_disposals.insert(("empty".into(), 2, 7));
        s.ingest(
            Event::CleanupBlocked {
                runtime: "empty".into(),
                attempt: 2,
                generation: Some(7),
            },
            cx,
        );
        s.ingest(
            Event::EmptyDisposed {
                runtime: "empty".into(),
                attempt: 2,
                generation: 7,
                success: false,
            },
            cx,
        );
        assert!(!s.retry_workflow_pending());
        assert!(!s.fallback_retry_pending());
        s.runtime_lifecycle_track_cancel("starting", 4);
        s.ingest(
            Event::CleanupBlocked {
                runtime: "starting".into(),
                attempt: 4,
                generation: None,
            },
            cx,
        );
        s.ingest(
            Event::StartCancelled {
                runtime: "starting".into(),
                attempt: 4,
                success: false,
            },
            cx,
        );
        assert!(!s.retry_workflow_pending());
        assert!(!s.fallback_retry_pending());
        s.retry_workflows(cx);
        assert_eq!(
            s.register_retries.len(),
            2,
            "failed acknowledgements remain owned"
        );
        assert!(s.workflow_operations_pending());
        assert_eq!(s.empty_disposals.len(), 1);
        assert!(requests.try_recv().is_err());
    });
}
