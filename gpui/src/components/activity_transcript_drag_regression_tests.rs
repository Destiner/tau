use super::*;
use gpui_kit::{MouseButton, MouseMoveEvent};
use std::time::Duration;

#[test]
fn held_left_single_line_thinking_drag_keeps_real_transcript_reader_and_anchor_stable() {
    for dark in [false, true] {
        let (app, mut window, transcript, key) = fixture_content(
            dark,
            ActivityContent::Thinking {
                text: "Horizontal reasoning selection must not move the transcript.".into(),
            },
        );
        let id = format!("wheel-transcript-activity-{key}-prose-0");
        let bounds = window.update(|_, w, _| w.find(id.clone()).bounds());
        let viewport = window.read(|_, cx| transcript.read(cx).list.viewport_bounds());
        let (prose, inner) = window.read(|_, cx| {
            let View::Activity(activity) = &transcript.read(cx).cache[&key].view else {
                panic!("fixture must mount an activity row")
            };
            activity.read(cx).source_state().read(cx).prose_reader(0)
        });
        let start = window.read(|_, cx| {
            (0..f32::from(bounds.size.height) as usize)
                .map(|dy| point(bounds.left() + px(2.), bounds.top() + px(dy as f32 + 0.5)))
                .filter(|p| viewport.contains(p))
                .find(|p| {
                    let text = prose.read(cx);
                    matches!((text.rendered_offset_at(*p), text.rendered_offset_at(point(p.x + px(238.), p.y))),
                        (Some(a), Some(b)) if a < b)
                })
                .expect("visible mounted glyphs must map advancing horizontal offsets")
        });
        let y = start.y;
        let before = position(&window, &transcript);
        let inner_before = inner.offset();
        window.simulate_mouse_down(start, MouseButton::Left);
        for x in [80., 160., 240.] {
            window.simulate_event(MouseMoveEvent {
                position: point(bounds.left() + px(x), y),
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            });
            for _ in 0..8 {
                app.advance_clock(Duration::from_millis(20));
                app.run_until_parked();
                window.draw();
                window.read(|_, cx| {
                    assert!(
                        !prose.read(cx).selected_text_with(cx).is_empty(),
                        "held drag must retain actual thinking selection on every frame"
                    );
                });
                assert_eq!(inner.offset(), inner_before);
                assert_eq!(position(&window, &transcript), before);
                assert_eq!(window.update(|_, w, _| w.find(id.clone()).bounds()), bounds);
            }
            assert_eq!(position(&window, &transcript), before);
            assert_eq!(window.update(|_, w, _| w.find(id.clone()).bounds()), bounds);
        }
        window.simulate_mouse_up(point(bounds.left() + px(240.), y), MouseButton::Left);
        window.simulate_keystroke("cmd-c");
        window.draw();
        window.read(|_, cx| {
            let copied = cx
                .read_from_clipboard()
                .and_then(|c| c.text())
                .unwrap_or_default();
            assert!(
                copied.starts_with("Horizontal"),
                "drag must select real thinking glyphs: {copied:?}"
            );
        });
    }
}
