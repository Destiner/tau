use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    TestApp,
    base::{Root, TextSelectionDomainEndpoint},
    point, size,
};

fn entry(id: impl Into<String>, text: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: id.into(),
        content: TranscriptContent::Assistant(text.into()),
    }
}
fn range(a: u64, start: usize, b: u64, end: usize) -> TextSelectionDomainRange {
    TextSelectionDomainRange {
        anchor: TextSelectionDomainEndpoint {
            item: a,
            offset: start,
        },
        cursor: TextSelectionDomainEndpoint {
            item: b,
            offset: end,
        },
    }
}
fn app() -> TestApp {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, true));
    app
}
#[test]
fn pending_acknowledgment_preserves_mounted_markdown_selection_and_source() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        let owner = transcript.downgrade();
        transcript.update(cx, |this, cx| {
            this.append_entry(
                TranscriptEntry {
                    id: "user".into(),
                    content: TranscriptContent::PendingUser("**draft**".into()),
                },
                cx,
            )
            .unwrap();
            let key = this.rows[0].key;
            let Some(View::Prose(before, _)) = this.mount(0, owner.clone(), cx) else {
                panic!()
            };
            assert_eq!(this.copy_selection(range(key, 0, key, 5)), "draft");
            this.update_content("user", TranscriptContent::User("**draft**".into()), cx);
            let Some(View::Prose(after, _)) = this.mount(0, owner, cx) else {
                panic!()
            };
            assert_eq!(before, after);
            assert_eq!(this.rows[0].key, key);
            assert!(matches!(
                this.rows[0].entry.content,
                TranscriptContent::User(_)
            ));
            assert_eq!(this.copy_selection(range(key, 0, key, 5)), "draft");
        });
    });
}

#[test]
fn compaction_dividers_have_no_copy_source_or_cache_and_reveals_keep_tail_keys() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        let owner = transcript.downgrade();
        transcript.update(cx, |this, cx| {
            let boundary = || TranscriptEntry {
                id: "compacted".into(),
                content: TranscriptContent::Compaction {
                    available: true,
                    loading: false,
                },
            };
            this.set_entries(vec![boundary(), entry("current", "current")], cx)
                .unwrap();
            let tail_key = this.rows[1].key;
            assert!(matches!(
                this.mount(0, owner.clone(), cx),
                Some(View::Boundary { .. })
            ));
            assert!(this.cache.is_empty());
            assert_eq!(this.prose_index(this.rows[0].key), None);
            this.set_entries(
                vec![entry("old", "old"), boundary(), entry("current", "current")],
                cx,
            )
            .unwrap();
            assert_eq!(this.rows[2].key, tail_key);
            let old_key = this.rows[0].key;
            assert_eq!(
                this.copy_selection(range(old_key, 0, tail_key, 7)),
                "old\n\ncurrent"
            );
            for index in 0..300 {
                this.append_entry(entry(format!("tail-{index}"), "source"), cx)
                    .unwrap();
            }
            for index in 0..this.len() {
                this.mount(index, owner.clone(), cx).unwrap();
            }
            assert!(this.cache.len() <= CACHE_LIMIT);
            assert_eq!(
                this.copy_selection(range(old_key, 0, tail_key, 7)),
                "old\n\ncurrent"
            );
        });
    });
}

#[test]
fn compaction_busy_feedback_is_delayed_and_retry_restarts_the_delay() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        let owner = transcript.downgrade();
        transcript.update(cx, |this, cx| {
            let content = |loading| TranscriptContent::Compaction {
                available: true,
                loading,
            };
            this.append_entry(
                TranscriptEntry {
                    id: "boundary".into(),
                    content: content(true),
                },
                cx,
            )
            .unwrap();
            assert!(matches!(
                this.mount(0, owner.clone(), cx),
                Some(View::Boundary {
                    loading: true,
                    show_loading: false,
                    ..
                })
            ));
            this.rows[0].loading_started =
                Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
            assert!(matches!(
                this.mount(0, owner.clone(), cx),
                Some(View::Boundary {
                    show_loading: true,
                    ..
                })
            ));
            this.update_content("boundary", content(false), cx);
            this.update_content("boundary", content(true), cx);
            assert!(matches!(
                this.mount(0, owner, cx),
                Some(View::Boundary {
                    show_loading: false,
                    ..
                })
            ));
            assert!(this.cache.is_empty());
        });
    });
}

fn boundary(available: bool, loading: bool) -> TranscriptEntry {
    TranscriptEntry {
        id: "boundary".into(),
        content: TranscriptContent::Compaction { available, loading },
    }
}

fn divider_fixture(
    available: bool,
    loading: bool,
) -> (TestApp, gpui_kit::TestAppWindow<Root>, Entity<Transcript>) {
    let mut app = app();
    let transcript = app.new_entity(|cx| {
        let mut t = Transcript::new("transcript", true, cx);
        t.set_entries(vec![boundary(available, loading)], cx)
            .unwrap();
        t
    });
    let mut window =
        app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
    window.simulate_resize(size(px(1000.), px(700.)));
    window.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    app.run_until_parked();
    window.activate_accessibility();
    window.draw();
    (app, window, transcript)
}

#[test]
fn painted_compaction_available_loading_and_unavailable_match_vue_geometry_and_semantics() {
    for (available, loading) in [(true, false), (true, true), (false, false)] {
        let (_, mut window, transcript) = divider_fixture(available, loading);
        for delayed in [false, true] {
            if delayed && loading {
                window.update(|_, _, cx| {
                    transcript.update(cx, |t, cx| {
                        t.rows[0].loading_started =
                            Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
                        cx.notify();
                    })
                });
                window.draw();
            }
            window.update(|_, w, _| {
                let row = w.find("transcript-row-0").bounds();
                let control = w.find("compaction-0").bounds();
                let label = w.find("compaction-label-0").bounds();
                assert_eq!(row.size.height, px(54.));
                assert_eq!(control.size.height, px(22.));
                assert_eq!(control.top() - row.top(), px(16.));
                assert_eq!(row.bottom() - control.bottom(), px(16.));
                assert_eq!(label.size.height, px(22.));
                for side in ["left", "right"] {
                    let rule = w.find(format!("compaction-rule-{side}-0")).bounds();
                    assert_eq!(rule.size.height, px(1.));
                    assert_eq!(rule.center().y, control.center().y);
                }
            });
            let tree = window.accessibility_tree().unwrap();
            let buttons = tree
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == gpui_kit::accesskit::Role::Button)
                .collect::<Vec<_>>();
            assert_eq!(buttons.len(), usize::from(available));
            if available {
                assert_eq!(buttons[0].1.label(), Some("Load earlier messages"));
                assert_eq!(buttons[0].1.is_disabled(), loading);
                assert_eq!(buttons[0].1.is_busy(), loading);
            }
            let text = tree
                .nodes
                .iter()
                .filter_map(|(_, n)| n.value())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("compacted"), "painted label: {text:?}");
            assert!(!text.contains("loading earlier messages"));
            window.read(|_, cx| {
                let t = transcript.read(cx);
                assert!(t.cache.is_empty());
                assert!(t.recent.is_empty());
                assert_eq!(t.prose_index(t.rows[0].key), None);
                assert_eq!(t.rows[0].entry.content.text(), None);
                assert_eq!(
                    t.copy_selection(range(t.rows[0].key, 0, t.rows[0].key, 100)),
                    ""
                );
            });
            assert_eq!(root_copy(&mut window), "copy sentinel");
        }
    }
}

#[test]
fn painted_compaction_keyboard_and_pointer_gate_duplicates_and_recheck_live_state() {
    use std::{cell::RefCell, rc::Rc};
    for keyboard in ["enter", "space"] {
        let press = |window: &mut gpui_kit::TestAppWindow<Root>| {
            window.simulate_keystroke(keyboard);
            window.simulate_event(gpui_kit::KeyUpEvent {
                keystroke: gpui_kit::Keystroke::parse(keyboard).unwrap(),
            });
        };
        let (mut app, mut window, transcript) = divider_fixture(true, false);
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = events.clone();
        let _subscription = app.update(|cx| {
            cx.subscribe(&transcript, move |_, event: &TranscriptEvent, _| {
                if let TranscriptEvent::LoadHistory(id) = event {
                    observed.borrow_mut().push(id.clone());
                }
            })
        });
        window.simulate_keystroke("tab");
        window.draw();
        window.update(|_, w, _| assert_eq!(w.find("compaction-0").focused(), Some(true)));
        let control = window.update(|_, w, _| w.find("compaction-0").bounds().center());
        // No intervening paint: both keyboard and pointer use the old handler.
        press(&mut window);
        press(&mut window);
        window.simulate_click(control, gpui_kit::MouseButton::Left);
        window.draw();
        assert_eq!(*events.borrow(), vec!["boundary"]);
        window.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                t.update_content("boundary", boundary(true, true).content, cx);
            })
        });
        window.draw();
        window.simulate_click(control, gpui_kit::MouseButton::Left);
        press(&mut window);
        assert_eq!(events.borrow().len(), 1);
        // A failed request's authoritative idle state allows a retry.
        window.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                t.update_content("boundary", boundary(true, false).content, cx);
            })
        });
        window.draw();
        window.simulate_click(control, gpui_kit::MouseButton::Left);
        press(&mut window);
        window.draw();
        assert_eq!(events.borrow().len(), 2);
        // Availability can disappear before the old control's handler is repainted.
        window.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                t.update_content("boundary", boundary(false, false).content, cx);
            })
        });
        window.simulate_click(control, gpui_kit::MouseButton::Left);
        press(&mut window);
        window.draw();
        assert_eq!(events.borrow().len(), 2);
        window.simulate_click(control, gpui_kit::MouseButton::Left);
        press(&mut window);
        assert_eq!(events.borrow().len(), 2);
        window.read(|_, cx| {
            let t = transcript.read(cx);
            assert_eq!(t.rows[0].key, 0);
            assert!(t.cache.is_empty());
        });
    }
}

#[test]
fn append_keeps_keys_and_does_not_mount_offscreen_prose() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        transcript.update(cx, |this, cx| {
            this.append_entry(entry("a", "alpha"), cx).unwrap();
            let key = this.rows[0].key;
            for ix in 0..10_000 {
                this.append_entry(entry(format!("history-{ix}"), "history"), cx)
                    .unwrap();
            }
            assert!(this.append_text("a", " beta", cx));
            assert_eq!(this.rows[0].key, key);
            assert_eq!(this.rows[0].entry.content.text(), Some("alpha beta"));
            assert!(this.cache.is_empty());
            assert_eq!(this.len(), 10_001);
            assert!(!this.append_text("missing", "ignored", cx));
        });
    });
}
#[test]
fn reconciliation_preserves_identity_and_retires_removed_keys() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        transcript.update(cx, |this, cx| {
            this.set_entries(vec![entry("a", "alpha"), entry("b", "beta")], cx)
                .unwrap();
            let (a, b) = (this.rows[0].key, this.rows[1].key);
            this.set_entries(vec![entry("b", "beta plus"), entry("a", "alpha")], cx)
                .unwrap();
            assert_eq!((this.rows[0].key, this.rows[1].key), (b, a));
            this.set_entries(vec![entry("b", "beta plus")], cx).unwrap();
            assert_eq!(this.prose_index(a), None);
            this.append_entry(entry("a", "new identity"), cx).unwrap();
            assert_ne!(this.rows[1].key, a);
            this.reset(cx);
            assert!(this.is_empty());
            assert!(this.is_following_tail());
            this.append_entry(entry("b", "new session"), cx).unwrap();
            assert_ne!(this.rows[0].key, b);
        });
    });
}
#[test]
fn duplicate_ids_are_rejected_atomically() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        transcript.update(cx, |this, cx| {
            this.append_entry(entry("a", "original"), cx).unwrap();
            let key = this.rows[0].key;
            assert!(
                this.set_entries(vec![entry("b", "one"), entry("b", "two")], cx)
                    .is_err()
            );
            assert!(this.append_entry(entry("a", "duplicate"), cx).is_err());
            assert_eq!(this.len(), 1);
            assert_eq!(this.rows[0].key, key);
            assert_eq!(this.rows[0].entry.content.text(), Some("original"));
        });
    });
}
#[test]
fn cache_is_bounded_and_directed_copy_uses_evicted_current_source() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        let owner = transcript.downgrade();
        transcript.update(cx, |this, cx| {
            this.set_entries(
                (0..300)
                    .map(|ix| entry(ix.to_string(), "**alpha**"))
                    .collect(),
                cx,
            )
            .unwrap();
            let (a, b) = (this.rows[0].key, this.rows[1].key);
            for ix in 0..300 {
                this.mount(ix, owner.clone(), cx).unwrap();
            }
            assert_eq!(this.cache.len(), CACHE_LIMIT);
            assert_eq!(this.recent.len(), CACHE_LIMIT);
            assert!(!this.cache.contains_key(&a));
            assert!(!this.cache.contains_key(&b));
            assert_eq!(this.copy_selection(range(a, 0, b, 5)), "alpha\n\nalpha");
            assert_eq!(this.copy_selection(range(b, 5, a, 0)), "alpha\n\nalpha");
            this.append_text("0", " current", cx);
            assert_eq!(
                this.copy_selection(range(a, 0, b, 5)),
                "alpha current\n\nalpha"
            );
            assert!(!this.cache.contains_key(&a));
        });
    });
}
#[test]
fn tool_disclosure_survives_eviction_and_content_refresh() {
    let mut app = app();
    app.update(|cx| {
        let transcript = cx.new(|cx| Transcript::new("transcript", true, cx));
        let owner = transcript.downgrade();
        transcript.update(cx, |this, cx| {
            let thinking = |text: &str| TranscriptEntry {
                id: "thinking".into(),
                content: TranscriptContent::Activity(ActivityContent::Thinking {
                    text: text.into(),
                }),
            };
            this.append_entry(thinking("working"), cx).unwrap();
            let View::Activity(activity) = this.mount(0, owner.clone(), cx).unwrap() else {
                panic!()
            };
            activity.update(cx, |activity, cx| activity.toggle(cx));
            for ix in 0..130 {
                this.append_entry(entry(ix.to_string(), "prose"), cx)
                    .unwrap();
                this.mount(ix + 1, owner.clone(), cx).unwrap();
            }
            assert!(this.rows[0].expanded);
            let View::Activity(activity) = this.mount(0, owner.clone(), cx).unwrap() else {
                panic!()
            };
            assert!(activity.read(cx).expanded());
            let key = this.rows[0].key;
            let pointer = this.rows[1].entry.content.text().unwrap().as_ptr();
            assert!(this.update_content("thinking", thinking("streaming").content, cx));
            let View::Activity(updated) = this.mount(0, owner.clone(), cx).unwrap() else {
                panic!()
            };
            assert_eq!(updated, activity);
            assert!(updated.read(cx).expanded());
            assert_eq!(this.rows[0].key, key);
            assert_eq!(this.rows[1].entry.content.text().unwrap().as_ptr(), pointer);
            let mut entries: Vec<_> = this.rows.iter().map(|row| row.entry.clone()).collect();
            entries[0] = thinking("finished");
            this.set_entries(entries, cx).unwrap();
            let View::Activity(activity) = this.mount(0, owner, cx).unwrap() else {
                panic!()
            };
            assert!(activity.read(cx).expanded());
            assert_eq!(
                this.copy_selection(range(this.rows[1].key, 0, this.rows[2].key, 5)),
                "prose\n\nprose"
            );
        });
    });
}

pub(crate) fn production_projection_matches_vue_activity_geometry_and_remeasures_neighbors(
    mut sync: impl FnMut(&[tau_session::transcript::Entry], &Entity<Transcript>, &mut Context<()>),
) {
    use serde_json::json;
    use tau_session::session::Controller;

    let mut controller = Controller::new("geometry");
    controller.begin_generation(1);
    controller.handle(json!({"type":"message_start", "message":{
        "role":"assistant", "timestamp":1, "content":[
            {"type":"thinking", "thinking":"Plan"},
            {"type":"toolCall", "id":"read", "name":"read", "arguments":{"path":"gpui/APP.md"}},
            {"type":"text", "text":"Reply"}
        ]
    }}));
    let mut app = app();
    let host = app.new_entity(|_| ());
    let transcript = app.new_entity(|cx| Transcript::new("transcript", true, cx));
    app.update(|cx| {
        host.update(cx, |_, cx| {
            sync(&controller.state().transcript, &transcript, cx)
        })
    });
    let mut window =
        app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
    window.simulate_resize(size(px(1000.), px(700.)));
    window.update(|_, w, cx| w.bounds_changed(cx));
    window.draw();
    window.update(|_, w, _| {
        let thinking = w.find("transcript-row-0").bounds();
        let tool = w.find("transcript-row-1").bounds();
        let text = w.find("transcript-row-2").bounds();
        let header = w.find("transcript-activity-1").bounds();
        // Vue: 10px * 1.25 + 2 * 3px; compact padding 0, otherwise 22px.
        assert_eq!(thinking.size.height, px(18.5));
        assert_eq!(tool.size.height, px(40.5));
        assert_eq!(tool.top(), thinking.bottom());
        assert_eq!(text.top() - header.bottom(), px(22.));
    });
    app.update(|cx| {
        transcript.update(cx, |t, _| {
            let TranscriptContent::Activity(ActivityContent::Tool { name, argument, .. }) =
                &t.rows[1].entry.content
            else {
                panic!()
            };
            assert_eq!((name.as_str(), argument.as_str()), ("read", "gpui/APP.md"));
        })
    });

    let mut source = controller.state().transcript.clone();
    // Reconcile away the tool: unchanged thinking must regain its 22px padding.
    source.remove(1);
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
    window.draw();
    window.update(|_, w, _| assert_eq!(w.find("transcript-row-0").bounds().size.height, px(40.5)));
    // Append an activity after the remaining thinking tail, then change its kind.
    source.pop();
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
    source.push(controller.state().transcript[1].clone());
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
    window.draw();
    window.update(|_, w, _| assert_eq!(w.find("transcript-row-0").bounds().size.height, px(18.5)));
    source[1].kind = "assistant".into();
    source[1].text = "Result".into();
    source[1].revision += 1;
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
    window.draw();
    window.update(|_, w, _| assert_eq!(w.find("transcript-row-0").bounds().size.height, px(40.5)));

    source[1].kind = "tool".into();
    source[1].text.clear();
    source[1].result = "raw result  \n".into();
    let key = app.update(|cx| transcript.read(cx).rows[1].key);
    for (name, args, expected) in [
        ("read", json!({"path":"gpui/APP.md"}), "gpui/APP.md"),
        ("bash", json!({"command":"bun run build"}), "bun run build"),
        (
            "edit",
            json!({"path":"file.rs", "oldText":"raw  \n", "newText":"new"}),
            "file.rs",
        ),
        ("tool", json!(null), ""),
        ("tool", json!(false), "false"),
    ] {
        source[1].tool_name = Some(name.into());
        source[1].arguments = args.clone();
        source[1].revision += 1;
        window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
        window.draw();
        window.read(|_, cx| {
            let t = transcript.read(cx);
            assert_eq!(t.rows[1].key, key);
            let TranscriptContent::Activity(ActivityContent::Tool {
                name: label,
                argument,
                result,
                ..
            }) = &t.rows[1].entry.content
            else {
                panic!()
            };
            assert_eq!((label.as_str(), argument.as_str()), (name, expected));
            assert_eq!(result, "raw result  \n");
            assert!(t.cache.len() <= CACHE_LIMIT);
        });
        assert_eq!(source[1].arguments, args);
    }
    let mut thinking = source[0].clone();
    thinking.id = "second-thinking".into();
    let mut tool = source[1].clone();
    tool.id = "second-tool".into();
    source = vec![
        source[0].clone(),
        thinking,
        source[1].clone(),
        tool,
        controller.state().transcript[2].clone(),
    ];
    window.update(|_, _, cx| host.update(cx, |_, cx| sync(&source, &transcript, cx)));
    window.draw();
    let keys = window.read(|_, cx| {
        transcript
            .read(cx)
            .rows
            .iter()
            .map(|row| row.key)
            .collect::<Vec<_>>()
    });
    window.update(|_, w, _| {
        let bounds = keys
            .iter()
            .map(|key| w.find(format!("transcript-row-{key}")).bounds())
            .collect::<Vec<_>>();
        // thinking → thinking → tool → tool → text has only one 22px gap.
        for ix in 0..3 {
            assert_eq!(bounds[ix].size.height, px(18.5));
            assert_eq!(bounds[ix].bottom(), bounds[ix + 1].top());
        }
        assert_eq!(bounds[3].size.height, px(40.5));
        assert_eq!(bounds[3].bottom(), bounds[4].top());
    });

    // Exercise the real production projection and Root, not just mount's enum.
    // Even a legacy/source row carrying rawSummary must remain a passive boundary.
    window.activate_accessibility();
    let raw_summary = "/private/file.rs\n<summary>rawSummary is not prose</summary>";
    let mut before = source[0].clone();
    before.id = "before-boundary".into();
    before.kind = "assistant".into();
    before.text = "before".into();
    let mut compact = source[1].clone();
    compact.id = "boundary".into();
    compact.kind = "compaction".into();
    compact.text = raw_summary.into();
    let mut after = before.clone();
    after.id = "after-boundary".into();
    after.text = "after".into();
    let mut stable_keys = None;
    let mut reader_bounds = None;
    for (available, loading) in [(true, false), (true, true), (false, false)] {
        compact.history_available = available;
        compact.history_loading = loading;
        compact.revision += 1;
        let rows = vec![before.clone(), compact.clone(), after.clone()];
        window.update(|_, _, cx| host.update(cx, |_, cx| sync(&rows, &transcript, cx)));
        window.draw();
        let keys = window.read(|_, cx| {
            let t = transcript.read(cx);
            let keys = t.rows.iter().map(|row| row.key).collect::<Vec<_>>();
            assert_eq!(t.rows[1].entry.content.text(), None);
            assert_eq!(t.prose_index(keys[1]), None);
            assert!(!t.cache.contains_key(&keys[1]));
            assert!(!t.recent.contains(&keys[1]));
            assert_eq!(
                t.copy_selection(range(keys[0], 0, keys[2], 5)),
                "before\n\nafter"
            );
            assert!(t.cache.len() <= CACHE_LIMIT);
            keys
        });
        if let Some(stable) = &stable_keys {
            assert_eq!(&keys, stable);
        } else {
            stable_keys = Some(keys.clone());
        }
        window.update(|_, w, _| {
            let row = w.find(format!("transcript-row-{}", keys[1])).bounds();
            assert_eq!(row.size.height, px(54.));
            assert_eq!(
                w.find(format!("compaction-{}", keys[1]))
                    .bounds()
                    .size
                    .height,
                px(22.)
            );
            assert_eq!(
                row.bottom(),
                w.find(format!("transcript-row-{}", keys[2])).bounds().top()
            );
        });
        let physical = window.update(|_, w, _| {
            (
                w.find(format!("transcript-row-{}", keys[0])).bounds(),
                w.find(format!("transcript-row-{}", keys[2])).bounds(),
            )
        });
        if let Some(bounds) = reader_bounds {
            assert_eq!(physical, bounds, "boundary state must not move the reader");
        } else {
            reader_bounds = Some(physical);
        }
        let tree = window.accessibility_tree().unwrap();
        let painted_text = tree
            .nodes
            .iter()
            .filter_map(|(_, n)| n.value())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(painted_text.contains("compacted"));
        assert!(!painted_text.contains("rawSummary"));
        assert!(!painted_text.contains("/private/file.rs"));
        assert_eq!(
            rows[1].text, raw_summary,
            "projection must not rewrite source"
        );
    }
}

pub(crate) fn production_working_tail(
    mut sync: impl FnMut(&tau_session::session::State, &Entity<Transcript>, &mut Context<()>),
) {
    use serde_json::json;
    use tau_session::session::Controller;
    let mut controller = Controller::new("working-render");
    controller.begin_generation(1);
    let mut app = app();
    let host = app.new_entity(|_| ());
    let transcript = app.new_entity(|cx| Transcript::new("transcript", true, cx));
    let mut window =
        app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
    window.activate_accessibility();
    for (event, expected) in [
        ("agent_start", Some("Working")),
        ("compaction_start", None),
        ("compaction_end", Some("Working")),
        ("abort", Some("Stopping")),
        ("agent_settled", None),
    ] {
        if event == "abort" {
            controller.abort();
        } else {
            controller.handle(json!({"type": event}));
        }
        window
            .update(|_, _, cx| host.update(cx, |_, cx| sync(controller.state(), &transcript, cx)));
        window.draw();
        window.read(|_, cx| {
            let this = transcript.read(cx);
            assert_eq!(this.working_indicator(), expected);
            assert_eq!(this.list.item_count(), usize::from(expected.is_some()));
            assert!(this.rows.is_empty());
            assert!(this.cache.is_empty());
            assert!(this.by_id.is_empty());
        });
        let tree = window.accessibility_tree().unwrap();
        let labels = tree
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == gpui_kit::accesskit::Role::Status)
            .filter_map(|(_, node)| node.label())
            .collect::<Vec<_>>();
        assert_eq!(labels, expected.into_iter().collect::<Vec<_>>());
        if expected.is_some() {
            window.update(|_, w, _| {
                assert_eq!(
                    w.find("tau-working-indicator").bounds().size.height,
                    px(30.)
                )
            });
        }
    }
    controller.handle(json!({"type":"agent_start"}));
    for (timestamp, content, expected) in [
        (
            1,
            json!({"type":"toolCall", "id":"read", "name":"read", "arguments":{"path":"file.rs"}}),
            Some("Working"),
        ),
        (2, json!({"type":"text", "text":"Reply"}), None),
    ] {
        controller.handle(json!({"type":"message_start", "message": {
            "role":"assistant", "timestamp": timestamp, "content":[content]
        }}));
        window
            .update(|_, _, cx| host.update(cx, |_, cx| sync(controller.state(), &transcript, cx)));
        window.draw();
        window.read(|_, cx| {
            let this = transcript.read(cx);
            assert_eq!(this.working_indicator(), expected);
            assert_eq!(
                this.list.item_count(),
                controller.state().transcript.len() + usize::from(expected.is_some())
            );
            assert!(
                this.rows
                    .iter()
                    .map(|row| row.entry.id.as_str())
                    .eq(controller
                        .state()
                        .transcript
                        .iter()
                        .map(|entry| entry.id.as_str()))
            );
            assert!(this.cache.len() <= CACHE_LIMIT);
        });
        let tree = window.accessibility_tree().unwrap();
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|(_, node)| node.role() == gpui_kit::accesskit::Role::Status)
                .count(),
            usize::from(expected.is_some())
        );
    }
}

fn fixture() -> (TestApp, gpui_kit::TestAppWindow<Root>, Entity<Transcript>) {
    let mut app = app();
    let mut transcript = None;
    let mut window = app.open_window(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = Transcript::new("transcript", true, cx);
            view.set_entries(
                (0..1000)
                    .map(|ix| {
                        entry(
                            ix.to_string(),
                            "A paragraph with enough text to read while later rows stream.",
                        )
                    })
                    .collect(),
                cx,
            )
            .unwrap();
            view
        });
        transcript = Some(view.clone());
        // Transcript fills a flex-column slot, as it does in the native shell.
        Root::new(view, window, cx).flex().flex_col()
    });
    window.simulate_resize(size(px(1000.), px(700.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    (app, window, transcript.unwrap())
}
#[test]
fn streaming_offscreen_and_append_preserve_physical_reader_position() {
    let (_, mut window, transcript) = fixture();
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.list.scroll_by(px(-1000.));
            cx.notify();
        });
    });
    for _ in 0..5 {
        window.draw();
    }
    let (key, before) = window.read(|_, cx| {
        let this = transcript.read(cx);
        assert!(!this.is_following_tail());
        let ix = this.list.logical_scroll_top().item_ix;
        let key = this.rows[ix].key;
        let View::Prose(text, _) = &this.cache[&key].view else {
            panic!()
        };
        (key, text.read(cx).bounds().top())
    });
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.append_text("999", " Offscreen streaming content.", cx);
            this.append_entry(entry("new", "A newly appended response."), cx)
                .unwrap();
        });
    });
    window.draw();
    window.read(|_, cx| {
        let this = transcript.read(cx);
        assert!(!this.is_following_tail());
        let View::Prose(text, _) = &this.cache[&key].view else {
            panic!()
        };
        assert!((f32::from(text.read(cx).bounds().top() - before)).abs() <= 1.);
        assert!(this.cache.len() <= CACHE_LIMIT);
    });
    window.update(|_, _, cx| transcript.update(cx, |this, cx| this.scroll_to_end(cx)));
    window.draw();
    window.read(|_, cx| assert!(transcript.read(cx).is_following_tail()));
}
#[test]
fn selected_source_survives_eviction_and_incompatible_markup_cancels_it() {
    let (_, mut window, transcript) = fixture();
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            let mut entries: Vec<_> = this.rows.iter().map(|row| row.entry.clone()).collect();
            entries[999] = entry("999", "**ending");
            this.set_entries(entries, cx).unwrap();
        });
    });
    window.draw();
    let bounds = window.read(|_, cx| {
        let this = transcript.read(cx);
        let key = this.rows[999].key;
        let View::Prose(text, _) = &this.cache[&key].view else {
            panic!()
        };
        text.read(cx).bounds()
    });
    let (start, end) = window.read(|_, cx| {
        let this = transcript.read(cx);
        let viewport = this.list.viewport_bounds();
        assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
        assert!(viewport.contains(&bounds.center()));
        let View::Prose(text, _) = &this.cache[&this.rows[999].key].view else {
            panic!()
        };
        let text = text.read(cx);
        assert_eq!(text.rendered_text().as_str().trim_end(), "**ending");
        // Resolve endpoints through painted glyph hit testing, not guessed row coordinates.
        let pointer = |offset| {
            for step in 0..=(f32::from(bounds.size.width) * 4.) as usize {
                let p = point(bounds.left() + px(step as f32 / 4.), bounds.center().y);
                if text.rendered_offset_at(p) == Some(offset) {
                    return p;
                }
            }
            panic!("no painted glyph hit for offset {offset} in {bounds:?}");
        };
        (pointer(0), pointer("**ending".len()))
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
    let selected = window.read(|_, cx| {
        transcript
            .read(cx)
            .selection
            .range(cx)
            .expect("actual pointer selection")
    });
    window.read(|_, cx| {
        let this = transcript.read(cx);
        assert_eq!(
            selected,
            range(this.rows[999].key, 0, this.rows[999].key, 8)
        );
        assert_eq!(this.copy_selection(selected), "**ending");
    });
    assert_eq!(root_copy(&mut window), "**ending");
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.set_working_indicator(Some("Working"), cx);
            this.set_working_indicator(Some("Stopping"), cx);
            this.set_working_indicator(None, cx);
            assert_eq!(this.selection.stored_range(), Some(selected));
            assert_eq!(this.copy_selection(selected), "**ending");
        })
    });
    window.draw();
    assert_eq!(root_copy(&mut window), "**ending");
    let viewport = window.read(|_, cx| transcript.read(cx).list.viewport_bounds());
    for _ in 0..30 {
        window.simulate_scroll(viewport.center(), point(px(0.), px(550.)));
        window.draw();
        if window.read(|_, cx| {
            !transcript
                .read(cx)
                .cache
                .contains_key(&selected.anchor.item)
        }) {
            break;
        }
    }
    window.read(|_, cx| {
        let this = transcript.read(cx);
        assert!(!this.cache.contains_key(&selected.anchor.item));
        assert!(this.cache.len() <= CACHE_LIMIT);
        assert_eq!(this.selection.range(cx), Some(selected));
    });
    assert_eq!(root_copy(&mut window), "**ending");
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            let expected = "**ending";
            assert_eq!(this.selection.stored_range(), Some(selected));
            assert_eq!(this.copy_selection(selected), expected);
            let activity = |text: &str| {
                TranscriptContent::Activity(ActivityContent::Thinking { text: text.into() })
            };
            this.append_entry(
                TranscriptEntry {
                    id: "activity".into(),
                    content: activity("working"),
                },
                cx,
            )
            .unwrap();
            this.update_content("activity", activity("more thinking"), cx);
            assert_eq!(this.selection.stored_range(), Some(selected));
            let mut entries: Vec<_> = this.rows.iter().map(|row| row.entry.clone()).collect();
            entries.last_mut().unwrap().content = activity("finished");
            this.set_entries(entries, cx).unwrap();
            assert_eq!(this.selection.stored_range(), Some(selected));
            this.update_content(
                "0",
                TranscriptContent::Assistant("unrelated prose replacement".into()),
                cx,
            );
            assert_eq!(this.selection.stored_range(), Some(selected));
            assert_eq!(this.copy_selection(selected), expected);
        });
    });
    assert_eq!(root_copy(&mut window), "**ending");
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            // Closing the delimiter changes the selected rendered prefix, even while cold.
            assert!(!this.cache.contains_key(&selected.anchor.item));
            this.append_text("999", "**", cx);
            assert!(this.selection.stored_range().is_none());
            assert!(this.selection.range(cx).is_none());
        });
    });
    assert_eq!(root_copy(&mut window), "copy sentinel");
}

fn root_copy(window: &mut gpui_kit::TestAppWindow<Root>) -> String {
    window.update(|_, _, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("copy sentinel".into()))
    });
    window.simulate_keystroke("cmd-c");
    window.draw();
    window.read(|_, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap()
    })
}

#[test]
fn expanded_tool_consumes_real_wheel_once_including_edges() {
    let (_, mut window, transcript) = fixture();
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.append_entry(
                TranscriptEntry {
                    id: "tool".into(),
                    content: TranscriptContent::Activity(ActivityContent::Tool {
                        name: "read".into(),
                        argument: "file.rs".into(),
                        arguments: "{\"path\":\"file.rs\"}".into(),
                        result: "output line\n".repeat(300),
                        running: false,
                        failed: false,
                    }),
                },
                cx,
            )
            .unwrap();
        })
    });
    window.draw();
    let key = window.read(|_, cx| transcript.read(cx).rows.last().unwrap().key);
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            let View::Activity(activity) = &this.cache[&key].view else {
                panic!()
            };
            activity.update(cx, |activity, cx| activity.toggle(cx));
        })
    });
    for _ in 0..3 {
        window.draw();
    }
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.list.scroll_by(px(-60.));
            cx.notify();
        })
    });
    window.draw();
    let inner = window.update(|_, window, _| {
        window
            .find(format!("transcript-activity-{key}-details"))
            .bounds()
            .center()
    });
    let scroll = window.read(|_, cx| {
        transcript
            .read(cx)
            .rows
            .last()
            .unwrap()
            .details_scroll
            .clone()
    });
    let before = window.read(|_, cx| {
        let p = transcript.read(cx).list.logical_scroll_top();
        (p.item_ix, p.offset_in_item)
    });
    let start = scroll.offset();
    window.simulate_scroll(inner, point(px(0.), px(-90.)));
    window.draw();
    assert!(scroll.offset().y < start.y);
    assert_eq!(scroll.offset().y, start.y - px(90.));
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            let owner = cx.entity().downgrade();
            for ix in 0..this.rows.len() - 1 {
                this.mount(ix, owner.clone(), cx);
            }
            assert!(!this.cache.contains_key(&key));
            let mut content = this.rows.last().unwrap().entry.content.clone();
            let TranscriptContent::Activity(ActivityContent::Tool { result, .. }) = &mut content
            else {
                panic!()
            };
            result.push_str("more output");
            this.update_content("tool", content, cx);
        })
    });
    window.draw();
    assert_eq!(scroll.offset().y, start.y - px(90.));
    window.read(|_, cx| {
        let this = transcript.read(cx);
        let View::Activity(activity) = &this.cache[&key].view else {
            panic!()
        };
        assert!(activity.read(cx).expanded());
        assert_eq!(activity.read(cx).details_scroll().offset(), scroll.offset());
        assert!(this.cache.len() <= CACHE_LIMIT);
        let position = this.list.logical_scroll_top();
        assert_eq!((position.item_ix, position.offset_in_item), before);
    });
    for delta in [-100_000., -100., 100_000., 100.] {
        window.simulate_scroll(inner, point(px(0.), px(delta)));
        window.draw();
        let after = window.read(|_, cx| {
            let p = transcript.read(cx).list.logical_scroll_top();
            (p.item_ix, p.offset_in_item)
        });
        assert_eq!(after, before, "nested wheel {delta} moved transcript");
    }
    assert_eq!(scroll.offset().y, px(0.));
    let viewport = window.read(|_, cx| transcript.read(cx).list.viewport_bounds());
    window.simulate_scroll(
        point(viewport.left() + px(5.), viewport.center().y),
        point(px(0.), px(100.)),
    );
    window.draw();
    let after = window.read(|_, cx| {
        let p = transcript.read(cx).list.logical_scroll_top();
        (p.item_ix, p.offset_in_item)
    });
    assert_ne!(after, before, "wheel outside details must reach transcript");
}

#[test]
fn working_tail_is_virtual_and_reset_retires_it_without_touching_selection() {
    let (_, mut window, transcript) = fixture();
    let keys = window.read(|_, cx| {
        transcript
            .read(cx)
            .rows
            .iter()
            .map(|row| row.key)
            .collect::<Vec<_>>()
    });
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.set_working_indicator(Some("Working"), cx);
            assert_eq!(this.len(), 1000);
            assert_eq!(this.list.item_count(), 1001);
            assert_eq!(this.copy_selection(range(keys[999], 0, keys[999], 1)), "A");
        })
    });
    window.draw();
    assert!(
        window.update(
            |_, window, _| window.find("tau-working-indicator").bounds().size.height > px(0.)
        )
    );
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.set_working_indicator(Some("Stopping"), cx);
            this.append_entry(entry("new", "new tail"), cx).unwrap();
            assert_eq!(this.list.item_count(), 1002);
            this.set_entries(vec![entry("other", "reconciled")], cx)
                .unwrap();
            assert_eq!(this.list.item_count(), 2);
            this.set_entries(vec![], cx).unwrap();
            assert_eq!(this.list.item_count(), 1);
        })
    });
    window.draw();
    assert!(
        window.update(
            |_, window, _| window.find("tau-working-indicator").bounds().size.height > px(0.)
        )
    );
    window.update(|_, _, cx| {
        transcript.update(cx, |this, cx| {
            this.reset(cx);
            assert_eq!(this.working_indicator(), None);
            assert_eq!(this.list.item_count(), 0);
            assert!(this.cache.len() <= CACHE_LIMIT);
        })
    });
}
