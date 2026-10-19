//! Production Shell composition under the real retained Root, not a product fixture.
use super::confirm_queue_fixture::{Fixture, secondary};
use super::*;
use crate::app::tests::{create, rpc};
use gpui_kit::{Focusable, test::TestWindowExt};
use serde_json::json;

#[test]
fn extension_editor_is_a_retained_transcript_tail_and_hides_queue_without_losing_draft() {
    let mut f = Fixture::new();
    f.queue(&["queued work"], &[]);
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let live = s.sessions.get_mut("a").unwrap();
            live.controller.set_draft("ordinary composer draft");
            live.dirty = true;
            s.sync(w, cx);
        })
    });
    f.ingest(
        json!({"type":"extension_ui_request", "id":"editor-request", "method":"editor",
        "title":"Edit", "prefill":"  first\nsecond  "}),
    );
    let dialog = f.root.update(|_, w, cx| {
        assert!(w.try_find("shell-transcript-slot").is_some());
        assert!(w.try_find("shell-composer-area").is_none());
        let s = f.shell.read(cx);
        let live = &s.sessions["a"];
        assert_eq!(live.controller.state().draft, "ordinary composer draft");
        assert_eq!(live.queue.read(cx).props().steering, ["queued work"]);
        let dialog = live.extension.as_ref().unwrap().1.clone();
        assert_eq!(dialog.read(cx).value(cx), "  first\nsecond  ");
        dialog
    });
    f.root.update(|_, w, cx| {
        dialog.update(cx, |d, cx| {
            d.set_value("  edited\nexact\n", w, cx);
        })
    });
    f.ingest(
        json!({"type":"auto_retry_start", "attempt":2, "maxAttempts":4, "errorMessage":"429"}),
    );
    f.root.update(|_, _, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"].extension.as_ref().unwrap().1,
            dialog
        );
        assert_eq!(dialog.read(cx).value(cx), "  edited\nexact\n");
    });
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            create(s, "b", "background draft", w, cx);
            s.select("b", w, cx);
            s.select("a", w, cx);
        })
    });
    f.root.draw();
    f.root.update(|_, _, cx| {
        assert_eq!(
            f.shell.read(cx).sessions["a"].extension.as_ref().unwrap().1,
            dialog
        );
        assert_eq!(dialog.read(cx).value(cx), "  edited\nexact\n");
    });
}

#[test]
fn failed_cancel_is_request_local_and_old_generation_dialog_cannot_answer_reused_id() {
    let mut f = Fixture::new();
    f.dialog("editor");
    let old = f.root.update(|_, _, cx| {
        f.shell.read(cx).sessions["a"]
            .extension
            .as_ref()
            .unwrap()
            .1
            .clone()
    });
    f.root.update(|_, w, cx| {
        old.update(cx, |d, cx| {
            d.set_value("retained answer\n", w, cx);
            d.focus(w, cx);
        })
    });
    f.root.draw();
    f.root.simulate_keystrokes("escape");
    let request = f
        .sent()
        .into_iter()
        .find(|r| r["type"] == "extension_ui_response")
        .unwrap();
    assert_eq!(request["cancelled"], true);
    f.root.update(|_, w, cx| f.shell.update(cx, |s, cx| {
        s.ingest(Event::Written { runtime: "a".into(), generation: 8, request, success: false }, cx);
        s.sync(w, cx);
        assert!(s.error.is_none());
        assert!(s.fallback.is_none());
        assert!(Shell::extension_failure_owned(&s.sessions["a"]));
        assert_eq!(s.sessions["a"].extension.as_ref().unwrap().1, old);
        assert_eq!(old.read(cx).value(cx), "retained answer\n");
        // Bootstrap transport is deliberately absent: exercise the production UI fence.
        let _ = s.sessions.get_mut("a").unwrap().controller.begin_generation(9);
        s.ingest(rpc("a", 9, json!({"type":"extension_ui_request", "id":"question", "method":"editor", "title":"New generation", "prefill":"new"})), cx);
        s.sessions.get_mut("a").unwrap().dirty = true;
        s.sync(w, cx);
        assert_ne!(s.sessions["a"].extension.as_ref().unwrap().1, old);
    }));
    f.sent();
    f.root
        .update(|_, _, cx| old.update(cx, |_, cx| cx.emit(DialogEvent::Submitted("stale".into()))));
    assert!(
        !f.sent()
            .iter()
            .any(|r| r["type"] == "extension_ui_response")
    );
}

#[test]
fn starting_draft_stays_editable_and_shell_focus_handoff_never_overrides_version_or_palette() {
    let mut f = Fixture::new();
    let composer = f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let live = s.sessions.get_mut("a").unwrap();
            live.starting = true;
            live.dirty = true;
            live.controller.set_draft("next draft");
            s.shell_focus.as_ref().unwrap().focus(w, cx);
            s.sync(w, cx);
            assert!(!s.sessions["a"].composer.read(cx).is_disabled());
            assert!(s.shell_focus.as_ref().unwrap().is_focused(w));
            s.sessions["a"].composer.clone()
        })
    });
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.sidebar
                .as_ref()
                .unwrap()
                .update(cx, |sidebar, cx| sidebar.open_version_status(w, cx));
            s.sessions.get_mut("a").unwrap().starting = false;
            s.sessions.get_mut("a").unwrap().dirty = true;
            s.sync(w, cx);
            s.restore_input_focus(cx);
        })
    });
    for _ in 0..3 {
        f.root.update(|_, w, cx| w.simulate_next_frame(cx));
        f.root.draw();
    }
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            assert!(s.sidebar.as_ref().unwrap().read(cx).version_open());
            assert!(
                !composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            );
            assert_eq!(
                composer.read(cx).editor().read(cx).value().as_ref(),
                "next draft"
            );
            s.toggle_palette(w, cx);
            s.sync_chrome_admission(cx);
            assert!(s.palette.is_open(cx));
            assert!(s.sidebar.as_ref().unwrap().read(cx).version_open());
            s.error = Some("An owned foreground operation failed.".into());
            s.sync(w, cx);
            assert!(!s.sidebar.as_ref().unwrap().read(cx).version_open());
            assert!(s.fallback.is_some());
        })
    });
    f.root.draw();
    f.root
        .update(|_, w, _| assert!(w.try_find("dismiss-error").is_none()));
}

#[test]
fn production_restore_is_a_draft_edit_and_modal_admission_is_rechecked() {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.sessions.get_mut("a").unwrap().restored_recovered =
                vec!["  exact\n".into(), "sibling".into()];
            s.sessions.get_mut("a").unwrap().dirty = true;
            s.sync(w, cx);
            let queue = s.sessions["a"].queue.clone();
            let ticket = s.queue_recovery("a", &queue, 0, cx).unwrap();
            s.toggle_palette(w, cx);
            assert!(!s.restore_queue_draft(ticket, w, cx));
            assert_eq!(s.sessions["a"].restored_recovered.len(), 2);
            s.toggle_palette(w, cx);
            s.sync(w, cx);
            let ticket = s.queue_recovery("a", &queue, 0, cx).unwrap();
            let live = s.sessions.get_mut("a").unwrap();
            live.controller.transport_failed(8);
            live.dirty = true;
            s.sync(w, cx);
            assert!(
                s.error.is_none(),
                "the session incident is independently owned"
            );
            assert!(s.fallback.as_ref().unwrap().1.read(cx).is_open());
            assert!(!s.restore_queue_draft(ticket, w, cx));
            assert!(s.queue_recovery("a", &queue, 0, cx).is_none());
            assert_eq!(s.sessions["a"].restored_recovered.len(), 2);
        })
    });
    f.click("close");
    f.sent();
    f.click("recover");
    f.root.update(|_, w, _| {
        let review = w.find("review-panel").bounds();
        assert!(review.top() >= w.find("recover").bounds().bottom());
        assert!(review.bottom() <= w.viewport_size().height);
        assert!(
            w.find("restore").visible(),
            "the owned overlay must be pointer reachable above the composer"
        );
    });
    f.click("restore");
    f.root.update(|_, _, cx| {
        let live = &f.shell.read(cx).sessions["a"];
        assert_eq!(live.controller.state().draft, "  exact\n");
        assert_eq!(live.restored_recovered, ["sibling"]);
    });
    assert!(!f.sent().iter().any(|r| matches!(
        r["type"].as_str(),
        Some("prompt" | "steer" | "follow_up" | "clear_queue")
    )));
}

#[test]
fn duplicate_select_labels_have_distinct_native_ids_and_exact_response() {
    let mut f = Fixture::new();
    f.ingest(json!({"type":"extension_ui_request", "id":"duplicates", "method":"select", "title":"Choose", "options":["same", "same", ""]}));
    f.root.update(|_, w, _| {
        assert!(w.try_find("extension-option-0").is_some());
        assert!(w.try_find("extension-option-1").is_some());
        assert!(w.try_find("extension-option-2").is_some());
    });
    f.click("extension-option-1");
    let requests = f.sent();
    let response = requests
        .iter()
        .find(|r| r["type"] == "extension_ui_response")
        .unwrap();
    assert_eq!(response["id"], "duplicates");
    assert_eq!(response["value"], "same");
    // The controller keeps this request owned until the matching transport completion.
    f.root.update(|_, _, cx| {
        assert!(
            f.shell.read(cx).sessions["a"]
                .controller
                .state()
                .extension_dialogs[0]
                .submitting
        )
    });
}

#[test]
fn editor_prefill_does_not_make_secondary_enter_append_a_newline() {
    let mut f = Fixture::new();
    f.ingest(json!({"type":"extension_ui_request", "id":"editor", "method":"editor", "title":"Edit", "text":"one\ntwo"}));
    f.root.update(|_, w, cx| {
        let dialog = f.shell.read(cx).sessions["a"]
            .extension
            .as_ref()
            .unwrap()
            .1
            .clone();
        dialog.update(cx, |d, cx| d.focus(w, cx));
    });
    f.root.draw();
    f.root.simulate_keystrokes(secondary(false));
    assert!(
        f.sent()
            .iter()
            .any(|r| r["type"] == "extension_ui_response" && r["value"] == "one\ntwo")
    );
}

#[test]
fn transient_compaction_projects_even_without_transcript_projection_dirtiness() {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.ingest(rpc("a", 8, json!({"type":"compaction_start"})), cx);
            s.sessions.get_mut("a").unwrap().projection_dirty = false;
            s.sync(w, cx);
        })
    });
    f.root.draw();
    f.root.update(|_, w, cx| {
        assert!(w.try_find("tau-compacting").is_some());
        assert!(f.shell.read(cx).sessions["a"].controller.state().compacting);
    });
    f.ingest(json!({"type":"compaction_end"}));
    f.root
        .update(|_, w, _| assert!(w.try_find("tau-compacting").is_none()));
}

#[test]
fn width_event_uses_existing_save_snapshot_and_old_ack_cannot_reset_new_width() {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let sidebar = s.sidebar.as_ref().unwrap().clone();
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_width(900., cx);
                cx.emit(components::chrome::SidebarWidthChanged(sidebar.width()));
            });
            s.sync(w, cx);
        })
    });
    f.root
        .update(|_, _, cx| f.shell.update(cx, |s, cx| s.persist(false, cx)));
    let revision = f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.preferences.sidebar_width, Some(480.));
        let (revision, snapshot) = s.pending_saves.last_key_value().unwrap();
        assert_eq!(snapshot.sidebar_width, Some(480.));
        *revision
    });
    f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, cx| {
            s.sidebar.as_ref().unwrap().update(cx, |sidebar, cx| {
                sidebar.set_width(301.6, cx);
                cx.emit(components::chrome::SidebarWidthChanged(sidebar.width()));
            });
        })
    });
    // Entity events are delivered after the update; the width write must precede the old ack.
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            assert_eq!(s.preferences.sidebar_width, Some(302.));
            s.ingest(
                Event::Saved {
                    revision,
                    success: true,
                    stopped: vec![],
                },
                cx,
            );
            s.sync(w, cx);
            assert_eq!(s.preferences.sidebar_width, Some(302.));
            assert_eq!(s.sidebar.as_ref().unwrap().read(cx).width(), 302.);
        })
    });
}

#[test]
fn mounted_header_denies_new_during_picker_and_preserves_retained_editor() {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.picker_busy = true;
            s.sync(w, cx);
        })
    });
    f.click("new-session");
    f.root
        .update(|_, _, cx| assert_eq!(f.shell.read(cx).sessions.len(), 1));
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.action(Action::NewSession, w, cx);
            assert_eq!(s.sessions.len(), 1);
            s.picker_busy = false;
            s.sync(w, cx);
        })
    });
}

#[test]
fn quit_picker_veto_and_matching_save_failure_stay_local_and_stale_save_cannot_release_exit() {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        let owner = f.shell.downgrade();
        let host = NativeHost::install(
            cx,
            "Tau",
            Duration::from_secs(60),
            HostCallbacks {
                lifecycle: LifecycleCallbacks {
                    requested: Rc::new(move |request, cx| {
                        let _ = owner.update(cx, |s, cx| {
                            s.lifecycle = Some(request);
                            s.refresh_window(cx);
                        });
                    }),
                    settled: Rc::new(|_, _, _| {}),
                },
                new_session: Rc::new(|_| {}),
                create_window: Rc::new(|_| Err(anyhow::anyhow!("No additional fixture windows"))),
                reveal_window: Rc::new(|_, _| {}),
                window_failed: Rc::new(|_| {}),
            },
        );
        host.lifecycle().attach(w, cx);
        f.shell.update(cx, |s, cx| {
            s.picker_busy = true;
            s.sync(w, cx);
        });
        cx.quit();
    });
    f.root
        .update(|_, w, cx| f.shell.update(cx, |s, cx| s.sync(w, cx)));
    f.root.draw();
    f.click("confirm");
    let request = f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        let request = s.lifecycle.unwrap();
        assert_eq!(s.quit_error.as_ref().unwrap().0, request);
        assert!(s.error.is_none());
        assert!(s.exit_revision.is_none());
        assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
        request
    });
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.picker_busy = false;
            s.sync(w, cx);
        })
    });
    f.click("confirm");
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let (revision, owner) = s.exit_revision.unwrap();
            assert_eq!(owner, request);
            assert!(s.quit_error.is_none());
            s.ingest(
                Event::Saved {
                    revision: revision.saturating_sub(1),
                    success: false,
                    stopped: vec![],
                },
                cx,
            );
            assert_eq!(s.exit_revision, Some((revision, request)));
            assert!(s.quit_error.is_none());
            s.ingest(
                Event::Saved {
                    revision,
                    success: false,
                    stopped: vec![],
                },
                cx,
            );
            s.sync(w, cx);
            assert!(s.exit_revision.is_none());
            assert_eq!(s.quit_error.as_ref().unwrap().0, request);
            assert!(s.confirmation.is_some());
            assert!(cx.global::<NativeHost>().lifecycle().is_pending(request));
            assert!(s.fallback.is_none());
        })
    });
}
