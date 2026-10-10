//! Opt-in acceptance against the production shell, worker, renderer and installed Pi.
use super::*;
use gpui_kit::base::input::InputState;
use gpui_kit::{
    AsyncApp, EntityInputHandler, Focusable, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput,
};
use std::cell::Cell;

pub const COMPLETE: &str = "PASS native-app completion-gated last CmdW quit";

pub fn run_native_smoke(root: PathBuf, project: String, check_menus: fn()) -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        eprintln!(
            "FAIL native-app assertion at line/column {:?} (no transcript or raw error logged)",
            info.location().map(|p| (p.line(), p.column()))
        );
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
    let captured_shell = shell.clone();
    cx.update(|cx| {
        window(shell, cx).update(cx, |_, w, _| {
            // GPUI drains frame callbacks before drawing. A second callback
            // observes the scene painted by the first normal platform frame.
            w.on_next_frame(move |w, _| w.on_next_frame(move |w, cx| {
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
                    let s = captured_shell.read(cx);
                    let textarea = s.sessions[&s.active].composer.read(cx).editor();
                    let first_line = textarea.read(cx).range_to_bounds(&(0..0)).expect("painted first line layout");
                    assert_eq!(first_line.top(), area.top() + px(10.));
                    eprintln!("PASS native-app {name} actual empty first-line bounds={first_line:?} inset=10px");
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

fn modal_facts(
    editor: &Entity<InputState>,
    w: &Window,
    cx: &App,
    panel: &str,
    field: &str,
) -> [bool; 6] {
    use gpui_kit::base::test_support::find;
    let panel = find(w, &[], &panel.to_owned().into());
    let field = panel
        .as_ref()
        .and_then(|panel| find(w, panel.path(), &field.to_owned().into()));
    let measured = |node: &gpui_kit::base::test_support::ElementSnapshot| {
        node.visible() && node.bounds().size.width > px(0.) && node.bounds().size.height > px(0.)
    };
    // BaseInput registers its wrapper geometry, not editor focus/value facts.
    let input = editor.read(cx);
    [
        panel.is_some(),
        field.is_some(),
        panel.as_ref().is_some_and(measured),
        field.as_ref().is_some_and(measured),
        input.focus_handle(cx).is_focused(w),
        input.value().is_empty(),
    ]
}

fn modal_ready(
    editor: &Entity<InputState>,
    w: &Window,
    cx: &App,
    panel: &str,
    field: &str,
) -> bool {
    modal_facts(editor, w, cx, panel, field)
        .into_iter()
        .all(|fact| fact)
}

async fn capture_modal(
    shell: &Entity<Shell>,
    cx: &AsyncApp,
    name: &str,
    field: &str,
    editor: Entity<InputState>,
) {
    let painted = Rc::new(Cell::new(false));
    let observed = painted.clone();
    let name = name.to_owned();
    let field = field.to_owned();
    let panel = if name == "palette" {
        "palette-surface"
    } else {
        "remote-dialog"
    };
    cx.update(|cx| {
        window(shell, cx).update(cx, |_, w, _| {
            w.on_next_frame(move |w, _| w.on_next_frame(move |w, cx| {
                assert!(modal_ready(&editor, w, cx, panel, &field));
                let bounds = gpui_kit::base::test_support::find(w, &[], &panel.into()).unwrap().bounds();
                eprintln!("PASS native-app {name} visible measured root panel + focused empty input bounds={bounds:?}");
                if let Some(directory) = std::env::var_os("TAU_NATIVE_CAPTURE_DIR") {
                    let destination = PathBuf::from(directory).join(format!("{name}.png"));
                    let image = w.render_to_image().expect("owned modal renderer capture");
                    let (width, height) = image.dimensions();
                    image.save(&destination).expect("save actual modal renderer PNG");
                    eprintln!("PASS native-app owned Metal scene PNG {width}x{height}: {}", destination.display());
                }
                observed.set(true);
            }));
        }).unwrap();
    });
    poll(
        cx,
        "normal native modal painted frame and capture",
        5,
        |_| painted.get(),
    )
    .await;
}

async fn palette_remote(shell: &Entity<Shell>, cx: &AsyncApp, first: &str) {
    let counts = cx.update(|cx| {
        let s = shell.read(cx);
        (
            s.sessions.len(),
            s.workspace.as_ref().unwrap().projects.len(),
        )
    });
    key(shell, cx, "cmd-k");
    let observed = Cell::new(None);
    poll(
        cx,
        "CmdK unfiltered root palette and focused search",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let s = shell.read(cx);
                    let open = s.palette.view.as_ref().is_some_and(|view| {
                        let p = view.read(cx);
                        p.is_open()
                            && p.page_id() == "root"
                            && p.query().is_empty()
                            && p.search_editor().read(cx).focus_handle(cx).is_focused(w)
                    });
                    let facts = s.palette.view.as_ref().map(|view| {
                        modal_facts(&view.read(cx).search_editor(), w, cx, "palette-surface", "search")
                    }).unwrap_or([false; 6]);
                    let visible = facts.into_iter().all(|fact| fact);
                    let flags = (w.is_window_active(), s.loaded, s.error.is_none(), open, facts);
                    if observed.replace(Some(flags)) != Some(flags) {
                        eprintln!("OBSERVED palette active/loaded/no-error/open-focused/(panel/field/panel-measured/field-measured/editor-focused/editor-empty)={flags:?}");
                    }
                    open && visible
                })
                .unwrap()
        },
    )
    .await;
    let editor = cx.update(|cx| {
        shell
            .read(cx)
            .palette
            .view
            .as_ref()
            .unwrap()
            .read(cx)
            .search_editor()
    });
    capture_modal(shell, cx, "palette", "search", editor).await;
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let editor = shell
                    .read(cx)
                    .palette
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .search_editor();
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "Open Remote Project", w, cx)
                });
            })
            .unwrap();
    });
    poll(
        cx,
        "real palette query selects Open Remote Project",
        5,
        |cx| {
            let s = shell.read(cx);
            let p = s.palette.view.as_ref().unwrap().read(cx);
            p.query() == "Open Remote Project" && p.selected_id() == Some("project.openRemote")
        },
    )
    .await;
    key(shell, cx, "enter");
    let observed = Cell::new(None);
    poll(cx, "palette Enter opens focused remote form", 5, |cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                assert!(!s.remote.is_busy());
                let facts = s.remote.view.as_ref().map(|view| {
                    modal_facts(&view.read(cx).editor(), w, cx, "remote-dialog", "remote-field")
                }).unwrap_or([false; 6]);
                let flags = (s.remote.is_open(), !s.palette.is_open(cx), facts);
                if observed.replace(Some(flags)) != Some(flags) {
                    eprintln!("OBSERVED remote open/palette-closed/(panel/field/panel-measured/field-measured/editor-focused/editor-empty)={flags:?}");
                }
                flags.0 && flags.1 && facts.into_iter().all(|fact| fact)
            })
            .unwrap()
    })
    .await;
    let editor = cx.update(|cx| {
        shell
            .read(cx)
            .remote
            .view
            .as_ref()
            .unwrap()
            .read(cx)
            .editor()
    });
    capture_modal(shell, cx, "remote", "remote-field", editor).await;
    key(shell, cx, "escape");
    for reopen in [true, false] {
        poll(
            cx,
            "modal Escape preserves live draft and restores composer focus",
            5,
            |cx| {
                window(shell, cx)
                    .update(cx, |_, w, cx| {
                        let s = shell.read(cx);
                        assert_eq!(
                            (
                                s.sessions.len(),
                                s.workspace.as_ref().unwrap().projects.len()
                            ),
                            counts
                        );
                        assert!(!s.remote.is_busy());
                        let live = &s.sessions[first];
                        let editor = live.composer.read(cx).editor();
                        s.active == first
                            && !s.remote.is_open()
                            && !s.palette.is_open(cx)
                            && live.controller.state().draft == "Owned draft café 😀"
                            && editor.read(cx).value().as_ref() == "Owned draft café 😀"
                            && editor.read(cx).focus_handle(cx).is_focused(w)
                            && gpui_kit::base::test_support::find(w, &[], &"remote-dialog".into())
                                .is_none()
                            && gpui_kit::base::test_support::find(w, &[], &"palette-surface".into())
                                .is_none()
                    })
                    .unwrap()
            },
        )
        .await;
        if reopen {
            key(shell, cx, "cmd-k");
            let observed = Cell::new(None);
            poll(cx, "reopened root palette focused search", 5, |cx| {
                window(shell, cx)
                    .update(cx, |_, w, cx| {
                        let s = shell.read(cx);
                        let root = s.palette.view.as_ref().is_some_and(|view| {
                            let p = view.read(cx);
                            p.is_open() && p.page_id() == "root" && p.query().is_empty()
                        });
                        let facts = s.palette.view.as_ref().map(|view| {
                            modal_facts(&view.read(cx).search_editor(), w, cx, "palette-surface", "search")
                        }).unwrap_or([false; 6]);
                        let flags = (root, facts);
                        if observed.replace(Some(flags)) != Some(flags) {
                            eprintln!("OBSERVED reopened palette root/(panel/field/panel-measured/field-measured/editor-focused/editor-empty)={flags:?}");
                        }
                        root && facts.into_iter().all(|fact| fact)
                    })
                    .unwrap()
            })
            .await;
            eprintln!(
                "PASS native-app reopened root palette visible measured panel + focused empty input"
            );
            key(shell, cx, "escape");
        }
    }
    eprintln!(
        "PASS native-app CmdK/query/Enter remote form + Escape/reopen/Escape retain draft and live composer focus without SSH"
    );
}

fn admin_code(shell: &Entity<Shell>, cx: &AsyncApp) {
    for binding in ["i", "d", "d", "q", "d"] {
        key(shell, cx, binding);
    }
}

fn persisted_admin(root: &std::path::Path) -> Option<bool> {
    let bytes = std::fs::read(root.join("tau/preferences.json")).ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes).ok()?["adminMode"].as_bool()
}

async fn sidebar_focus(shell: &Entity<Shell>, cx: &AsyncApp, first: &str) {
    click(shell, cx, &format!("session-status-{first}")).await;
    poll(cx, "natural dirty-dot click focuses sidebar row", 5, |cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let s = shell.read(cx);
                let dot = gpui_kit::base::test_support::find(
                    w,
                    &[],
                    &format!("session-status-{first}").into(),
                )
                .unwrap();
                assert_eq!(dot.bounds().size, gpui_kit::size(px(6.), px(6.)));
                assert_eq!(
                    s.sessions[first].controller.state().draft,
                    "Owned draft café 😀"
                );
                assert_eq!(
                    s.sessions[first]
                        .composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .value()
                        .as_ref(),
                    "Owned draft café 😀"
                );
                s.active == first
                    && !s.sessions[first]
                        .composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(w)
                    && gpui_kit::base::test_support::find(
                        w,
                        &[],
                        &format!("session-{first}").into(),
                    )
                    .is_some_and(|row| row.visible() && row.focused() == Some(true))
            })
            .unwrap()
    })
    .await;
}

async fn report_editor_focus(shell: &Entity<Shell>, cx: &AsyncApp) {
    poll(
        cx,
        "report opens with actual checkbox or editor focus",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let view = shell.read(cx).admin.view.as_ref().unwrap().read(cx);
                    view.is_open()
                        && (view.editor().read(cx).focus_handle(cx).is_focused(w)
                            || gpui_kit::base::test_support::find(
                                w,
                                &[],
                                &"report-include-session".into(),
                            )
                            .is_some_and(|checkbox| {
                                checkbox.visible() && checkbox.focused() == Some(true)
                            }))
                })
                .unwrap()
        },
    )
    .await;
    let checkbox_focused = cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, _| {
                gpui_kit::base::test_support::find(w, &[], &"report-include-session".into())
                    .is_some_and(|checkbox| checkbox.focused() == Some(true))
            })
            .unwrap()
    });
    if checkbox_focused {
        key(shell, cx, "tab");
    }
    poll(
        cx,
        "report Tab reaches retained textarea naturally",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    shell
                        .read(cx)
                        .admin
                        .view
                        .as_ref()
                        .unwrap()
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
}

async fn capture_report(shell: &Entity<Shell>, cx: &AsyncApp) {
    let painted = Rc::new(Cell::new(false));
    let observed = painted.clone();
    cx.update(|cx| {
        let view = shell.read(cx).admin.view.as_ref().unwrap().clone();
        window(shell, cx).update(cx, |_, w, _| {
            w.on_next_frame(move |w, _| w.on_next_frame(move |w, cx| {
                assert!(view.read(cx).editor().read(cx).focus_handle(cx).is_focused(w));
                for id in ["issue-report-popover", "report-description-field", "report-trigger"] {
                    let control = gpui_kit::base::test_support::find(w, &[], &id.into()).unwrap();
                    assert!(control.visible());
                    assert!(control.bounds().size.width > px(0.) && control.bounds().size.height > px(0.));
                }
                let checkbox = gpui_kit::base::test_support::find(w, &[], &"report-include-session".into()).unwrap();
                eprintln!("OBSERVED report checkbox checked/disabled={:?}/{:?}", checkbox.checked(), checkbox.disabled());
                assert_eq!(checkbox.checked(), Some(false));
                assert_ne!(checkbox.disabled(), Some(true));
                if let Some(directory) = std::env::var_os("TAU_NATIVE_CAPTURE_DIR") {
                    w.render_to_image().expect("owned report renderer capture")
                        .save(PathBuf::from(directory).join("issue-report.png"))
                        .expect("save owned report PNG");
                }
                eprintln!("PASS native-app measured issue-report-popover + focused retained textarea + unchecked session");
                observed.set(true);
            }));
        }).unwrap();
    });
    poll(cx, "owned issue reporter painted capture", 5, |_| {
        painted.get()
    })
    .await;
}

async fn admin_report(shell: &Entity<Shell>, cx: &AsyncApp, root: &std::path::Path, first: &str) {
    const DESCRIPTION: &str = "Invented native smoke issue café 😀 iddqd";
    let issues = root.join("tau/feedback/issues.jsonl");
    assert!(
        !issues.exists(),
        "isolated fixture must start without reports"
    );
    assert!(!cx.update(|cx| shell.read(cx).admin.enabled()));
    sidebar_focus(shell, cx, first).await;
    admin_code(shell, cx);
    poll(
        cx,
        "raw sidebar iddqd enables shared persisted admin",
        5,
        |cx| {
            let s = shell.read(cx);
            s.admin.enabled() && !s.admin.mode_failed() && persisted_admin(root) == Some(true)
        },
    )
    .await;
    click(shell, cx, "report-trigger").await;
    for expected in [true, false] {
        click(shell, cx, "report-include-session").await;
        poll(
            cx,
            "real report checkbox toggles optional attachment",
            5,
            |cx| {
                window(shell, cx)
                    .update(cx, |_, w, _| {
                        gpui_kit::base::test_support::find(w, &[], &"report-include-session".into())
                            .is_some_and(|checkbox| checkbox.checked() == Some(expected))
                    })
                    .unwrap()
            },
        )
        .await;
    }
    report_editor_focus(shell, cx).await;
    capture_report(shell, cx).await;
    let view = cx.update(|cx| shell.read(cx).admin.view.as_ref().unwrap().clone());
    let editor = cx.update(|cx| view.read(cx).editor());
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "iddqd", w, cx)
                });
            })
            .unwrap();
    });
    admin_code(shell, cx);
    cx.update(|cx| {
        assert!(
            shell.read(cx).admin.enabled(),
            "editor must refuse admin code"
        );
        assert!(editor.read(cx).value().contains("iddqd"));
    });
    key(shell, cx, "cmd-a");
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                assert!(
                    shell.read(cx).admin.enabled(),
                    "editor must refuse admin code"
                );
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |input, cx| {
                    input.replace_text_in_range(None, DESCRIPTION, w, cx)
                });
            })
            .unwrap();
    });
    key(shell, cx, "escape");
    poll(cx, "report Escape retains unsent editor", 5, |cx| {
        !view.read(cx).is_open() && editor.read(cx).value().as_ref() == DESCRIPTION
    })
    .await;
    click(shell, cx, "report-trigger").await;
    report_editor_focus(shell, cx).await;
    cx.update(|cx| {
        assert_eq!(
            shell.read(cx).admin.view.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        assert_eq!(view.read(cx).editor().entity_id(), editor.entity_id());
        assert_eq!(editor.read(cx).value().as_ref(), DESCRIPTION);
    });
    click(shell, cx, "report-submit").await;
    poll(
        cx,
        "queued shared report saved indicator and exactly one isolated record",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let saved = gpui_kit::base::test_support::find(w, &[], &"report-sent".into())
                        .is_some_and(|indicator| indicator.visible());
                    if !saved {
                        return false;
                    }
                    assert!(editor.read(cx).value().is_empty());
                    let contents =
                        std::fs::read_to_string(&issues).expect("isolated shared issues file");
                    let lines: Vec<_> = contents.lines().collect();
                    assert_eq!(lines.len(), 1);
                    let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
                    assert_eq!(record["description"], DESCRIPTION);
                    assert!(record.get("sessionId").is_none());
                    true
                })
                .unwrap()
        },
    )
    .await;
    sidebar_focus(shell, cx, first).await;
    admin_code(shell, cx);
    poll(
        cx,
        "real sidebar iddqd disables persisted admin and reporter",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let s = shell.read(cx);
                    !s.admin.enabled()
                        && !s.admin.mode_failed()
                        && persisted_admin(root) == Some(false)
                        && gpui_kit::base::test_support::find(w, &[], &"report-trigger".into())
                            .is_none()
                        && gpui_kit::base::test_support::find(
                            w,
                            &[],
                            &"issue-report-popover".into(),
                        )
                        .is_none()
                        && s.sessions[first].controller.state().draft == "Owned draft café 😀"
                })
                .unwrap()
        },
    )
    .await;
    eprintln!(
        "PASS native-app dirty-dot natural focus + raw admin unlock/editor refusal + retained report Escape/reopen + shared isolated save + persisted admin off"
    );
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
async fn empty_session_keyboard_switch(shell: &Entity<Shell>, cx: &AsyncApp, first: &str) {
    let count = cx.update(|cx| shell.read(cx).sessions.len());
    cx.update(|cx| cx.dispatch_action(&crate::native::NewSession));
    ready(shell, cx).await;
    let temporary = cx.update(|cx| shell.read(cx).active.clone());
    assert_ne!(temporary, first);
    capture(shell, cx, "transient-empty", true).await;
    key(shell, cx, "cmd-p");
    poll(cx, "CmdP sessions search naturally focused", 5, |cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                shell.read(cx).palette.view.as_ref().is_some_and(|view| {
                    let p = view.read(cx);
                    p.is_open()
                        && p.page_id() == "sessions"
                        && modal_ready(&p.search_editor(), w, cx, "palette-surface", "search")
                })
            })
            .unwrap()
    })
    .await;
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let editor = shell
                    .read(cx)
                    .palette
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .search_editor();
                assert!(editor.read(cx).focus_handle(cx).is_focused(w));
                editor.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "New Session", w, cx)
                });
            })
            .unwrap();
    });
    poll(cx, "actual CmdP search results", 5, |cx| {
        let s = shell.read(cx);
        let p = s.palette.view.as_ref().unwrap().read(cx);
        p.query() == "New Session" && p.selected_id().is_some()
    })
    .await;
    // Both live blanks have the same title; choose the original via actual arrow keys.
    let target = format!("live:Live({first:?},");
    for _ in 0..=count {
        let selected = cx.update(|cx| {
            shell
                .read(cx)
                .palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_id()
                .unwrap()
                .to_owned()
        });
        if selected.starts_with(&target) {
            break;
        }
        key(shell, cx, "down");
        poll(cx, "CmdP keyboard selection advances", 5, |cx| {
            shell
                .read(cx)
                .palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_id()
                != Some(selected.as_str())
        })
        .await;
    }
    cx.update(|cx| {
        assert!(
            shell
                .read(cx)
                .palette
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_id()
                .unwrap()
                .starts_with(&target)
        );
    });
    key(shell, cx, "enter");
    ready(shell, cx).await;
    poll(
        cx,
        "CmdP retires blank and acknowledges runtime disposal",
        5,
        |cx| {
            let s = shell.read(cx);
            assert!(s.error.is_none());
            assert!(
                s.sessions
                    .values()
                    .all(|live| live.controller.state().error.is_none())
            );
            s.active == first
                && !s.palette.is_open(cx)
                && !s.sessions.contains_key(&temporary)
                && s.sessions.len() == count
                && s.empty_candidates.is_empty()
                && s.empty_disposals.is_empty()
                && s.sessions
                    .values()
                    .all(|live| live.registration_pending.is_empty())
                && s.sessions[first].controller.state().draft == "Owned draft café 😀"
                && s.sessions[first]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref()
                    == "Owned draft café 😀"
        },
    )
    .await;
    // No activation, click or focus repair between the session switch and CmdK.
    key(shell, cx, "cmd-k");
    poll(
        cx,
        "post-CmdP CmdK root palette naturally focused",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    shell.read(cx).palette.view.as_ref().is_some_and(|view| {
                        let p = view.read(cx);
                        p.is_open()
                            && p.page_id() == "root"
                            && modal_ready(&p.search_editor(), w, cx, "palette-surface", "search")
                    })
                })
                .unwrap()
        },
    )
    .await;
    key(shell, cx, "escape");
    poll(
        cx,
        "post-switch Escape restores actual first composer focus",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let s = shell.read(cx);
                    let live = &s.sessions[first];
                    let editor = live.composer.read(cx).editor();
                    assert!(s.error.is_none() && live.controller.state().error.is_none());
                    s.active == first
                        && !s.palette.is_open(cx)
                        && editor.read(cx).focus_handle(cx).is_focused(w)
                        && editor.read(cx).value().as_ref() == "Owned draft café 😀"
                        && live.controller.state().draft == "Owned draft café 😀"
                        && gpui_kit::base::test_support::find(w, &[], &"palette-surface".into())
                            .is_none()
                })
                .unwrap()
        },
    )
    .await;
    eprintln!(
        "PASS native-app real CmdP search/arrows/Enter disposes blank + retains Unicode draft; immediate CmdK/Escape restores first composer without refocus"
    );
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
    palette_remote(&shell, &cx, &first).await;
    admin_report(&shell, &cx, &root, &first).await;
    empty_session_keyboard_switch(&shell, &cx, &first).await;
    cx.update(|cx| cx.dispatch_action(&crate::native::NewSession));
    ready(&shell, &cx).await;
    let second = cx.update(|cx| shell.read(cx).active.clone());
    assert_ne!(first, second);
    poll(
        &cx,
        "second composer naturally focused before draft input",
        5,
        |cx| {
            window(&shell, cx)
                .update(cx, |_, w, cx| {
                    shell.read(cx).sessions[&second]
                        .composer
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
    insert_composer(&shell, &cx, "/tau-native-smoke");
    poll(
        &cx,
        "second draft protects session before sidebar switch",
        5,
        |cx| shell.read(cx).sessions[&second].controller.state().draft == "/tau-native-smoke",
    )
    .await;
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
        let s = shell.read(cx);
        s.active == second
            && s.sessions[&second].controller.state().draft == "/tau-native-smoke"
            && s.sessions[&second]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .as_ref()
                == "/tau-native-smoke"
    })
    .await;
    eprintln!("PASS native-app NewSession + real sidebar switches retain editor draft");
    check_menus();
    eprintln!("PASS native-app native menu omits Settings");
    focus_composer(&shell, &cx);
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
    click(&shell, &cx, &format!("session-status-{second}")).await;
    poll(
        &cx,
        "sidebar selects second session before busy close",
        5,
        |cx| shell.read(cx).active == second,
    )
    .await;
    eprintln!("PASS native-app measured sidebar click switches session before busy close");
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
            && s.sessions[&first].controller.state().draft == "Owned draft café 😀"
            && cx.windows().len() == 1
    })
    .await;
    eprintln!(
        "PASS native-app busy last CmdW cancel retains actual root/composer and unrelated draft"
    );
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
    cx.update(|cx| {
        assert_eq!(cx.windows().len(), 1);
        assert_eq!(
            shell.read(cx).sessions[&first].controller.state().draft,
            "Owned draft café 😀"
        );
    });
    complete.set(true);
    key(&shell, &cx, "cmd-w");
}
