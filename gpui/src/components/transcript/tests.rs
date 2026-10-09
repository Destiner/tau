use super::*;
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
