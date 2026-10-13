//! Parent registers this inside transcript.rs; the test exercises its real 128-view eviction.
use super::*;
use gpui_kit::{MouseButton, TestApp, base::Root, point, size, test::TestWindowExt};

#[test]
fn production_transcript_copy_keeps_exact_selected_raw_bytes_after_stream_update_and_cache_eviction()
 {
    for dark in [false, true] {
        let original = "  first  λ\r\nlast  line\n\n";
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let transcript = app.new_entity(|cx| {
            let mut transcript = Transcript::new("exact-transcript", dark, cx);
            let mut entries: Vec<_> = (0..200)
                .map(|i| TranscriptEntry {
                    id: format!("earlier-{i}"),
                    content: TranscriptContent::Assistant("Earlier prose.".into()),
                })
                .collect();
            entries.push(TranscriptEntry {
                id: "tool".into(),
                content: TranscriptContent::Activity(ActivityContent::Tool {
                    name: "bash".into(),
                    argument: "run".into(),
                    arguments: "".into(),
                    result: original.into(),
                    running: true,
                    failed: false,
                }),
            });
            transcript.set_entries(entries, cx).unwrap();
            transcript
        });
        let mut window = app
            .open_window(|window, cx| Root::new(transcript.clone(), window, cx).flex().flex_col());
        window.simulate_resize(size(px(1000.), px(700.)));
        window.update(|_, window, cx| {
            window.activate_window();
            window.bounds_changed(cx);
        });
        window.draw();
        let key = window.read(|_, cx| transcript.read(cx).rows.last().unwrap().key);
        window.update(|_, _, cx| {
            transcript.update(cx, |transcript, cx| {
                let View::Activity(activity) = &transcript.cache[&key].view else {
                    panic!()
                };
                activity.update(cx, |row, cx| row.toggle(cx));
            })
        });
        for _ in 0..3 {
            window.draw();
        }
        let bounds = window.update(|_, window, _| window.find("Result-body").bounds());
        let start = point(bounds.left() + px(0.5), bounds.top() + px(3.));
        let end = point(bounds.right() - px(1.), bounds.bottom() - px(1.));
        window.simulate_mouse_down(start, MouseButton::Left);
        window.simulate_event(gpui_kit::MouseMoveEvent {
            position: end,
            pressed_button: Some(MouseButton::Left),
            modifiers: Default::default(),
        });
        window.simulate_mouse_up(end, MouseButton::Left);
        window.simulate_keystroke("cmd-c");
        window.draw();
        assert_eq!(
            window.read(|_, cx| cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap()),
            original
        );
        let source = window.read(|_, cx| {
            let View::Activity(activity) = &transcript.read(cx).cache[&key].view else {
                panic!()
            };
            activity.read(cx).source_state()
        });
        window.update(|_, _, cx| {
            transcript.update(cx, |transcript, cx| {
                let owner = cx.entity().downgrade();
                for ix in 0..transcript.rows.len() - 1 {
                    transcript.mount(ix, owner.clone(), cx);
                }
                assert!(!transcript.cache.contains_key(&key));
                let mut content = transcript.rows.last().unwrap().entry.content.clone();
                let TranscriptContent::Activity(ActivityContent::Tool { result, .. }) =
                    &mut content
                else {
                    panic!()
                };
                result.push_str("stream suffix\n");
                assert!(transcript.update_content("tool", content, cx));
            })
        });
        window.draw();
        window.read(|_, cx| {
            let View::Activity(activity) = &transcript.read(cx).cache[&key].view else {
                panic!()
            };
            assert_eq!(
                activity.read(cx).source_state(),
                source,
                "selection must belong to the source row, not the evicted ActivityRow"
            );
        });
        window.simulate_keystroke("cmd-c");
        window.draw();
        assert_eq!(
            window.read(|_, cx| cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap()),
            original
        );
        window.update(|_, w, cx| w.blur(cx));
        app.run_until_parked();
    }
}
