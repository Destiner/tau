//! Opt-in acceptance against the production shell, worker, renderer and installed Pi.
use super::*;
use gpui_kit::{
    AsyncApp, EntityInputHandler, Focusable, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput,
};
use std::cell::Cell;

pub const COMPLETE: &str = "PASS native-app completion-gated quit";

pub fn run_native_smoke(root: PathBuf, project: String, check_menus: fn()) -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("FAIL native-app assertion (no transcript or raw error logged)");
        std::process::exit(1);
    }));
    std::thread::spawn(|| {
        // Reserve four seconds for the parent's bounded process-group cleanup.
        std::thread::sleep(Duration::from_secs(96));
        eprintln!("FAIL native-app watchdog");
        std::process::exit(2);
    });
    let complete = Rc::new(Cell::new(false));
    let observed = complete.clone();
    run_with(
        StorageProfile::isolated(root.join("tau"), root.join("pi-sessions")),
        Some(project),
        None,
        move |shell, cx| {
            let quit = complete.clone();
            cx.on_app_quit(move |_| {
                assert!(quit.get(), "premature native exit");
                eprintln!("{COMPLETE}");
                std::future::ready(())
            })
            .detach();
            cx.spawn(async move |cx| {
                exercise(shell, root, check_menus, complete, cx.clone()).await
            })
            .detach();
        },
    )?;
    anyhow::ensure!(
        observed.get(),
        "application returned before acceptance completed"
    );
    Ok(())
}

async fn poll(
    cx: &AsyncApp,
    label: &str,
    seconds: u64,
    mut predicate: impl FnMut(&mut App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if cx.update(|cx| predicate(cx)) {
            return;
        }
        if Instant::now() >= deadline {
            eprintln!("FAIL native-app phase: {label} ({seconds}s)");
            panic!("phase deadline");
        }
        cx.background_executor()
            .timer(Duration::from_millis(15))
            .await;
    }
}
fn window(shell: &Entity<Shell>, cx: &App) -> AnyWindowHandle {
    shell.read(cx).window.unwrap()
}
fn key(shell: &Entity<Shell>, cx: &AsyncApp, binding: &str) {
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                w.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke: Keystroke::parse(binding).unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            })
            .unwrap()
    });
}
fn focus_composer(shell: &Entity<Shell>, cx: &AsyncApp) {
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                shell.update(cx, |s, cx| s.action(Action::FocusComposer, w, cx));
            })
            .unwrap()
    });
}
fn insert_composer(shell: &Entity<Shell>, cx: &AsyncApp, text: &str) {
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                let editor = s.sessions[&s.active].composer.read(cx).editor();
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |s, cx| s.replace_text_in_range(None, text, w, cx));
            })
            .unwrap()
    });
}
async fn click(shell: &Entity<Shell>, cx: &AsyncApp, id: &str) {
    let label = if id.starts_with("session-status-") {
        "painted sidebar session"
    } else {
        id
    };
    let mut previous = None;
    poll(cx, label, 5, |cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let control = gpui_kit::base::test_support::find(w, &[], &id.to_string().into());
                // Row actions are revealed by real hover, not eligibility overrides.
                if let Some(control) = control.as_ref()
                    && !control.visible()
                    && (id.starts_with("archive-") || id.starts_with("restore-"))
                {
                    w.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: control.bounds().center(),
                            pressed_button: None,
                            modifiers: Default::default(),
                        }),
                        cx,
                    );
                }
                let status = (
                    w.is_window_active(),
                    control.is_some(),
                    control.as_ref().is_some_and(|s| s.visible()),
                );
                if previous != Some(status) {
                    eprintln!("OBSERVED {label} active/found/visible={status:?}");
                    previous = Some(status);
                }
                status.2
            })
            .unwrap()
    })
    .await;
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let bounds = gpui_kit::base::test_support::find(w, &[], &id.to_string().into())
                    .unwrap()
                    .bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                let position = bounds.center();
                w.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }),
                    cx,
                );
                w.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                w.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: Default::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            })
            .unwrap()
    });
}
fn registry_exists(live: &LiveSession) -> bool {
    tau_core::storage::default_session_dir(&live.project.path)
        .ok()
        .zip(profile::session_registry_filename().ok())
        .is_some_and(|(directory, filename)| directory.join(filename).is_file())
}
fn registry_record(live: &LiveSession) -> Option<serde_json::Value> {
    let path = tau_core::storage::default_session_dir(&live.project.path)
        .ok()?
        .join(profile::session_registry_filename().ok()?);
    let registry: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    registry["sessions"]
        .as_array()?
        .iter()
        .find(|record| record["id"] == live.controller.state().identity.id)
        .cloned()
}

async fn capture(shell: &Entity<Shell>, cx: &AsyncApp, name: &str, empty: bool) {
    let painted = Rc::new(Cell::new(false));
    let observed = painted.clone();
    let name = name.to_owned();
    cx.update(|cx| {
        window(shell, cx).update(cx, |_, w, _| {
            // GPUI drains frame callbacks before drawing. A second callback
            // observes the scene painted by the first normal platform frame.
            w.on_next_frame(move |w, _| w.on_next_frame(move |w, _| {
                let bounds = |id: &str| gpui_kit::base::test_support::find(w, &[], &id.to_owned().into()).unwrap().bounds();
                let area = bounds("shell-composer-area");
                let editor = bounds("editor");
                let footer = bounds("footer-gap");
                assert_eq!(area.bottom(), w.viewport_size().height);
                assert_eq!(area.bottom() - footer.bottom(), px(8.));
                assert_eq!(footer.left() - area.left(), px(6.));
                assert_eq!(area.right() - footer.right(), px(8.));
                if empty {
                    assert!(gpui_kit::base::test_support::find(w, &[], &"shell-transcript-slot".into()).is_none());
                    assert_eq!(editor.top(), area.top() + px(6.));
                    assert_eq!(editor.bottom(), footer.top());
                    assert!(editor.size.height > w.viewport_size().height * 0.75);
                } else {
                    let slot = bounds("shell-transcript-slot");
                    assert!(slot.size.height > px(200.));
                    assert!(slot.bottom() <= area.top());
                    assert_eq!(editor.size.height, px(83.));
                }
                eprintln!("PASS native-app {name} actual frame composer/editor/footer bounds={area:?}/{editor:?}/{footer:?}");
                if let Some(directory) = std::env::var_os("TAU_NATIVE_CAPTURE_DIR") {
                    let destination = PathBuf::from(directory).join(format!("{name}.png"));
                    let image = w.render_to_image().unwrap_or_else(|_| {
                        eprintln!("FAIL native-app owned Metal scene capture unsupported or renderer failed");
                        panic!("native capture failed")
                    });
                    let (width, height) = image.dimensions();
                    image.save(&destination).expect("save actual renderer PNG");
                    eprintln!("PASS native-app owned Metal scene PNG {width}x{height}: {}", destination.display());
                }
                observed.set(true);
            }));
        }).unwrap();
    });
    poll(cx, "normal native painted frame and capture", 5, |_| {
        painted.get()
    })
    .await;
}

fn insert_rename(shell: &Entity<Shell>, cx: &AsyncApp, text: &str) {
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let editor = shell.read(cx).header.as_ref().unwrap().read(cx).editor();
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |input, cx| {
                    input.replace_text_in_range(None, text, w, cx)
                });
            })
            .unwrap();
    });
}

async fn ready(shell: &Entity<Shell>, cx: &AsyncApp) {
    poll(cx, "real Pi bootstrap", 12, |cx| {
        let s = shell.read(cx);
        assert!(s.error.is_none());
        s.sessions.get(&s.active).is_some_and(|l| {
            let state = l.controller.state();
            assert!(state.error.is_none());
            !l.starting
                && state.ready
                && !state.history_loading
                && l.controller.pending_count() == 0
                && state
                    .commands
                    .iter()
                    .any(|c| c["name"] == "tau-native-smoke")
        })
    })
    .await;
}
async fn exercise(
    shell: Entity<Shell>,
    root: PathBuf,
    check_menus: fn(),
    complete: Rc<Cell<bool>>,
    cx: AsyncApp,
) {
    poll(&cx, "workspace and own active native window", 5, |cx| {
        let s = shell.read(cx);
        s.loaded
            && s.workspace.as_ref().is_some_and(|w| !w.projects.is_empty())
            && s.window
                .is_some_and(|target| target.update(cx, |_, w, _| w.is_window_active()).unwrap())
    })
    .await;
    cx.update(|cx| cx.dispatch_action(&crate::native::NewSession));
    ready(&shell, &cx).await;
    let first = cx.update(|cx| shell.read(cx).active.clone());
    capture(&shell, &cx, "empty", true).await;
    focus_composer(&shell, &cx);
    insert_composer(&shell, &cx, "Owned draft café 😀");
    poll(&cx, "draft delivered through real editor", 5, |cx| {
        shell.read(cx).sessions[&first].controller.state().draft == "Owned draft café 😀"
    })
    .await;
    cx.update(|cx| cx.dispatch_action(&crate::native::NewSession));
    ready(&shell, &cx).await;
    let second = cx.update(|cx| shell.read(cx).active.clone());
    assert_ne!(first, second);
    cx.update(|cx| {
        let s = shell.read(cx);
        eprintln!(
            "OBSERVED initial sidebar first/second/draft/projects/collapsed={:?}",
            (
                s.sessions.contains_key(&first),
                s.sessions.contains_key(&second),
                s.sessions
                    .get(&first)
                    .is_some_and(|l| l.controller.state().draft == "Owned draft café 😀"),
                s.workspace.as_ref().map(|w| w.projects.len()),
                s.workspace
                    .as_ref()
                    .is_some_and(|w| w.projects.iter().any(|p| p.collapsed)),
            )
        );
    });
    click(&shell, &cx, &format!("session-status-{first}")).await;
    poll(&cx, "sidebar selects first retained draft", 5, |cx| {
        let s = shell.read(cx);
        s.active == first
            && s.sessions[&first]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .as_ref()
                == "Owned draft café 😀"
    })
    .await;
    click(&shell, &cx, &format!("session-status-{second}")).await;
    poll(&cx, "sidebar selects second isolated draft", 5, |cx| {
        shell.read(cx).active == second
    })
    .await;
    eprintln!("PASS native-app NewSession + real sidebar switches retain editor draft");
    check_menus();
    eprintln!("PASS native-app native menu omits Settings");
    focus_composer(&shell, &cx);
    insert_composer(&shell, &cx, "/tau-native-smoke");
    key(&shell, &cx, "enter");
    poll(&cx, "real Pi extension input", 12, |cx| {
        shell.read(cx).sessions[&second].extension.is_some()
    })
    .await;
    poll(&cx, "initial real extension editor focus", 5, |cx| {
        window(&shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                s.sessions[&second]
                    .extension
                    .as_ref()
                    .unwrap()
                    .1
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            })
            .unwrap()
    })
    .await;
    cx.update(|cx| {
        window(&shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                let editor = s.sessions[&second]
                    .extension
                    .as_ref()
                    .unwrap()
                    .1
                    .read(cx)
                    .editor();
                editor.update(cx, |s, cx| {
                    s.replace_text_in_range(None, "invented reply", w, cx)
                });
            })
            .unwrap()
    });
    key(&shell, &cx, "enter");
    poll(
        &cx,
        "real Pi custom output and ephemeral identity",
        12,
        |cx| {
            let s = shell.read(cx);
            let state = s.sessions[&second].controller.state();
            assert!(s.error.is_none() && state.error.is_none());
            state.extension_dialogs.is_empty()
                && state.identity.name == "Tau invented smoke"
                && state
                    .transcript
                    .iter()
                    .any(|e| e.text == "Invented custom output")
                && !state.durable
                && !PathBuf::from(&state.identity.path).is_file()
                && s.workspace.as_ref().is_some_and(|w| {
                    w.projects.iter().all(|p| {
                        p.sessions
                            .iter()
                            .all(|session| session.id != state.identity.id)
                    })
                })
        },
    )
    .await;
    eprintln!(
        "PASS native-app real Pi command -> native input/Enter -> custom output/name; unsaved command identity stays ephemeral (not assistant)"
    );
    if std::env::var("TAU_NATIVE_MODEL_SMOKE").as_deref() == Ok("1") {
        ready(&shell, &cx).await;
        focus_composer(&shell, &cx);
        insert_composer(
            &shell,
            &cx,
            "Reply exactly TAU_NATIVE_OK. Do not use tools.",
        );
        key(&shell, &cx, "enter");
        let mut previous = None;
        poll(&cx, "real provider assistant completion", 60, |cx| {
            let s = shell.read(cx);
            let state = s.sessions[&second].controller.state();
            if state.error.is_some() { eprintln!("FAIL native-app model: Pi/provider could not complete the request; check existing Pi authentication/model availability"); panic!("model request failed"); }
            let records = std::fs::read_to_string(root.join("events.jsonl")).unwrap_or_default();
            let gates = [
                records.lines().any(|s| s == "message_update:text_delta"),
                records.lines().any(|s| s == "message_end:assistant"),
                state.transcript.iter().any(|e| e.kind == "assistant" && e.text.trim() == "TAU_NATIVE_OK"),
                !state.working, state.durable,
                PathBuf::from(&state.identity.path).is_file(),
                registry_exists(&s.sessions[&second]),
            ];
            if previous != Some(gates) {
                eprintln!("OBSERVED model stream/end/text/idle/durable/file/registry={gates:?}");
                previous = Some(gates);
            }
            assert!(!state.transcript.iter().any(|e| e.kind == "error"), "assistant error");
            gates.into_iter().all(|passed| passed)
        }).await;
        eprintln!(
            "PASS native-app real model message_update + assistant message_end + durable assistant transcript"
        );
        let (queue, journal_offset) = cx.update(|cx| {
            let s = shell.read(cx);
            (
                s.sessions[&second].controller.state().queue.clone(),
                std::fs::read_to_string(root.join("events.jsonl"))
                    .unwrap()
                    .len(),
            )
        });
        focus_composer(&shell, &cx);
        insert_composer(
            &shell,
            &cx,
            "Write exactly TAU_NATIVE_TOOL_OK as assistant text, then call tau_native_wait exactly once with {}. This is an invented cancellation acceptance fixture; wait for its result. Do not call any other tool.",
        );
        key(&shell, &cx, "enter");
        poll(
            &cx,
            "real model tool start and partial while busy",
            30,
            |cx| {
                let s = shell.read(cx);
                let live = &s.sessions[&second];
                let state = live.controller.state();
                assert!(s.error.is_none() && state.error.is_none());
                let records =
                    std::fs::read_to_string(root.join("events.jsonl")).unwrap_or_default();
                let records = records.get(journal_offset..).unwrap_or_default();
                assert!(!records.lines().any(|s| s == "protocol_or_extension_error"));
                state.working
                    && live.composer.read(cx).is_streaming()
                    && live.composer.read(cx).editor().read(cx).value().is_empty()
                    && records.lines().any(|s| s == "tool_execution_start:fixture")
                    && records.lines().any(|s| s == "tool_execution_update:ready")
                    && state
                        .transcript
                        .iter()
                        .any(|e| e.kind == "assistant" && e.text.trim() == "TAU_NATIVE_TOOL_OK")
                    && state.transcript.iter().any(|e| {
                        e.tool_name.as_deref() == Some("tau_native_wait")
                            && e.activity
                            && e.result == "TAU_NATIVE_PARTIAL"
                    })
            },
        )
        .await;
        let tool_id = cx.update(|cx| {
            let s = shell.read(cx);
            let tools: Vec<_> = s.sessions[&second]
                .controller
                .state()
                .transcript
                .iter()
                .filter(|e| e.tool_name.as_deref() == Some("tau_native_wait"))
                .collect();
            assert_eq!(tools.len(), 1);
            tools[0].id.clone()
        });
        let stops = Rc::new(Cell::new(0));
        let observed = stops.clone();
        let _stop_observer = cx.update(|cx| {
            let composer = shell.read(cx).sessions[&second].composer.clone();
            cx.subscribe(&composer, move |_, event: &ComposerEvent, _| {
                if matches!(event, ComposerEvent::Stop) {
                    observed.set(observed.get() + 1);
                    eprintln!("OBSERVED native rendered composer Stop event");
                }
            })
        });
        eprintln!(
            "PASS native-app real model tool execution start/update and live partial row while busy"
        );
        // Empty, streaming composer renders Stop at this existing control ID.
        click(&shell, &cx, "send").await;
        let mut last_stop_invariants = None;
        poll(
            &cx,
            "native Stop abort response and real tool settlement",
            10,
            |cx| {
                let s = shell.read(cx);
                let live = &s.sessions[&second];
                let state = live.controller.state();
                let invariants = [
                    s.error.is_none(),
                    state.error.is_none(),
                    state.queue == queue,
                    s.sessions[&first].controller.state().draft == "Owned draft café 😀",
                    !state.transcript.iter().any(|e| e.kind == "error"),
                ];
                if last_stop_invariants != Some(invariants) {
                    eprintln!("OBSERVED Stop shell/session/queue/draft/no-error={invariants:?}");
                    last_stop_invariants = Some(invariants);
                }
                assert!(invariants.into_iter().all(|ok| ok));
                let records =
                    std::fs::read_to_string(root.join("events.jsonl")).unwrap_or_default();
                let records = records.get(journal_offset..).unwrap_or_default();
                assert!(!records.lines().any(|s| s == "protocol_or_extension_error"));
                let markers: Vec<_> = records.lines().collect();
                let ended = markers
                    .iter()
                    .position(|s| *s == "tool_execution_end:aborted");
                let settled = markers.iter().rposition(|s| *s == "agent_settled");
                stops.get() == 1
                    && ended.zip(settled).is_some_and(|(end, settle)| end < settle)
                    && markers.contains(&"response:abort:success")
                    && !state.working
                    && !state.stopping
                    && live.controller.pending_count() == 0
                    && !live.composer.read(cx).is_streaming()
                    && state.transcript.iter().any(|e| {
                        e.id == tool_id
                            && !e.activity
                            && !e.pending
                            && !e.is_error
                            && e.result == "TAU_NATIVE_PARTIAL\nTAU_NATIVE_ABORTED"
                    })
                    && state.transcript.iter().any(|e| {
                        e.kind == "assistant"
                            && e.text.trim() == "TAU_NATIVE_TOOL_OK"
                            && !e.activity
                    })
                    && state
                        .transcript
                        .iter()
                        .any(|e| e.kind == "assistant" && e.text.trim() == "TAU_NATIVE_OK")
            },
        )
        .await;
        eprintln!(
            "PASS native-app measured rendered Stop click -> genuine ComposerEvent::Stop -> real Pi abort/tool end/agent_settled; retained assistant/tool partial, unrelated draft and unchanged empty queue; idle/pending-free"
        );
        click(&shell, &cx, "session-name").await;
        insert_rename(&shell, &cx, "Tau invented gypq");
        key(&shell, &cx, "enter");
        poll(
            &cx,
            "rename actual Pi state and persisted registry echo",
            5,
            |cx| {
                let s = shell.read(cx);
                let live = &s.sessions[&second];
                assert!(s.error.is_none() && live.controller.state().error.is_none());
                !s.header.as_ref().unwrap().read(cx).renaming()
                    && !live.controller.rename_pending()
                    && live.controller.state().identity.name == "Tau invented gypq"
                    && registry_record(live).is_some_and(|r| r["name"] == "Tau invented gypq")
                    && s.workspace.as_ref().unwrap().projects.iter().any(|p| {
                        p.sessions.iter().any(|r| {
                            r.id == live.controller.state().identity.id
                                && r.title == "Tau invented gypq"
                        })
                    })
            },
        )
        .await;
        eprintln!(
            "PASS native-app measured header click -> focused real InputHandler/Enter -> Pi name + registry + workspace echo gypq"
        );
        click(&shell, &cx, "session-name").await;
        insert_rename(&shell, &cx, "Cancelled invented title");
        key(&shell, &cx, "escape");
        poll(&cx, "rename Escape cancellation", 5, |cx| {
            let s = shell.read(cx);
            !s.header.as_ref().unwrap().read(cx).renaming()
                && s.sessions[&second].controller.state().identity.name == "Tau invented gypq"
                && registry_record(&s.sessions[&second])
                    .is_some_and(|r| r["name"] == "Tau invented gypq")
        })
        .await;
        eprintln!("PASS native-app focused rename Escape preserves Pi and persisted name");
        capture(&shell, &cx, "nonempty-renamed", false).await;
        click(&shell, &cx, &format!("archive-{second}")).await;
        poll(
            &cx,
            "archive persisted and removed from normal view retaining draft",
            5,
            |cx| {
                let s = shell.read(cx);
                assert!(s.error.is_none());
                s.sessions[&first].controller.state().draft == "Owned draft café 😀"
                    && registry_record(&s.sessions[&second]).is_some_and(|r| r["archived"] == true)
                    && window(&shell, cx)
                        .update(cx, |_, w, _| {
                            gpui_kit::base::test_support::find(
                                w,
                                &[],
                                &format!("session-status-{second}").into(),
                            )
                            .is_none()
                        })
                        .unwrap()
            },
        )
        .await;
        click(&shell, &cx, "archived").await;
        click(&shell, &cx, &format!("archived-select-{second}")).await;
        poll(&cx, "archived browse selects without restoring", 5, |cx| {
            let s = shell.read(cx);
            s.active == second
                && s.session_actions.archived
                && s.sessions[&second].controller.pending_count() == 0
                && registry_record(&s.sessions[&second]).is_some_and(|r| r["archived"] == true)
                && s.archived_items().iter().any(|r| r.id == second)
        })
        .await;
        eprintln!(
            "PASS native-app measured Archive -> normal row absent + retained draft; archived browse/select remains persisted archived"
        );
        click(&shell, &cx, &format!("restore-{second}")).await;
        poll(&cx, "explicit restore persisted false", 5, |cx| {
            let s = shell.read(cx);
            assert!(s.error.is_none());
            registry_record(&s.sessions[&second]).is_some_and(|r| r["archived"] == false)
                && !s.archived_items().iter().any(|r| r.id == second)
        })
        .await;
        click(&shell, &cx, "archived").await;
        click(&shell, &cx, &format!("session-status-{second}")).await;
        poll(
            &cx,
            "restored normal row selects retained session",
            5,
            |cx| {
                let s = shell.read(cx);
                !s.session_actions.archived
                    && s.active == second
                    && s.sessions[&first].controller.state().draft == "Owned draft café 😀"
                    && registry_record(&s.sessions[&second]).is_some_and(|r| r["archived"] == false)
            },
        )
        .await;
        eprintln!(
            "PASS native-app explicit measured Restore -> registry false -> normal rendered row selects; unrelated draft retained"
        );
    }
    click(&shell, &cx, &format!("session-status-{first}")).await;
    poll(&cx, "draft persisted before close", 5, |cx| {
        let s = shell.read(cx);
        s.active == first
            && s.sessions[&first].persisted_draft == "Owned draft café 😀"
            && s.pending_saves.is_empty()
    })
    .await;
    key(&shell, &cx, "cmd-w");
    poll(&cx, "save and close owned window", 5, |cx| {
        cx.windows().is_empty()
    })
    .await;
    cx.update(|cx| cx.global::<NativeHost>().clone().reopen(cx));
    poll(&cx, "reopened retained editor and draft", 5, |cx| {
        let target = shell.read(cx).window;
        let active =
            target.is_some_and(|target| target.update(cx, |_, w, _| w.is_window_active()).unwrap());
        let s = shell.read(cx);
        active
            && s.active == first
            && s.sessions[&first]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .as_ref()
                == "Owned draft café 😀"
    })
    .await;
    eprintln!(
        "PASS native-app save/close/reopen retains selected draft; own native renderer painted measured controls"
    );
    let observed_selects = Rc::new(Cell::new(0usize));
    let observed = observed_selects.clone();
    let _click_observer = cx.update(|cx| {
        let sidebar = shell.read(cx).sidebar.as_ref().unwrap().clone();
        cx.subscribe(&sidebar, move |_, action: &Action, _| {
            if matches!(action, Action::Select(_)) {
                observed.set(observed.get() + 1);
                eprintln!(
                    "OBSERVED reopened sidebar Select emitted count={}",
                    observed.get()
                );
            }
        })
    });
    click(&shell, &cx, &format!("session-status-{second}")).await;
    poll(&cx, "reopened sidebar selects second session", 5, |cx| {
        shell.read(cx).active == second
    })
    .await;
    assert_eq!(observed_selects.get(), 1);
    eprintln!("PASS native-app reopened measured sidebar click emits Select and switches session");
    ready(&shell, &cx).await;
    focus_composer(&shell, &cx);
    insert_composer(&shell, &cx, "/tau-native-smoke");
    key(&shell, &cx, "enter");
    let mut last = None;
    poll(&cx, "second real Pi extension input", 12, |cx| {
        let s = shell.read(cx);
        let live = &s.sessions[&second];
        let status = (s.active == second, live.controller.state().ready, live.controller.pending_count(), live.controller.state().draft.len(), live.composer.read(cx).is_disabled(), live.controller.state().extension_dialogs.len(), live.controller.state().error.is_some());
        if last != Some(status) { eprintln!("OBSERVED second submit active/ready/pending/draft_len/disabled/dialogs/error={status:?}"); last = Some(status); }
        live.extension.is_some()
    })
    .await;
    let identities = cx.update(|cx| {
        let s = shell.read(cx);
        (
            s.window.unwrap(),
            s.sessions[&second].composer.read(cx).editor().entity_id(),
        )
    });
    key(&shell, &cx, "cmd-w");
    poll(&cx, "busy close confirmation", 5, |cx| {
        shell.read(cx).confirmation.is_some()
    })
    .await;
    key(&shell, &cx, "escape");
    poll(&cx, "close cancelled retains root and composer", 5, |cx| {
        let s = shell.read(cx);
        s.lifecycle.is_none()
            && s.confirmation.is_none()
            && s.window == Some(identities.0)
            && s.sessions[&second].composer.read(cx).editor().entity_id() == identities.1
    })
    .await;
    eprintln!("PASS native-app busy close cancel retains actual root and composer identities");
    poll(
        &cx,
        "extension focus restored after close cancel",
        5,
        |cx| {
            window(&shell, cx)
                .update(cx, |_, w, cx| {
                    let s = shell.read(cx);
                    s.sessions[&second]
                        .extension
                        .as_ref()
                        .unwrap()
                        .1
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w)
                })
                .unwrap()
        },
    )
    .await;
    cx.update(|cx| {
        window(&shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                let editor = s.sessions[&second]
                    .extension
                    .as_ref()
                    .unwrap()
                    .1
                    .read(cx)
                    .editor();
                editor.update(cx, |s, cx| {
                    s.replace_text_in_range(None, "invented reply", w, cx)
                });
            })
            .unwrap()
    });
    key(&shell, &cx, "enter");
    poll(
        &cx,
        "real dialog completes after close cancellation",
        12,
        |cx| {
            let s = shell.read(cx);
            assert!(s.error.is_none() && s.sessions[&second].controller.state().error.is_none());
            s.sessions[&second]
                .controller
                .state()
                .extension_dialogs
                .is_empty()
                && !s.is_busy()
        },
    )
    .await;
    complete.set(true);
    key(&shell, &cx, "cmd-q");
}
