use super::*;
use std::time::Duration;

fn tick(app: &mut TestApp, window: &mut TestAppWindow<Root>) {
    for _ in 0..8 {
        app.advance_clock(Duration::from_millis(20));
        app.run_until_parked();
        window.draw();
    }
}

#[test]
fn held_left_horizontal_thinking_selection_keeps_anchor_geometry_and_reader_offsets() {
    for dark in [false, true] {
        for overflowing in [false, true] {
            let text = if overflowing {
                "Horizontal reasoning stays on this line.\n\n".repeat(200)
            } else {
                "Horizontal reasoning stays on this line.".into()
            };
            let (app, mut window, host, row) = fixture(ActivityContent::Thinking { text }, dark);
            let source = window.read(|_, cx| row.read(cx).source_state());
            let inner = window.read(|_, cx| source.read(cx).prose_scroll[0].clone());
            let outer = window.read(|_, cx| host.read(cx).outer.clone());
            if overflowing {
                inner.set_offset(point(px(0.), px(-100.)));
                window.update(|_, _, cx| host.update(cx, |_, cx| cx.notify()));
                window.draw();
            }
            let bounds = window.update(|_, w, _| w.find("exact-prose-0").bounds());
            let prose = window.read(|_, cx| source.read(cx).prose_reader(0).0);
            // A paragraph gap maps both X positions to one endpoint. Require
            // advancing offsets on a mounted run, away from the tall reader's edges.
            let inset = if overflowing { px(32.) } else { px(1.) };
            let (start, end) = window.read(|_, cx| {
                (0..f32::from(bounds.size.height) as usize)
                    .map(|dy| bounds.top() + px(dy as f32 + 0.5))
                    .filter(|y| *y >= bounds.top() + inset && *y < bounds.bottom() - inset)
                    .find_map(|y| {
                        let start = point(bounds.left() + px(2.), y);
                        let end = point(bounds.left() + px(180.), y);
                        let text = prose.read(cx);
                        let a = text.rendered_offset_at(start)?;
                        let b = text.rendered_offset_at(end)?;
                        (a < b).then_some((start, end))
                    })
                    .expect("reader must have a mounted horizontal glyph run")
            });
            let y = start.y;
            let before = (inner.offset(), outer.offset(), bounds);
            window.simulate_mouse_down(start, MouseButton::Left);
            for x in [60., 120., 180.] {
                window.simulate_event(gpui_kit::MouseMoveEvent {
                    position: point(bounds.left() + px(x), y),
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Default::default(),
                });
                for _ in 0..8 {
                    app.advance_clock(Duration::from_millis(20));
                    app.run_until_parked();
                    window.draw();
                    window.read(|_, cx| {
                        assert!(!prose.read(cx).selected_text_with(cx).is_empty(),
                            "held drag must retain real selection on every frame: dark={dark}, overflowing={overflowing}");
                    });
                    assert_eq!(inner.offset(), before.0);
                    assert_eq!(outer.offset(), before.1);
                    assert_eq!(
                        window.update(|_, w, _| w.find("exact-prose-0").bounds()),
                        before.2
                    );
                }
            }
            window.simulate_mouse_up(end, MouseButton::Left);
            assert!(
                !copy(&mut window).is_empty(),
                "held-left drag must really select text"
            );
        }
    }
}

#[test]
fn real_thinking_viewport_edge_selection_still_autoscrolls() {
    let (mut app, mut window, _, row) = fixture(
        ActivityContent::Thinking {
            text: "Long reasoning line.\n\n".repeat(240),
        },
        false,
    );
    let source = window.read(|_, cx| row.read(cx).source_state());
    let inner = window.read(|_, cx| source.read(cx).prose_scroll[0].clone());
    let bounds = window.update(|_, w, _| w.find("exact-prose-0").bounds());
    let start = point(bounds.left() + px(3.), bounds.top() + px(60.));
    let edge = point(start.x + px(100.), bounds.bottom() - px(2.));
    window.simulate_mouse_down(start, MouseButton::Left);
    window.simulate_event(gpui_kit::MouseMoveEvent {
        position: edge,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    tick(&mut app, &mut window);
    assert!(
        inner.offset().y < px(0.),
        "real edge selection must scroll the reader"
    );
    window.simulate_mouse_up(edge, MouseButton::Left);
    let stopped = inner.offset();
    tick(&mut app, &mut window);
    assert_eq!(inner.offset(), stopped);
}

#[test]
fn fitting_tool_details_do_not_paint_a_fake_scrollbar_thumb() {
    for dark in [false, true] {
        let (_app, mut window, _, row) = fixture(tool("", "Short output", false), dark);
        assert_eq!(
            window.read(|_, cx| row.read(cx).details_scroll().max_offset().y),
            px(0.)
        );
        let details = window.update(|_, w, _| w.find("exact-details").bounds());
        window.update(|_, w, _| {
            let ink = gpui_kit::rgba((support::tokens(dark).text << 8) | 0x59);
            let clip = details.scale(w.scale_factor());
            assert!(
                !w.painted_quads()
                    .into_iter()
                    .any(|q| { q.background == ink.into() && clip.contains(&q.bounds.center()) })
            );
        });
    }
}

#[test]
fn overflowing_tool_details_paint_shared_thumb_and_real_thumb_drag_moves_retained_handle() {
    for dark in [false, true] {
        let (_app, mut window, host, row) =
            fixture(tool("", &"Output line\n".repeat(300), false), dark);
        let scroll = window.read(|_, cx| row.read(cx).details_scroll().clone());
        let outer = window.read(|_, cx| host.read(cx).outer.clone());
        let details = window.update(|_, w, _| w.find("exact-details").bounds());
        let thumb = window.update(|_, w, _| {
            let ink = gpui_kit::rgba((support::tokens(dark).text << 8) | 0x59);
            let clip = details.scale(w.scale_factor());
            w.painted_quads()
                .into_iter()
                .find(|q| q.background == ink.into() && clip.contains(&q.bounds.center()))
                .expect("overflow reader must paint the shared scrollbar thumb")
                .bounds
        });
        let scale = window.update(|_, w, _| w.scale_factor());
        let start = point(
            px(thumb.center().x.0 / scale),
            px(thumb.center().y.0 / scale),
        );
        let end = point(start.x, start.y + px(100.));
        window.simulate_mouse_down(start, MouseButton::Left);
        window.simulate_event(gpui_kit::MouseMoveEvent {
            position: end,
            pressed_button: Some(MouseButton::Left),
            modifiers: Default::default(),
        });
        window.simulate_mouse_up(end, MouseButton::Left);
        window.draw();
        assert!(
            scroll.offset().y < px(-100.),
            "real thumb drag must drive the retained handle"
        );
        assert_eq!(outer.offset().y, px(0.));
    }
}
