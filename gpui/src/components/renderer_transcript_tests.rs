use super::*;
use crate::components::renderer_interaction_tests::{held_move, shaped_rows};
use gpui_kit::{MouseButton, TestApp, base::Root, point, size};

pub(crate) fn cached_prose(t: &Transcript) -> Vec<(String, Entity<TextViewState>)> {
    t.rows
        .iter()
        .filter_map(|row| {
            let View::Prose(text, _) = &t.cache.get(&row.key)?.view else {
                return None;
            };
            Some((row.entry.id.clone(), text.clone()))
        })
        .collect()
}

#[test]
fn production_transcript_unicode_copy_after_128_view_eviction_does_not_mount_cold_sources() {
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let transcript = app.new_entity(|cx| {
            let mut t = Transcript::new("renderer-cold-copy", dark, cx);
            t.set_entries(
                (0..260)
                    .map(|i| TranscriptEntry {
                        id: format!("source-{i}"),
                        content: TranscriptContent::Assistant(
                            "甲乙 **café** 👩‍💻\n\n```txt\n  λ\r\nlast  line\r\n```".into(),
                        ),
                    })
                    .collect(),
                cx,
            )
            .unwrap();
            t
        });
        let mut root =
            app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
        root.simulate_resize(size(px(600.), px(600.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        root.draw();
        let (key, text) = root.read(|_, cx| {
            let t = transcript.read(cx);
            let key = t.rows.last().unwrap().key;
            let View::Prose(text, _) = &t.cache[&key].view else {
                panic!()
            };
            (key, text.clone())
        });
        let len = root.read(|_, cx| text.read(cx).rendered_text().len());
        let rows = shaped_rows(&mut root, &text, 0..len);
        let first = rows[0];
        let last = *rows.last().unwrap();
        root.simulate_mouse_down(
            point(first.left() + px(0.1), first.center().y),
            MouseButton::Left,
        );
        let end = point(last.right() + px(2.), last.center().y);
        held_move(&mut root, end);
        root.simulate_mouse_up(end, MouseButton::Left);
        let selected = root.read(|_, cx| transcript.read(cx).selection.range(cx).unwrap());
        let canonical = "甲乙 café 👩‍💻\n  λ\nlast  line";
        root.read(|_, cx| {
            assert_eq!(
                text.read(cx)
                    .rendered_text()
                    .as_str()
                    .trim_end_matches('\n'),
                canonical
            );
            assert_eq!(selected.anchor.offset, 0);
            assert_eq!(selected.cursor.offset, canonical.len());
        });
        super::super::renderer_interaction_tests::assert_crlf_code_geometry(&mut root, &text);
        let expected = canonical;
        root.read(|_, cx| {
            let rendered = text.read(cx).rendered_text();
            let at = rendered.as_str().find("λ\nlast").unwrap();
            let mapped = text
                .read(cx)
                .source_range_for_rendered_range(at..at + "λ\nlast".len())
                .unwrap();
            let original = "甲乙 **café** 👩‍💻\n\n```txt\n  λ\r\nlast  line\r\n```";
            assert_eq!(&original[mapped], "λ\r\nlast");
            assert_eq!(
                TextViewState::copy_markdown_slice(
                    original,
                    None,
                    None,
                    gpui_kit::base::SelectionFormat::Source
                )
                .unwrap(),
                original
            );
        });
        root.simulate_keystroke("cmd-c");
        root.read(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|v| v.text()),
                Some(expected.into())
            )
        });
        root.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                let owner = cx.entity().downgrade();
                for i in 0..t.rows.len() - 1 {
                    t.mount(i, owner.clone(), cx);
                }
                assert_eq!(t.cache.len(), 128);
                assert!(!t.cache.contains_key(&key));
                let before = t.cache.keys().copied().collect::<Vec<_>>();
                // This is the same source provider Root's Copy action uses, with no view present.
                assert_eq!(t.copy_selection(selected), expected);
                assert_eq!(
                    t.cache.keys().copied().collect::<Vec<_>>(),
                    before,
                    "copy must neither mount nor evict a cold view"
                );
            })
        });
        // The domain provider reads Transcript, so observe it outside its update.
        root.read(|_, cx| assert_eq!(transcript.read(cx).selection.range(cx), Some(selected)));
        let remounted = root.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                let owner = cx.entity().downgrade();
                let View::Prose(remounted, _) = t.mount(t.rows.len() - 1, owner, cx).unwrap()
                else {
                    panic!()
                };
                assert_ne!(remounted, text);
                remounted
            })
        });
        root.read(|_, cx| {
            assert_eq!(transcript.read(cx).selection.range(cx), Some(selected));
            assert_eq!(remounted.read(cx).selected_text_with(cx), expected);
        });
        for _ in 0..3 {
            root.draw();
        }
        root.simulate_keystroke("cmd-c");
        root.read(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|v| v.text()),
                Some(expected.into())
            )
        });
    }
}

#[test]
fn production_transcript_raw_crlf_copy_survives_source_owned_eviction_and_three_frames() {
    // This exercises the real ActivityRow native readonly editor. CRLF and
    // trailing blank lines are significant.
    use gpui_kit::test::TestWindowExt;
    let original = "  first  λ\r\n👩‍💻 last  line\r\n\r\n";
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let transcript = app.new_entity(|cx| {
            let mut t = Transcript::new("renderer-raw-copy", dark, cx);
            let mut entries = (0..260)
                .map(|i| TranscriptEntry {
                    id: format!("earlier-{i}"),
                    content: TranscriptContent::Assistant("Earlier café.".into()),
                })
                .collect::<Vec<_>>();
            entries.push(TranscriptEntry {
                id: "raw".into(),
                content: TranscriptContent::Activity(ActivityContent::Tool {
                    name: "bash".into(),
                    argument: "run".into(),
                    arguments: String::new(),
                    result: original.into(),
                    running: false,
                    failed: false,
                }),
            });
            t.set_entries(entries, cx).unwrap();
            t
        });
        let mut root =
            app.open_window(|w, cx| Root::new(transcript.clone(), w, cx).flex().flex_col());
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        root.draw();
        let key = root.read(|_, cx| transcript.read(cx).rows.last().unwrap().key);
        root.update(|_, _, cx| {
            let View::Activity(row) = &transcript.read(cx).cache[&key].view else {
                panic!()
            };
            row.clone().update(cx, |row, cx| row.toggle(cx));
        });
        for _ in 0..3 {
            root.draw();
        }
        let body = root.update(|_, w, _| w.find("Result-body").bounds());
        let end = point(body.right() - px(1.), body.bottom() - px(1.));
        root.simulate_mouse_down(
            point(body.left() + px(0.5), body.top() + px(3.)),
            MouseButton::Left,
        );
        held_move(&mut root, end);
        for _ in 0..3 {
            root.draw();
            root.simulate_keystroke("cmd-c");
            root.read(|_, cx| {
                assert_eq!(
                    cx.read_from_clipboard().and_then(|v| v.text()),
                    Some(original.into())
                )
            });
            root.update(|_, w, _| {
                let blue = support::input_style(dark).selection;
                assert!(w.painted_paths().iter().any(|p| p.color == blue.into()
                    && body.scale(w.scale_factor()).contains(&p.bounds.center())));
            });
        }
        root.simulate_mouse_up(end, MouseButton::Left);
        let source = root.read(|_, cx| {
            let View::Activity(row) = &transcript.read(cx).cache[&key].view else {
                panic!()
            };
            row.read(cx).source_state()
        });
        root.update(|_, _, cx| {
            transcript.update(cx, |t, cx| {
                let owner = cx.entity().downgrade();
                for i in 0..t.rows.len() - 1 {
                    t.mount(i, owner.clone(), cx);
                }
                assert_eq!(t.cache.len(), 128);
                assert!(!t.cache.contains_key(&key));
                let View::Activity(row) = t.mount(t.rows.len() - 1, owner, cx).unwrap() else {
                    panic!()
                };
                assert_eq!(
                    row.read(cx).source_state(),
                    source,
                    "raw selection is source-owned, not view-owned"
                );
                let ActivityContent::Tool { result, .. } = row.read(cx).content() else {
                    panic!()
                };
                assert_eq!(
                    result, original,
                    "view remount must preserve original CRLF bytes"
                );
            })
        });
        for _ in 0..3 {
            root.draw();
        }
        root.simulate_keystroke("cmd-c");
        root.read(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|v| v.text()),
                Some(original.into())
            )
        });
        // TestPlatform retains the focused native input handler until a blur frame.
        // Release it only after all interaction, eviction and Copy assertions.
        root.update(|_, w, cx| w.blur(cx));
        app.run_until_parked();
        root.draw();
    }
}
