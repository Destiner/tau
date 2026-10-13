use super::*;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, base::Root, test::TestWindowExt};

struct Host {
    composer: Entity<Composer>,
    top: f32,
    pane_width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().relative().child(
            div()
                .absolute()
                .top(px(self.top))
                .left(px(12.))
                .w(px(self.pane_width))
                .child(self.composer.clone()),
        )
    }
}
fn fixture(width: f32, height: f32, top: f32) -> (TestApp, TestAppWindow<Root>, Entity<Composer>) {
    fixture_with_pane(width, height, top, 210.)
}
fn fixture_with_pane(
    width: f32,
    height: f32,
    top: f32,
    pane_width: f32,
) -> (TestApp, TestAppWindow<Root>, Entity<Composer>) {
    let mut app = TestApp::new();
    app.update(|cx| super::super::init(cx, false));
    let mut composer = None;
    let mut window = app.open_window_with_options(
        gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::new(
                Default::default(),
                gpui_kit::size(px(width), px(height)),
            ))),
            ..Default::default()
        },
        |window, cx| {
            let host = cx.new(|cx| {
                let entity = cx.new(|cx| {
                    Composer::new(
                        ComposerOptions {
                            models: (0..90)
                                .map(|i| Choice {
                                    id: format!(
                                        "{}\u{1f}model-{i}",
                                        ["openai-codex", "xai", "github-copilot"][i % 3]
                                    ),
                                    label: format!("Model {i} {}", "long name ".repeat(12)),
                                    element_id: format!("row-{i}"),
                                })
                                .collect(),
                            model_heading: "Legacy Models".into(),
                            efforts: ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                                .into_iter()
                                .map(|id| Choice {
                                    id: id.into(),
                                    label: effort_label(id).into(),
                                    element_id: format!("effort-{id}"),
                                })
                                .collect(),
                            model: "github-copilot\u{1f}model-89".into(),
                            effort: "xhigh".into(),
                            draft: "draft retained".into(),
                            dark: false,
                            disabled: false,
                            streaming: false,
                        },
                        window,
                        cx,
                    )
                });
                composer = Some(entity.clone());
                Host {
                    composer: entity,
                    top,
                    pane_width,
                }
            });
            Root::new(host, window, cx)
        },
    );
    window.draw();
    (app, window, composer.unwrap())
}
fn click(window: &mut TestAppWindow<Root>, id: &str) {
    window.draw();
    let position = window.update(|_, w, _| w.find(id.to_owned()).bounds().center());
    window.simulate_click(position, MouseButton::Left);
    window.draw();
}
fn assert_visible(window: &mut TestAppWindow<Root>, row: &str) {
    window.update(|_, w, _| {
        let list = w.find("model-list").bounds();
        let row = w.find(row.to_owned()).bounds();
        assert!(row.top() >= list.top(), "{row:?} above {list:?}");
        // The Vue viewport-minus-120 cap can be smaller than one option.
        if row.size.height <= list.size.height {
            assert!(row.bottom() <= list.bottom(), "{row:?} below {list:?}");
        }
    });
}

#[test]
fn root_large_catalog_selected_keyboard_wheel_pinned_search_and_geometry() {
    for (width, height, top, above) in [
        (420., 600., 430., true),
        (240., 270., 140., true),
        (240., 400., 0., false),
        (180., 160., 0., true),
    ] {
        let (_app, mut window, composer) = fixture(width, height, top);
        let (editor, search) = window.read(|_, cx| {
            (
                composer.read(cx).editor(),
                composer.read(cx).search_editor(),
            )
        });
        click(&mut window, "model");
        assert_visible(&mut window, "row-89");
        let search_bounds = window.update(|_, w, _| w.find("model-search").bounds());
        window.update(|_, w, _| {
            let surface = w.find("model-menu").bounds();
            let trigger = w.find("model").bounds();
            assert!(surface.left() >= px(8.) && surface.right() <= px(width - 8.));
            assert!(surface.top() >= px(8.) && surface.bottom() <= px(height - 8.));
            if above {
                assert!(surface.bottom() <= trigger.top() - px(5.));
            } else {
                assert!(surface.top() >= trigger.bottom() + px(5.));
            }
        });
        // One native list consumes wheel input; search is outside its scroll viewport.
        let position = window.update(|_, w, _| w.find("model-list").bounds().center());
        let before = window.read(|_, cx| composer.read(cx).selector_scroll.offset());
        window.simulate_scroll(position, gpui_kit::point(px(0.), px(28.)));
        window.draw();
        let after = window.read(|_, cx| composer.read(cx).selector_scroll.offset());
        assert_eq!(after.y - before.y, px(28.));
        assert_eq!(
            window.update(|_, w, _| w.find("model-search").bounds()),
            search_bounds
        );
        window.simulate_keystroke("up");
        window.draw();
        assert_eq!(
            window.read(|_, cx| composer.read(cx).model_cursor().unwrap().element_id.clone()),
            "row-86"
        );
        assert_visible(&mut window, "row-86");
        window.simulate_keystroke("enter");
        window.draw();
        assert_eq!(
            window.read(|_, cx| composer.read(cx).model().unwrap().element_id.clone()),
            "row-86"
        );
        window.read(|_, cx| {
            let s = composer.read(cx);
            assert_eq!(s.editor(), editor);
            assert_eq!(s.search_editor(), search);
            assert_eq!(editor.read(cx).value().as_str(), "draft retained");
        });
    }
}

#[test]
fn grouping_matches_vue_map_filter_and_explicit_mapping_without_scope_expansion() {
    let (_app, mut window, composer) = fixture(420., 600., 430.);
    window.read(|_, cx| {
        let s = composer.read(cx);
        let indices = s.visible_model_indices();
        assert_eq!(&indices[..4], &[0, 3, 6, 9]);
        assert_eq!(&indices[30..34], &[1, 4, 7, 10]);
        assert_eq!(s.model_group(0), "OpenAI Codex");
        assert_eq!(s.model_group(1), "xAI");
        assert_eq!(s.model_group(2), "GitHub Copilot");
    });
    window.update(|_, _, cx| {
        composer.update(cx, |s, cx| {
            s.set_model_groups(
                [
                    (s.models[1].id.clone(), "First Custom".into()),
                    (s.models[4].id.clone(), "First Custom".into()),
                ]
                .into(),
                cx,
            );
            s.set_model("outside\u{1f}not-in-scope", cx);
            s.set_model_fallback("Outside Model", cx);
        })
    });
    assert_eq!(
        window.read(|_, cx| composer.read(cx).model_label().to_owned()),
        "Outside Model"
    );
    click(&mut window, "model");
    click(&mut window, "model-heading");
    assert!(window.read(|_, cx| composer.read(cx).model().is_none()));
    assert_eq!(
        window.read(|_, cx| composer.read(cx).open_selector()),
        Some("model")
    );
    // Filter using the retained Input and direct keyboard/paste route.
    window.update(|_, _, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("model-4".into()))
    });
    window.simulate_keystroke("cmd-v");
    window.draw();
    window.read(|_, cx| {
        let s = composer.read(cx);
        assert_eq!(
            s.visible_model_indices(),
            vec![4, 40, 43, 46, 49, 41, 44, 47, 42, 45, 48]
        );
        assert_eq!(s.models.len(), 90);
    });
    window.update(|_, w, _| assert!(w.try_find("model-heading-2").is_none()));
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    window.draw();
    assert_eq!(
        window.read(|_, cx| composer.read(cx).model().unwrap().element_id.clone()),
        "row-40"
    );
    click(&mut window, "model");
    window.update(|_, _, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("no-such-model".into()))
    });
    window.simulate_keystroke("cmd-v");
    window.draw();
    assert!(window.read(|_, cx| composer.read(cx).visible_models().is_empty()));
    window.update(|_, w, _| {
        assert!(w.try_find("model-heading").is_none());
        assert!(w.try_find("model-no-match").is_some());
        assert!(w.try_find("model-empty").is_none());
    });
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    assert_eq!(
        window.read(|_, cx| composer.read(cx).open_selector()),
        Some("model")
    );
}

#[test]
fn root_search_ime_protects_navigation_selection_and_retained_ownership() {
    let (_app, mut window, composer) = fixture(420., 600., 430.);
    click(&mut window, "model");
    let search = window.read(|_, cx| composer.read(cx).search_editor());
    window.update(|_, w, cx| {
        search.update(cx, |s, cx| {
            s.replace_and_mark_text_in_range(None, "model", Some(5..5), w, cx);
        })
    });
    window.draw();
    let before = window.read(|_, cx| composer.read(cx).cursor);
    for key in ["down", "up", "enter", "escape"] {
        window.simulate_keystroke(key);
        window.draw();
        window.read(|_, cx| {
            let s = composer.read(cx);
            assert_eq!(s.cursor, before);
            assert_eq!(s.open_selector(), Some("model"));
            assert_eq!(s.search_editor(), search);
        });
    }
    window.update(|_, w, cx| search.update(cx, |s, cx| s.unmark_text(w, cx)));
    window.simulate_keystroke("escape");
    window.draw();
    assert_eq!(window.read(|_, cx| composer.read(cx).open_selector()), None);
}

#[test]
fn labels_preserve_protocol_values_and_legacy_heading() {
    for (id, label) in [
        ("off", "Off"),
        ("minimal", "Minimal"),
        ("low", "Low"),
        ("medium", "Medium"),
        ("high", "High"),
        ("xhigh", "Extra High"),
        ("max", "Max"),
        ("future", "future"),
    ] {
        assert_eq!(effort_label(id), label);
    }
    for (id, label) in [
        ("amazon-bedrock", "Amazon Bedrock"),
        ("google-vertex", "Google Vertex"),
        ("opencode", "OpenCode"),
        ("opencode-go", "OpenCode Go"),
        ("openrouter", "OpenRouter"),
        ("pi-claude", "Pi Claude"),
        ("some-provider", "Some Provider"),
        ("openai", "Openai"),
    ] {
        assert_eq!(provider_label(id), label);
    }
    let (_app, mut window, composer) = fixture(420., 600., 430.);
    click(&mut window, "effort");
    click(&mut window, "effort-high");
    assert_eq!(
        window.read(|_, cx| composer.read(cx).effort().unwrap().id.clone()),
        "high"
    );
    window.update(|_, _, cx| {
        composer.update(cx, |s, cx| {
            s.set_choices(
                vec![Choice {
                    id: "legacy".into(),
                    label: "Legacy".into(),
                    element_id: "legacy-row".into(),
                }],
                s.efforts.clone(),
                cx,
            );
            s.set_model_heading("Fixture Heading", cx);
        })
    });
    assert_eq!(
        window.read(|_, cx| composer.read(cx).model_group(0)),
        "Fixture Heading"
    );
}

#[test]
fn root_trigger_caps_use_narrow_main_pane_not_wide_window_and_empty_admission() {
    for (dark, pane_width) in [(false, 160.), (true, 210.), (false, 350.), (true, 900.)] {
        let (_app, mut window, composer) = fixture_with_pane(1000., 600., 300., pane_width);
        window.update(|_, w, cx| {
            composer.update(cx, |s, cx| {
                s.set_theme(dark, w, cx);
                s.efforts = vec![Choice {
                    id: "future".into(),
                    label: "Long unbroken effort".repeat(20),
                    element_id: "future-effort".into(),
                }];
                s.effort = 0;
            })
        });
        window.draw();
        window.draw();
        window.update(|_, w, _| {
            let model = w.find("model").bounds();
            let effort = w.find("effort").bounds();
            let send = w.find("send").bounds();
            assert!(model.size.width <= px(210.));
            assert!(effort.size.width <= px(110.));
            assert!(
                send.right() <= px(12. + pane_width),
                "toolbar escaped its main pane in 1000px window"
            );
        });
        click(&mut window, "model");
        window.update(|_, w, _| assert!(w.find("model-menu").bounds().size.height <= px(320.)));
        window.update(|_, _, cx| composer.update(cx, |s, cx| s.set_choices(vec![], vec![], cx)));
        click(&mut window, "effort");
        assert_eq!(window.read(|_, cx| composer.read(cx).open_selector()), None);
        click(&mut window, "model");
        window.update(|_, w, _| {
            assert!(w.try_find("model-empty").is_some());
            assert!(w.try_find("model-no-match").is_none());
            assert!(w.try_find("model-search").is_none());
        });
        window.read(|_, cx| {
            assert_eq!(
                composer.read(cx).editor.read(cx).value().as_str(),
                "draft retained"
            )
        });
    }
}

#[test]
fn root_start_stop_pending_modes_clear_and_settings_keep_editing_but_guard_actions() {
    let (_app, mut window, composer) = fixture(1000., 600., 300.);
    let events = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = events.clone();
    window.update(|_, _, cx| {
        cx.subscribe(&composer, move |_, _, event: &Event, _| {
            seen.borrow_mut().push(event.clone())
        })
        .detach()
    });
    for (ready, streaming, stopping, pending, queue_busy, extension, setting) in [
        (false, false, false, false, false, false, false), // starting
        (true, true, true, false, false, false, false),    // stopping
        (true, true, false, true, true, false, false),     // mode write or prompt ack
        (true, true, false, false, true, false, false),    // queue clear
        (true, false, false, true, false, false, false),   // ordinary prompt ack
        (true, true, false, false, false, true, false),    // extension prompt
        (true, false, false, false, false, false, true),   // settings: send stays admitted
    ] {
        events.borrow_mut().clear();
        window.update(|_, w, cx| {
            composer.update(cx, |s, cx| {
                s.set_status(false, streaming, cx);
                s.set_admission(setting, false, cx);
                s.set_readiness(ready, stopping, pending, queue_busy, extension, cx);
                s.editor.update(cx, |e, cx| {
                    e.set_value("new draft", w, cx);
                    e.focus(w, cx);
                });
            })
        });
        window.simulate_input(" edited");
        window.draw();
        assert!(window.read(|_, cx| composer.read(cx).editor.read(cx).value().contains("edited")));
        let before = window.read(|_, cx| composer.read(cx).editor.read(cx).value().to_string());
        click(&mut window, "send");
        if setting {
            assert_eq!(events.borrow().as_slice(), &[Event::Send(before)]);
        } else {
            assert!(events.borrow().is_empty());
            window.read(|_, cx| {
                assert_eq!(composer.read(cx).editor.read(cx).value().as_str(), before)
            });
        }
    }
    // Pending queue work must not veto a genuine Stop, but never turns blank Enter into Stop.
    events.borrow_mut().clear();
    window.update(|_, w, cx| {
        composer.update(cx, |s, cx| {
            s.set_status(false, true, cx);
            s.set_admission(false, false, cx);
            s.set_readiness(true, false, true, true, false, cx);
            s.editor.update(cx, |e, cx| {
                e.set_value("", w, cx);
                e.focus(w, cx);
            });
        })
    });
    window.simulate_keystroke("enter");
    window.draw();
    assert!(events.borrow().is_empty());
    click(&mut window, "send");
    assert_eq!(events.borrow().as_slice(), &[Event::Stop]);
}

#[test]
fn root_composer_ime_and_same_frame_submission_latch_preserve_next_draft() {
    let (_app, mut window, composer) = fixture(1000., 600., 300.);
    window.update(|_, w, cx| {
        composer.update(cx, |s, cx| {
            s.editor.update(cx, |e, cx| {
                e.focus(w, cx);
                e.replace_and_mark_text_in_range(None, "composing", Some(9..9), w, cx);
            });
            s.activate(false, true, w, cx);
            assert!(!s.submission_pending);
            assert!(!s.editor.read(cx).value().is_empty());
            s.editor.update(cx, |e, cx| e.unmark_text(w, cx));
            s.activate(false, false, w, cx);
            assert!(s.submission_pending);
            s.editor
                .update(cx, |e, cx| e.set_value("next draft", w, cx));
            s.activate(false, false, w, cx);
            assert_eq!(s.editor.read(cx).value().as_str(), "next draft");
            s.set_readiness(true, false, false, false, false, cx);
            s.activate(false, false, w, cx);
            assert!(s.editor.read(cx).value().is_empty());
        })
    });
}

#[test]
fn root_readiness_changes_do_not_take_focus_from_shell_or_replace_retained_editor() {
    let (_app, mut window, composer) = fixture(1000., 600., 300.);
    let editor = window.read(|_, cx| composer.read(cx).editor());
    window.update(|_, w, cx| {
        let shell_focus = cx.focus_handle();
        shell_focus.focus(w, cx);
        for (ready, stopping) in [(false, false), (true, false), (true, true), (true, false)] {
            composer.update(cx, |s, cx| {
                s.set_status(false, false, cx);
                s.set_readiness(ready, stopping, false, false, false, cx);
            });
            assert!(shell_focus.is_focused(w));
            assert_eq!(composer.read(cx).editor(), editor);
        }
    });
}

#[test]
fn root_retry_reason_is_focusable_and_retained_draft_survives_real_state_changes() {
    let (_app, mut window, composer) = fixture(1000., 600., 300.);
    window.update(|_, _, cx| {
        composer.update(cx, |s, cx| {
            s.set_status(false, true, cx);
            s.set_retry(
                Some(retry::RetryPresentation {
                    attempt: Some(2),
                    max_attempts: Some(3),
                    reason: tau_session::session::RetryReason::RateLimit,
                }),
                cx,
            );
        })
    });
    window.draw();
    window.draw();
    window.update(|_, w, cx| {
        assert!(w.try_find("retry-indicator").is_some());
        let focus = composer.read(cx).retry_focus.clone();
        focus.focus(w, cx);
    });
    window.draw();
    window.update(|_, w, _| assert!(w.try_find("retry-tooltip").is_some()));
    window.update(|_, _, cx| composer.update(cx, |s, cx| s.set_retry(None, cx)));
    window.draw();
    window.update(|_, w, _| {
        assert!(w.try_find("retry-indicator").is_none());
        assert!(w.try_find("retry-tooltip").is_none());
    });
    window.read(|_, cx| {
        assert_eq!(
            composer.read(cx).editor.read(cx).value().as_str(),
            "draft retained"
        )
    });
}
