use super::*;
use gpui_kit::{TestApp, base::Root, point, test::TestWindowExt};
use serde_json::json;
use tau_session::session::Controller;

fn fixture(dark: bool) -> (TestApp, gpui_kit::TestAppWindow<Root>, Entity<Transcript>) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let transcript = app.new_entity(|cx| Transcript::new("notice-transcript", dark, cx));
    let mut window =
        app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
    window.simulate_resize(size(px(1000.), px(900.)));
    window.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    app.run_until_parked();
    window.activate_accessibility();
    (app, window, transcript)
}
fn copy(window: &mut gpui_kit::TestAppWindow<Root>) -> String {
    window.simulate_keystroke("cmd-c");
    window.draw();
    window.read(|_, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    })
}
fn quad_at(window: &Window, id: &str) -> gpui_kit::Background {
    let bounds = window.find(id.to_owned()).bounds();
    window
        .painted_quads()
        .into_iter()
        .find(|q| {
            q.bounds
                == gpui_kit::Bounds {
                    origin: bounds.origin.scale(window.scale_factor()),
                    size: bounds.size.scale(window.scale_factor()),
                }
        })
        .unwrap_or_else(|| panic!("missing painted quad at {id}"))
        .background
}

/// Called by projection.rs: real controller wire → production projection → retained Root.
pub(crate) fn production_notices(
    mut sync: impl FnMut(&[tau_session::transcript::Entry], &Entity<Transcript>, &mut Context<()>),
) {
    for (dark, scale) in [false, true]
        .into_iter()
        .flat_map(|dark| [2., 5.].map(|scale| (dark, scale)))
    {
        let mut controller = Controller::new("notices");
        let bootstrap = controller.begin_generation(1);
        let request = bootstrap
            .iter()
            .find_map(|effect| match effect {
                tau_session::session::Effect::Send(request)
                    if request["type"] == "get_messages" =>
                {
                    Some(request)
                }
                _ => None,
            })
            .unwrap();
        // Custom output is hydrated by the command's get_messages read, not an assistant stream.
        controller.handle(
            json!({"type":"response", "id":request["id"], "command":"get_messages",
            "success":true, "data":{"messages":[{
                "role":"custom", "customType":"usage", "display":true, "timestamp":1,
                "content":[{"type":"text", "text":"**Usage**: error warning"}],
                "details":{"severity":"error", "data":{"tokens":123}}
            }]}}),
        );
        for (id, notify_type) in [
            ("info", "info"),
            ("warning", "warning"),
            ("error", "error"),
            ("unknown", "warn"),
        ] {
            controller.handle(json!({"type":"extension_ui_request", "method":"notify",
                "id":id, "notifyType":notify_type, "message":"**Body**: error warning"}));
        }
        controller.handle(json!({"type":"message_end", "message":{
            "role":"assistant", "timestamp":2, "stopReason":"error", "errorMessage":"provider secret detail",
            "content":[]
        }}));
        controller.handle(json!({"type":"message_end", "message":{
            "role":"assistant", "timestamp":3, "content":[{"type":"text", "text":"Ordinary **reply**: error warning"}]
        }}));
        assert_eq!(controller.state().transcript.len(), 7);
        let source = controller.state().transcript.clone();
        let (mut app, mut window, transcript) = fixture(dark);
        // A fifth-pixel lattice preserves the authored 10.8px leading in measured
        // boxes; Retina's half-pixel lattice rounds that box to 11px.
        window.simulate_scale_factor_change(scale);
        let host = app.new_entity(|_| ());
        app.update(|cx| {
            host.update(cx, |_, cx| {
                // Each theme is a fresh runtime; reconcile the previous fixture's projection.
                sync(&[], &transcript, cx);
                sync(&source, &transcript, cx);
            })
        });
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        let accessible_text = tree
            .nodes
            .iter()
            .filter_map(|(_, n)| n.value())
            .collect::<Vec<_>>()
            .join("\n");
        for label in [
            "Info",
            "Warning",
            "Error",
            "Reply Failed",
            "Usage",
            "Body",
            "Ordinary",
        ] {
            assert!(
                accessible_text.contains(label),
                "missing accessible {label}: {accessible_text:?}"
            );
        }
        assert!(!accessible_text.contains("provider secret detail"));
        for body in [
            "Usage: error warning",
            "Body: error warning",
            "Ordinary reply: error warning",
        ] {
            let node = tree
                .nodes
                .iter()
                .find(|(_, node)| node.value().is_some_and(|value| value.trim() == body))
                .unwrap_or_else(|| panic!("missing rendered text AX leaf: {body}"));
            assert_eq!(node.1.role(), gpui_kit::accesskit::Role::Label);
            let bounds = node.1.bounds().unwrap();
            assert!(bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0);
        }
        window.read(|_, cx| {
            let t = transcript.read(cx);
            for (index, expected) in [
                (0, "Usage: error warning"),
                (1, "Body: error warning"),
                (6, "Ordinary reply: error warning"),
            ] {
                let View::Prose(text, _) = &t.cache[&t.rows[index].key].view else {
                    panic!("missing source")
                };
                let text = text.read(cx);
                assert_eq!(text.rendered_text().as_str().trim(), expected);
                let bounds = text.bounds();
                assert!(
                    (0..=(f32::from(bounds.size.width) * 4.) as usize).any(|step| {
                        text.rendered_offset_at(point(
                            bounds.left() + px(step as f32 / 4.),
                            bounds.top() + px(9.),
                        )) == Some(1)
                    }),
                    "missing shaped source glyph hit: {expected}"
                );
            }
        });
        let body_ax_ids = tree
            .nodes
            .iter()
            .filter_map(|(id, node)| (node.value() == Some("Body: error warning")).then_some(*id))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            body_ax_ids.len(),
            4,
            "each rendered source needs its own AX identity"
        );
        window.update(|_, w, cx| {
            let t = transcript.read(cx);
            for (index, severity) in [
                NoticeSeverity::Info,
                NoticeSeverity::Info,
                NoticeSeverity::Warning,
                NoticeSeverity::Error,
                NoticeSeverity::Info,
                NoticeSeverity::Error,
            ]
            .into_iter()
            .enumerate()
            {
                let key = t.rows[index].key;
                let TranscriptContent::Notice(notice) = &t.rows[index].entry.content else {
                    panic!("notice fell through to ordinary prose")
                };
                assert_eq!(notice.severity, severity);
                assert_eq!(notice.inline, index == 5);
                assert_eq!(t.rows[index].entry.id, source[index].id);
                let row = w.find(format!("transcript-row-{key}")).bounds();
                let card = w.find(format!("transcript-notice-{key}")).bounds();
                let label = w.find(format!("transcript-notice-label-{key}")).bounds();
                let body = w.find(format!("transcript-notice-body-{key}")).bounds();
                assert_eq!(card.left(), row.left() + px(58.));
                assert!((f32::from(row.bottom() - card.bottom()) - 22.).abs() < 0.0001);
                assert_eq!(label.left() - card.left(), px(11.));
                assert_eq!(label.top() - card.top(), px(8.));
                // Exact authored leading at 5x, and its actual device-snapped box at Retina.
                let expected_label_height = if scale == 5. { 10.8 } else { 11. };
                assert!(
                    (f32::from(label.size.height) - expected_label_height).abs() <= 0.1,
                    "label height: {:?}",
                    label.size.height
                );
                assert!((f32::from(body.top() - label.bottom()) - 2.).abs() <= 0.1);
                assert!((f32::from(card.bottom() - body.bottom()) - 8.).abs() < 0.0001);
                let expected_tone = match severity {
                    NoticeSeverity::Info => support::tokens(dark).muted,
                    NoticeSeverity::Warning => support::tokens(dark).accent,
                    NoticeSeverity::Error => support::tokens(dark).danger,
                };
                let tone = rgb(expected_tone);
                let raised = rgb(support::tokens(dark).panel_raised);
                let expected = gpui_kit::Rgba {
                    r: tone.r * 0.07 + raised.r * 0.93,
                    g: tone.g * 0.07 + raised.g * 0.93,
                    b: tone.b * 0.07 + raised.b * 0.93,
                    a: 1.,
                };
                assert_eq!(
                    quad_at(w, &format!("transcript-notice-{key}")),
                    expected.into()
                );
            }
            assert!(matches!(
                t.rows[6].entry.content,
                TranscriptContent::Assistant(_)
            ));
        });
        // Select the actual rendered body using glyph hits, not a synthesized domain range.
        let (start, end, key) = window.read(|_, cx| {
            let t = transcript.read(cx);
            let key = t.rows[1].key;
            let View::Prose(text, _) = &t.cache[&key].view else {
                panic!()
            };
            let text = text.read(cx);
            let bounds = text.bounds();
            let hit = |offset| {
                for step in 0..=(f32::from(bounds.size.width) * 4.) as usize {
                    let p = point(bounds.left() + px(step as f32 / 4.), bounds.top() + px(9.));
                    if text.rendered_offset_at(p) == Some(offset) {
                        return p;
                    }
                }
                panic!("no glyph hit for {offset}")
            };
            (hit(0), hit(4), key)
        });
        window.simulate_mouse_move(start);
        window.simulate_mouse_down(start, gpui_kit::MouseButton::Left);
        window.simulate_event(gpui_kit::MouseMoveEvent {
            position: end,
            pressed_button: Some(gpui_kit::MouseButton::Left),
            modifiers: Default::default(),
        });
        window.simulate_mouse_up(end, gpui_kit::MouseButton::Left);
        window.draw();
        assert_eq!(copy(&mut window), "Body");
        let mounted = window.read(|_, cx| {
            let t = transcript.read(cx);
            let View::Prose(text, _) = &t.cache[&key].view else {
                panic!()
            };
            text.clone()
        });
        let mut changed = source.clone();
        changed[1].notice_type = tau_session::transcript::NoticeType::Warning;
        changed[1].revision += 1;
        window.update(|_, _, cx| host.update(cx, |_, cx| sync(&changed, &transcript, cx)));
        window.draw();
        let updated_tree = window.accessibility_tree().unwrap();
        let updated_body_ax_ids = updated_tree
            .nodes
            .iter()
            .filter_map(|(id, node)| (node.value() == Some("Body: error warning")).then_some(*id))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(updated_body_ax_ids, body_ax_ids);
        assert_eq!(copy(&mut window), "Body");
        window.read(|_, cx| {
            let t = transcript.read(cx);
            let View::Prose(text, _) = &t.cache[&key].view else {
                panic!()
            };
            assert_eq!(*text, mounted);
            assert_eq!(t.rows[1].key, key);
        });
        // Offscreen eviction does not discard the selected source or exceed cache128.
        window.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                for index in 0..300 {
                    t.append_entry(
                        TranscriptEntry {
                            id: format!("history-{index}"),
                            content: TranscriptContent::Assistant("history".into()),
                        },
                        cx,
                    )
                    .unwrap();
                }
                let owner = cx.entity().downgrade();
                for index in 0..t.len() {
                    t.mount(index, owner.clone(), cx);
                }
                assert!(t.cache.len() <= CACHE_LIMIT);
                assert!(!t.cache.contains_key(&key));
                assert!(t.selection.stored_range().is_some());
            })
        });
        assert_eq!(copy(&mut window), "Body");
    }
}

#[test]
fn production_divider_hover_and_focus_paint_rules_without_layout_shift() {
    for dark in [false, true] {
        for available in [false, true] {
            let (_, mut window, transcript) = fixture(dark);
            window.update(|_, _, cx| {
                transcript.update(cx, |t, cx| {
                    t.append_entry(
                        TranscriptEntry {
                            id: "boundary".into(),
                            content: TranscriptContent::Compaction {
                                available,
                                loading: false,
                            },
                        },
                        cx,
                    )
                    .unwrap();
                })
            });
            window.draw();
            let (control, idle) = window.update(|_, w, _| {
                (
                    w.find("compaction-0").bounds(),
                    quad_at(w, "compaction-rule-left-0"),
                )
            });
            window.simulate_mouse_move(control.center());
            window.draw();
            let hover = window.update(|_, w, _| {
                assert_eq!(w.find("compaction-0").bounds(), control);
                quad_at(w, "compaction-rule-left-0")
            });
            if available {
                assert_ne!(hover, idle);
            } else {
                assert_eq!(hover, idle);
            }
            window.simulate_mouse_move(point(px(990.), px(880.)));
            window.simulate_keystroke("tab");
            window.draw();
            window.update(|_, w, _| {
                assert_eq!(w.find("compaction-0").bounds(), control);
                let focused = quad_at(w, "compaction-rule-left-0");
                if available {
                    assert_eq!(w.find("compaction-0").focused(), Some(true));
                    assert_eq!(focused, hover);
                } else {
                    assert_eq!(focused, idle);
                }
            });
        }
    }
}

pub(crate) fn production_compaction_state(
    mut sync: impl FnMut(&tau_session::session::State, &Entity<Transcript>, &mut Context<()>),
) {
    let mut controller = Controller::new("compaction-state");
    controller.begin_generation(1);
    let (mut app, mut window, transcript) = fixture(true);
    let host = app.new_entity(|_| ());
    controller.handle(json!({"type":"compaction_start"}));
    app.update(|cx| host.update(cx, |_, cx| sync(controller.state(), &transcript, cx)));
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert!(
        tree.nodes
            .iter()
            .filter_map(|(_, n)| n.value())
            .any(|value| value == "compacting")
    );
    window.read(|_, cx| assert!(transcript.read(cx).rows.is_empty()));
    let effects = controller
        .handle(json!({"type":"compaction_end", "result":{"summary":"Model context, not prose"}}));
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(controller.state(), &transcript, cx)));
    let request = effects
        .iter()
        .find_map(|effect| match effect {
            tau_session::session::Effect::Send(request) if request["type"] == "get_messages" => {
                Some(request)
            }
            _ => None,
        })
        .unwrap();
    controller.handle(json!({"type":"response", "command":"get_messages", "id":request["id"],
        "success":true, "data":{"messages":[{"role":"compactionSummary", "summary":"Model context, not prose"}]}}));
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(controller.state(), &transcript, cx)));
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let text = tree
        .nodes
        .iter()
        .filter_map(|(_, n)| n.value())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("compacted"));
    assert!(!text.contains("Model context"));
    window.read(|_, cx| {
        let t = transcript.read(cx);
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.compaction_pulse.map(|(key, _)| key), Some(t.rows[0].key));
        assert!(t.cache.is_empty());
        assert_eq!(t.prose_index(t.rows[0].key), None);
    });
}

#[test]
fn css_loading_and_pulse_curves_match_the_production_keyframes() {
    assert!(css_ease(0., 0.42, 0., 0.58, 1.).abs() < 0.0001);
    assert!((css_ease(0.5, 0.42, 0., 0.58, 1.) - 0.5).abs() < 0.0001);
    assert!((css_ease(1., 0.42, 0., 0.58, 1.) - 1.).abs() < 0.0001);
    // CSS ease-out (0, 0, .58, 1), not a decorative linear/cosine substitute.
    assert!((css_ease(0.5, 0., 0., 0.58, 1.) - 0.6846432).abs() < 0.0001);
}

#[test]
fn busy_divider_keeps_disabled_opacity_and_hover_after_the_loading_delay() {
    let (mut app, mut window, transcript) = fixture(true);
    app.update(|cx| cx.set_reduce_motion(true));
    window.update(|_, _, cx| {
        transcript.update(cx, |t, cx| {
            t.append_entry(
                TranscriptEntry {
                    id: "busy".into(),
                    content: TranscriptContent::Compaction {
                        available: true,
                        loading: true,
                    },
                },
                cx,
            )
            .unwrap();
        })
    });
    window.draw();
    let control = window.update(|_, w, _| w.find("compaction-0").bounds());
    let initial_quads = window.update(|_, w, _| w.painted_quads().len());
    window.update(|_, _, cx| {
        transcript.update(cx, |t, cx| {
            t.rows[0].loading_started =
                Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
            cx.notify();
        })
    });
    window.draw();
    window.update(|_, w, _| assert!(w.painted_quads().len() > initial_quads));
    window.simulate_mouse_move(control.center());
    window.draw();
    window.update(|_, w, _| {
        let mut color = rgb(support::tokens(true).muted);
        color.a = 0.55 * 0.45;
        assert_eq!(quad_at(w, "compaction-rule-left-0"), color.into());
        assert_eq!(w.find("compaction-0").bounds(), control);
    });
    let tree = window.accessibility_tree().unwrap();
    let button = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == gpui_kit::accesskit::Role::Button)
        .unwrap();
    assert!(button.1.is_busy() && button.1.is_disabled());
    window.simulate_click(control.center(), gpui_kit::MouseButton::Left);
    window.read(|_, cx| {
        let t = transcript.read(cx);
        assert!(!t.rows[0].history_requested);
        assert!(t.cache.is_empty());
    });
}

#[test]
fn transient_compaction_and_new_boundary_pulse_never_become_copy_sources() {
    let (_, mut window, transcript) = fixture(true);
    window.update(|_, _, cx| transcript.update(cx, |t, cx| t.set_compacting(true, cx)));
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert!(
        tree.nodes
            .iter()
            .filter_map(|(_, n)| n.value())
            .any(|value| value == "compacting")
    );
    window.read(|_, cx| {
        let t = transcript.read(cx);
        assert!(t.rows.is_empty());
        assert!(t.cache.is_empty());
        assert!(t.selection.stored_range().is_none());
    });
    window.update(|_, _, cx| {
        transcript.update(cx, |t, cx| {
            t.set_compacting(false, cx);
            assert!(t.awaiting_compaction);
            t.append_entry(
                TranscriptEntry {
                    id: "new-boundary".into(),
                    content: TranscriptContent::Compaction {
                        available: true,
                        loading: false,
                    },
                },
                cx,
            )
            .unwrap();
            assert_eq!(t.compaction_pulse.map(|(key, _)| key), Some(t.rows[0].key));
            assert!(!t.awaiting_compaction);
        })
    });
    window.draw();
    window.read(|_, cx| assert!(transcript.read(cx).cache.is_empty()));
    window.update(|_, _, cx| {
        transcript.update(cx, |t, cx| {
            t.reset(cx);
            assert!(t.compaction_pulse.is_none());
            assert!(!t.awaiting_compaction);
        })
    });
}
