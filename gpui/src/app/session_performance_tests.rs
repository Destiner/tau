//! Source-authored fake Root regressions; no real Pi, filesystem, SSH or timings.
use super::confirm_queue_fixture::Fixture;
use super::*;
use crate::app::tests::{create, project, rpc, started};
use gpui_kit::{Focusable, MouseButton};
use serde_json::json;

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let live = s.sessions.get_mut("a").unwrap();
            live.controller.set_draft("retained predecessor");
            live.dirty = true;
            s.sync(w, cx);
        })
    });
    f
}

fn settle(f: &mut Fixture) {
    for _ in 0..4 {
        f.root.update(|_, w, cx| w.simulate_next_frame(cx));
        f.root.draw();
    }
}

fn assert_editor(f: &mut Fixture, key: &str) {
    f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.active, key);
        assert!(
            s.sessions[key]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
    });
}

fn complete_bootstrap(f: &mut Fixture, key: &str, generation: u64, model: &str) {
    complete_bootstrap_scoped(f, key, generation, model, "beta");
}

fn complete_bootstrap_scoped(
    f: &mut Fixture,
    key: &str,
    generation: u64,
    model: &str,
    scope: &str,
) {
    complete_discovery(f, key, generation, model, scope, "high", &["off", "high"]);
}

fn complete_discovery(
    f: &mut Fixture,
    key: &str,
    generation: u64,
    model: &str,
    scope: &str,
    effort: &str,
    levels: &[&str],
) {
    for _ in 0..8 {
        let requests: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        if requests.is_empty() {
            break;
        }
        for command in requests {
            f.root.update(|_, w, cx| f.shell.update(cx, |s, cx| {
                match command {
                    Command::Send { runtime, generation: actual, request } => {
                        assert_eq!(runtime, key);
                        assert_eq!(actual, generation);
                        let kind = request["type"].as_str().unwrap();
                        let data = match kind {
                            "get_state" => json!({"model":{"provider":"fixture","id":model,"name":model},"thinkingLevel":effort}),
                            "get_messages" => json!({"messages":[]}),
                            "get_available_models" => json!({"models":[
                                {"provider":"fixture","id":"alpha","name":"Alpha"},
                                {"provider":"fixture","id":"beta","name":"Beta"}]}),
                            "get_scoped_models" => json!({"models":[{"provider":"fixture","id":scope}]}),
                            "get_available_thinking_levels" => json!({"levels":levels}),
                            "get_commands" => json!({"commands":[{"name":"fixture","description":"Fixture","source":"extension"}]}),
                            other => panic!("unexpected bootstrap: {other}"),
                        };
                        s.ingest(rpc(key, generation, json!({"type":"response","id":request["id"],
                            "command":kind,"success":true,"data":data})), cx);
                    }
                    Command::ReadModelScope { runtime, token, .. } => {
                        s.model_scope_result(runtime, token, Some(vec![format!("fixture/{scope}")]), cx);
                    }
                    _ => {}
                }
                s.sync(w, cx);
            }));
        }
    }
}

#[test]
fn cold_local_and_remote_new_sessions_focus_drafts_before_start_and_bootstrap() {
    for remote in [false, true] {
        let mut f = fixture();
        let key = f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                if remote {
                    s.workspace.as_mut().unwrap().projects[0].connection_string =
                        Some("fixture@host".into());
                }
                s.new_session(Some(project().path), w, cx);
                s.active.clone()
            })
        });
        settle(&mut f);
        assert_editor(&mut f, &key);
        f.root.simulate_input("  cold 🦀\nexact  ");
        f.press("enter");
        f.root.update(|_, _, cx| {
            let s = f.shell.read(cx);
            let live = &s.sessions[&key];
            assert!(live.starting);
            assert!(!live.controller.state().ready);
            assert_eq!(live.controller.state().draft, "  cold 🦀\nexact  ");
            assert_eq!(live.controller.generation(), 0);
        });
        let commands: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        assert_eq!(
            commands
                .iter()
                .filter(|c| matches!(c, Command::Start { runtime, .. } if runtime == &key))
                .count(),
            1
        );
        assert!(!commands.iter().any(|c| matches!(c, Command::Send { .. })));
        // Start alone still cannot authorize Send, and must not replace the editor.
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                started(s, &key, 20, cx);
                s.sync(w, cx);
            })
        });
        settle(&mut f);
        assert_editor(&mut f, &key);
        f.press("enter");
        let held: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        assert!(!held.iter().any(|c| matches!(c, Command::Send { request, .. }
            if matches!(request["type"].as_str(), Some("prompt" | "set_model" | "set_thinking_level")))));
    }
}

#[test]
fn cold_saved_navigation_drops_stale_handoffs_and_ready_does_not_steal_pointer_focus() {
    for remote in [false, true] {
        let mut f = fixture();
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                let p = &mut s.workspace.as_mut().unwrap().projects[0];
                if remote {
                    p.connection_string = Some("fixture@host".into());
                }
                p.sessions = ["saved-one", "saved-two"]
                    .into_iter()
                    .map(|id| {
                        serde_json::from_value(
                            json!({"id":id,"path":format!("/fixture/{id}.jsonl"),
                    "title":id,"lastActive":"now","sortAt":1,"lastUserMessageAt":0,
                    "archived":false,"selected":false}),
                        )
                        .unwrap()
                    })
                    .collect();
                s.select("saved-one", w, cx);
                s.select("saved-two", w, cx);
            })
        });
        let key = f.root.update(|_, _, cx| f.shell.read(cx).active.clone());
        settle(&mut f);
        assert_editor(&mut f, &key);
        f.root.simulate_input("  saved draft 🌍  ");
        f.root.draw();
        // A real pointer focus owner supersedes both navigation and later readiness.
        let target = f.root.update(|_, w, _| {
            use gpui_kit::test::TestWindowExt;
            w.find("sidebar-resize-handle").bounds().center()
        });
        f.root.simulate_click(target, MouseButton::Left);
        let focused = f.root.update(|_, w, cx| w.focused(cx));
        // Ignore held Start commands: only this runtime's own fresh bootstrap may win.
        while f.requests.try_recv().is_ok() {}
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                started(s, &key, 21, cx);
                s.sync(w, cx);
            })
        });
        complete_bootstrap(&mut f, &key, 21, "beta");
        settle(&mut f);
        f.root.update(|_, w, cx| {
            let s = f.shell.read(cx);
            assert_eq!(w.focused(cx), focused);
            assert_eq!(
                s.sessions[&key].controller.state().draft,
                "  saved draft 🌍  "
            );
            assert!(s.sessions[&key].controller.state().ready);
        });
    }
}

#[test]
fn lookup_work_is_linear_and_launch_metadata_does_not_copy_saved_rows() {
    let mut f = fixture();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            for i in 0..8 {
                create(s, &format!("retained-{i}"), "retained", w, cx);
            }
            s.workspace.as_mut().unwrap().projects[0].sessions = (0..1000)
                .map(|i| {
                    serde_json::from_value(
                        json!({"id":format!("row-{i}"),"path":format!("/fixture/{i}.jsonl"),
                "title":"Saved","lastActive":"now","sortAt":1,"lastUserMessageAt":0,
                "archived":false,"selected":false}),
                    )
                    .unwrap()
                })
                .collect();
            s.session_actions.lookup_runtime_visits.set(0);
            s.session_actions.lookup_saved_visits.set(0);
            let found = s.saved_session("row-999").unwrap();
            assert_eq!(found.1.id, "row-999");
            assert!(found.0.sessions.is_empty());
            assert_eq!(
                s.session_actions.lookup_runtime_visits.get(),
                s.sessions.len()
            );
            assert_eq!(s.session_actions.lookup_saved_visits.get(), 1000);
            assert!(s.selected_project().unwrap().sessions.is_empty());
        })
    });
}

#[test]
fn clean_sync_skips_retained_history_visits_and_composer_notifications() {
    let mut f = fixture();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            for timestamp in 1..=100 {
                s.ingest(
                    rpc(
                        "a",
                        8,
                        json!({"type":"message_end","message":{"role":"assistant",
                "content":[{"type":"text","text":"retained history"}],"timestamp":timestamp}}),
                    ),
                    cx,
                );
            }
            create(s, "foreground", "draft", w, cx);
            s.sync(w, cx);
            for live in s.sessions.values_mut() {
                live.file_source_visits = 0;
            }
        })
    });
    let notices = Rc::new(std::cell::Cell::new(0));
    let observer = f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, cx| {
            let composer = s.sessions["a"].composer.clone();
            let notices = notices.clone();
            cx.observe(&composer, move |_, _, _| notices.set(notices.get() + 1))
        })
    });
    for _ in 0..4 {
        f.root
            .update(|_, w, cx| f.shell.update(cx, |s, cx| s.sync(w, cx)));
        f.root.draw();
    }
    f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.sessions["a"].file_source_visits, 0);
        assert!(!s.sessions["a"].controller.state().transcript.is_empty());
    });
    assert_eq!(notices.get(), 0);
    // Background events still update their own derived permissions and transcript.
    f.ingest(json!({"type":"message_end","message":{"role":"assistant",
        "content":[{"type":"text","text":"background update"}],"timestamp":101}}));
    f.root.update(|_, _, cx| {
        let s = f.shell.read(cx);
        assert_eq!(s.active, "foreground");
        assert!(s.sessions["a"].file_source_visits > 0);
        assert_eq!(s.sessions["foreground"].file_source_visits, 0);
    });
    drop(observer);
}

#[test]
fn new_session_inherits_scoped_presentation_not_runtime_authority() {
    let mut f = fixture();
    // Populate a separate source using its actual bootstrap, not state mutation.
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            create(s, "source", "source draft", w, cx);
            started(s, "source", 30, cx);
        })
    });
    complete_bootstrap(&mut f, "source", 30, "beta");
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            assert!(
                s.sessions["source"]
                    .controller
                    .settings_presentation()
                    .is_some()
            );
            s.new_session(None, w, cx);
            let key = s.active.clone();
            let live = &s.sessions[&key];
            assert!(live.starting);
            assert!(!live.controller.state().ready);
            assert!(live.controller.state().identity.id.is_empty());
            assert!(live.controller.state().models.is_empty());
            assert_eq!(live.composer.read(cx).visible_models().len(), 1);
            assert_eq!(
                live.composer.read(cx).model().unwrap().id,
                "fixture\u{1f}beta"
            );
            assert!(live.inherited_commands.is_some());
            key
        })
    });
    // A fresh differing selection wins even when it is outside the cached scope.
    let key = f.root.update(|_, _, cx| f.shell.read(cx).active.clone());
    while f.requests.try_recv().is_ok() {}
    f.root
        .update(|_, _, cx| f.shell.update(cx, |s, cx| started(s, &key, 31, cx)));
    complete_bootstrap(&mut f, &key, 31, "alpha");
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let live = &s.sessions[&key];
            assert!(live.inherited_choices.is_none());
            assert!(live.inherited_commands.is_none());
            assert_eq!(live.controller.state().model["id"], "alpha");
            assert!(live.composer.read(cx).model().is_none());
            // A different transport, directory or project may not borrow that snapshot.
            s.active = "source".into();
            let mut other = project();
            other.connection_string = Some("other@host".into());
            s.create_live(
                "remote".into(),
                other,
                String::new(),
                String::new(),
                String::new(),
                w,
                cx,
            );
            assert!(s.sessions["remote"].inherited_choices.is_none());
            assert!(s.sessions["remote"].inherited_commands.is_none());
        })
    });
}

#[test]
fn timed_out_settings_never_seed_a_held_new_session() {
    for (change_model, read_kind) in [
        (false, 0),
        (true, 0),
        (false, 1),
        (true, 1),
        (false, 2),
        (true, 2),
    ] {
        let mut f = fixture();
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                create(s, "source", "source draft", w, cx);
                started(s, "source", 50, cx);
            })
        });
        complete_bootstrap_scoped(&mut f, "source", 50, "alpha", "alpha");
        f.root.update(|_, _, cx| {
            f.shell.update(cx, |s, _| {
                assert!(
                    s.sessions
                        .get_mut("source")
                        .unwrap()
                        .controller
                        .tick(1_000_000)
                        .is_empty()
                );
            })
        });
        if read_kind == 2 {
            f.root.update(|_, _, cx| {
                f.shell.update(cx, |s, cx| {
                    let effects = s
                        .sessions
                        .get_mut("source")
                        .unwrap()
                        .controller
                        .handle(json!({"type":"agent_settled"}));
                    s.effects("source", effects, cx);
                })
            });
            complete_bootstrap_scoped(&mut f, "source", 50, "alpha", "alpha");
        }
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                let c = &mut s.sessions.get_mut("source").unwrap().controller;
                assert!(c.state().ready);
                assert!(!c.state().history_loading);
                assert_eq!(c.pending_count(), 0);
                assert_eq!(c.settings_presentation().unwrap().model["id"], "alpha");
                let effects = if change_model {
                    c.set_model("fixture", "beta")
                } else {
                    c.set_thinking("medium")
                };
                s.effects("source", effects, cx);
                s.sync(w, cx);
            })
        });
        let writes: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        let writes: Vec<_> = writes
            .iter()
            .filter_map(|command| match command {
                Command::Send {
                    runtime,
                    generation,
                    request,
                } => {
                    assert_eq!(runtime, "source");
                    assert_eq!(*generation, 50);
                    assert_eq!(
                        request["type"],
                        if change_model {
                            "set_model"
                        } else {
                            "set_thinking_level"
                        }
                    );
                    Some(request.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(writes.len(), 1);
        let write = &writes[0];
        let cold = f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                let c = &mut s.sessions.get_mut("source").unwrap().controller;
                assert!(c.request_write_accepted(50, write["id"].as_str().unwrap()));
                assert!(c.settings_presentation().is_none());
                let old_reads = match read_kind {
                    0 => vec![],
                    1 => {
                        assert!(c.tick(1_003_200).is_empty());
                        c.handle(json!({"type":"session_info_changed"}))
                    },
                    2 => c.tick(1_003_200),
                    _ => unreachable!(),
                };
                let old_reads: Vec<_> = old_reads.into_iter().filter_map(|effect| match effect {
                    tau_session::session::Effect::Send(request) => {
                        assert_eq!(request["type"], "get_state");
                        Some(request)
                    }
                    _ => None,
                }).collect();
                assert_eq!(old_reads.is_empty(), read_kind == 0);
                assert!(c.tick(1_010_000).is_empty());
                assert!(c.state().ready);
                assert!(!c.state().history_loading);
                assert!(!c.state().working);
                assert!(!c.setting_pending());
                assert_eq!(c.pending_count(), old_reads.len());
                for request in old_reads {
                    let follow = c.handle(json!({"type":"response","id":request["id"],"command":"get_state","success":true,
                        "data":{"model":{"provider":"fixture","id":"alpha"},"thinkingLevel":"high"}}));
                    assert!(c.settings_presentation().is_none());
                    for effect in follow {
                        if let tau_session::session::Effect::Send(request) = effect {
                            let data = match request["type"].as_str().unwrap() {
                                "get_available_thinking_levels" => json!({"levels":["off","high"]}),
                                "get_messages" => json!({"messages":[]}),
                                other => panic!("unexpected old-state follow-up: {other}"),
                            };
                            c.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}));
                        }
                    }
                    assert!(c.settings_presentation().is_none());
                }
                assert!(c.state().ready);
                assert!(!c.state().history_loading);
                assert_eq!(c.pending_count(), 0);
                assert_eq!(c.state().model["id"], "alpha");
                assert_eq!(c.state().effort, "high");
                assert!(c.settings_presentation().is_none());
                let token = c.error_token().unwrap();
                assert!(c.acknowledge_error(&token));
                c.set_draft("still editable");
                s.active = "source".into();
                s.new_session(None, w, cx);
                let cold = s.active.clone();
                let live = &s.sessions[&cold];
                assert!(live.inherited_choices.is_none());
                assert!(live.composer.read(cx).visible_models().is_empty());
                assert!(live.composer.read(cx).effort().is_none());
                cold
            })
        });
        let starts: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        assert_eq!(
            starts
                .iter()
                .filter(|c| matches!(c, Command::Start { runtime, .. } if runtime == &cold))
                .count(),
            1
        );
        // Never acknowledge either destination's Start; only the source is refreshed.
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                s.ingest(
                    rpc(
                        "source",
                        50,
                        json!({"type":"response","id":write["id"],
                    "command":write["type"],"success":true,"data":{}}),
                    ),
                    cx,
                );
                assert!(
                    s.sessions["source"]
                        .controller
                        .settings_presentation()
                        .is_none()
                );
                let effects = s.sessions.get_mut("source").unwrap().controller.refresh();
                s.effects("source", effects, cx);
                s.sync(w, cx);
            })
        });
        complete_discovery(
            &mut f,
            "source",
            50,
            "beta",
            "beta",
            "medium",
            &["low", "medium"],
        );
        f.root.update(|_, w, cx| {
            f.shell.update(cx, |s, cx| {
                assert!(s.sessions[&cold].inherited_choices.is_none());
                assert!(s.sessions[&cold].composer.read(cx).effort().is_none());
                s.active = "source".into();
                s.new_session(None, w, cx);
                let beta = s.active.clone();
                let snapshot = s.sessions[&beta].inherited_choices.as_ref().unwrap();
                assert_eq!(snapshot.model["id"], "beta");
                assert_eq!(snapshot.models.len(), 1);
                assert_eq!(snapshot.models[0]["id"], "beta");
                assert_eq!(snapshot.effort, "medium");
                assert_eq!(snapshot.efforts, vec!["low", "medium"]);
                let choices = s.sessions[&beta].composer.read(cx).visible_models();
                assert_eq!(choices.len(), 1);
                assert_eq!(choices[0].id, "fixture\u{1f}beta");
                assert_eq!(
                    s.sessions[&beta].composer.read(cx).effort().unwrap().id,
                    "medium"
                );
                for key in [&cold, &beta] {
                    let live = &s.sessions[key];
                    assert!(live.starting);
                    assert_eq!(live.controller.generation(), 0);
                    assert!(!live.controller.state().ready);
                    assert!(live.controller.state().identity.id.is_empty());
                    assert!(live.controller.state().models.is_empty());
                    s.composer_event(key, &ComposerEvent::Send("held draft".into()), cx);
                    s.composer_event(
                        key,
                        &ComposerEvent::ModelChanged("fixture\u{1f}beta".into()),
                        cx,
                    );
                    s.composer_event(key, &ComposerEvent::EffortChanged("medium".into()), cx);
                }
            })
        });
        let commands: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        assert!(!commands.iter().any(|c| matches!(c, Command::Send { request, .. }
            if matches!(request["type"].as_str(), Some("prompt" | "set_model" | "set_thinking_level")))));
    }
}

#[test]
fn new_session_during_scope_refresh_never_freezes_previous_runtime_scope() {
    let mut f = fixture();
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            create(s, "source", "source draft", w, cx);
            started(s, "source", 40, cx);
        })
    });
    complete_bootstrap_scoped(&mut f, "source", 40, "alpha", "alpha");
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            let snapshot = s.sessions["source"]
                .controller
                .settings_presentation()
                .unwrap();
            assert_eq!(snapshot.models[0]["id"], "alpha");
            let effects = s.sessions.get_mut("source").unwrap().controller.refresh();
            s.effects("source", effects, cx);
            s.sync(w, cx);
        })
    });
    let mut held_scope = None;
    // Complete Beta state, catalogue and host scope, holding only its runtime scope probe.
    for _ in 0..8 {
        let commands: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
        if commands.is_empty() {
            break;
        }
        for command in commands {
            f.root.update(|_, w, cx| f.shell.update(cx, |s, cx| {
                match command {
                    Command::Send { runtime, generation, request } => {
                        assert_eq!(runtime, "source");
                        assert_eq!(generation, 40);
                        let kind = request["type"].as_str().unwrap();
                        if kind == "get_scoped_models" {
                            held_scope = Some(request);
                        } else {
                            let data = match kind {
                                "get_state" => json!({"model":{"provider":"fixture","id":"beta","name":"Beta"},"thinkingLevel":"high"}),
                                "get_messages" => json!({"messages":[]}),
                                "get_available_models" => json!({"models":[
                                    {"provider":"fixture","id":"alpha","name":"Alpha"},
                                    {"provider":"fixture","id":"beta","name":"Beta"}]}),
                                "get_available_thinking_levels" => json!({"levels":["off","high"]}),
                                other => panic!("unexpected refresh: {other}"),
                            };
                            s.ingest(rpc("source", 40, json!({"type":"response","id":request["id"],
                                "command":kind,"success":true,"data":data})), cx);
                        }
                    }
                    Command::ReadModelScope { runtime, token, .. } => {
                        assert_eq!(runtime, "source");
                        s.model_scope_result(runtime, token, Some(vec!["fixture/beta".into()]), cx);
                    }
                    _ => {}
                }
                s.sync(w, cx);
            }));
        }
    }
    let held_scope = held_scope.expect("held runtime scope probe");
    let cold = f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            assert!(
                s.sessions["source"]
                    .controller
                    .settings_presentation()
                    .is_none()
            );
            s.new_session(None, w, cx);
            let key = s.active.clone();
            let live = &s.sessions[&key];
            assert!(live.inherited_choices.is_none());
            assert!(live.composer.read(cx).visible_models().is_empty());
            key
        })
    });
    // The destination Start is deliberately never acknowledged.
    let starts: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
    assert_eq!(
        starts
            .iter()
            .filter(|c| matches!(c, Command::Start { runtime, .. } if runtime == &cold))
            .count(),
        1
    );
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.ingest(
                rpc(
                    "source",
                    40,
                    json!({"type":"response","id":held_scope["id"],
            "command":"get_scoped_models","success":true,
            "data":{"models":[{"provider":"fixture","id":"beta"}]}}),
                ),
                cx,
            );
            s.sync(w, cx);
            assert!(s.sessions[&cold].inherited_choices.is_none());
            assert!(
                s.sessions[&cold]
                    .composer
                    .read(cx)
                    .visible_models()
                    .is_empty()
            );
            s.active = "source".into();
            s.new_session(None, w, cx);
            let beta = s.active.clone();
            let choices = s.sessions[&beta].composer.read(cx).visible_models();
            assert_eq!(choices.len(), 1);
            assert_eq!(choices[0].id, "fixture\u{1f}beta");
            for key in [&cold, &beta] {
                let live = &s.sessions[key];
                assert!(live.starting);
                assert_eq!(live.controller.generation(), 0);
                assert!(!live.controller.state().ready);
                assert!(live.controller.state().identity.id.is_empty());
                assert!(live.controller.state().models.is_empty());
                s.composer_event(key, &ComposerEvent::Send("held draft".into()), cx);
                s.composer_event(
                    key,
                    &ComposerEvent::ModelChanged("fixture\u{1f}beta".into()),
                    cx,
                );
                s.composer_event(key, &ComposerEvent::EffortChanged("high".into()), cx);
            }
        })
    });
    let commands: Vec<_> = std::iter::from_fn(|| f.requests.try_recv().ok()).collect();
    assert!(!commands.iter().any(|c| matches!(c, Command::Send { request, .. }
        if matches!(request["type"].as_str(), Some("prompt" | "set_model" | "set_thinking_level")))));
}
