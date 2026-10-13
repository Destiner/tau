//! Register as a child of `transcript` so the oracle reads the real virtual-list position.
use super::*;
use gpui_kit::{Pixels, TestApp, TestAppWindow, base::Root, point, size, test::TestWindowExt};

fn fixture(dark: bool, result: &str) -> (TestApp, TestAppWindow<Root>, Entity<Transcript>, u64) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let mut transcript = None;
    let mut window = app.open_window(|window, cx| {
        let entity = cx.new(|cx| {
            let mut this = Transcript::new("wheel-transcript", dark, cx);
            let mut entries: Vec<_> = (0..200)
                .map(|i| TranscriptEntry {
                    id: format!("paragraph-{i}"),
                    content: TranscriptContent::Assistant("Earlier transcript paragraph.".into()),
                })
                .collect();
            entries.push(TranscriptEntry {
                id: "tool".into(),
                content: TranscriptContent::Activity(ActivityContent::Tool {
                    name: "read".into(),
                    argument: "fixture.rs".into(),
                    arguments: "{\"path\":\"fixture.rs\"}".into(),
                    result: result.into(),
                    running: false,
                    failed: false,
                }),
            });
            this.set_entries(entries, cx).unwrap();
            this
        });
        transcript = Some(entity.clone());
        Root::new(entity, window, cx).flex().flex_col()
    });
    window.simulate_resize(size(px(1000.), px(700.)));
    window.update(|_, w, cx| w.bounds_changed(cx));
    window.draw();
    let transcript = transcript.unwrap();
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
            this.list.scroll_by(px(-30.));
            cx.notify();
        })
    });
    window.draw();
    (app, window, transcript, key)
}

fn position(window: &TestAppWindow<Root>, transcript: &Entity<Transcript>) -> (usize, Pixels) {
    window.read(|_, cx| {
        let p = transcript.read(cx).list.logical_scroll_top();
        (p.item_ix, p.offset_in_item)
    })
}
fn inner_point(
    window: &mut TestAppWindow<Root>,
    transcript: &Entity<Transcript>,
    key: u64,
) -> gpui_kit::Point<Pixels> {
    window.update(|_, w, cx| {
        let bounds = w
            .find(format!("wheel-transcript-activity-{key}-details"))
            .bounds();
        let viewport = transcript.read(cx).list.viewport_bounds();
        let top = bounds.top().max(viewport.top());
        let bottom = bounds.bottom().min(viewport.bottom());
        assert!(bottom > top + px(2.), "wheel must target painted details");
        point(bounds.left() + px(30.), (top + bottom) / 2.)
    })
}

#[test]
fn fitting_tool_details_pass_genuine_wheel_to_root_transcript_in_both_themes() {
    for dark in [false, true] {
        let (_app, mut window, transcript, key) = fixture(dark, "Short result");
        let scroll = window.read(|_, cx| {
            transcript
                .read(cx)
                .rows
                .last()
                .unwrap()
                .details_scroll
                .clone()
        });
        assert_eq!(scroll.max_offset().y, px(0.));
        for delta in [-20., 20.] {
            let before = position(&window, &transcript);
            let inner = inner_point(&mut window, &transcript, key);
            window.simulate_scroll(inner, point(px(0.), px(delta)));
            window.draw();
            assert_ne!(
                position(&window, &transcript),
                before,
                "fitting details swallowed wheel {delta}"
            );
            assert_eq!(scroll.offset().y, px(0.));
        }
    }
}

#[test]
fn tool_overflow_uses_actual_viewport_cap_and_retains_offset_while_resizing() {
    let (_app, mut window, transcript, key) = fixture(false, &"output line\n".repeat(300));
    let scroll = window.read(|_, cx| {
        transcript
            .read(cx)
            .rows
            .last()
            .unwrap()
            .details_scroll
            .clone()
    });
    let inner = inner_point(&mut window, &transcript, key);
    window.simulate_scroll(inner, point(px(0.), px(-90.)));
    window.draw();
    for (height, cap) in [(1000., 520.), (480., 312.), (700., 455.)] {
        window.simulate_resize(size(px(1000.), px(height)));
        window.update(|_, w, cx| w.bounds_changed(cx));
        for _ in 0..3 {
            window.draw();
        }
        window.update(|_, w, _| {
            let details = w
                .find(format!("wheel-transcript-activity-{key}-details"))
                .bounds();
            assert_eq!(details.size.height, px(cap));
        });
        assert_eq!(scroll.offset().y, px(-90.));
        assert!(scroll.max_offset().y > px(0.));
        let before = position(&window, &transcript);
        let inner = inner_point(&mut window, &transcript, key);
        window.simulate_scroll(inner, point(px(0.), px(-10.)));
        window.draw();
        assert_eq!(position(&window, &transcript), before);
        assert_eq!(scroll.offset().y, px(-100.));
        scroll.set_offset(point(px(0.), px(-90.)));
        window.update(|_, _, cx| transcript.update(cx, |_, cx| cx.notify()));
        window.draw();
    }
}

#[test]
fn overflowing_details_claim_wheel_at_both_edges_and_keep_retained_offset_after_eviction() {
    for dark in [false, true] {
        let (_app, mut window, transcript, key) = fixture(dark, &"output line\n".repeat(300));
        let scroll = window.read(|_, cx| {
            transcript
                .read(cx)
                .rows
                .last()
                .unwrap()
                .details_scroll
                .clone()
        });
        assert!(scroll.max_offset().y > px(0.));
        let before = position(&window, &transcript);
        let inner = inner_point(&mut window, &transcript, key);
        window.simulate_scroll(inner, point(px(0.), px(-90.)));
        window.draw();
        assert_eq!(
            scroll.offset().y,
            px(-90.),
            "inner scroller must handle exactly once"
        );
        assert_eq!(position(&window, &transcript), before);
        window.update(|_, _, cx| {
            transcript.update(cx, |this, cx| {
                let owner = cx.entity().downgrade();
                for ix in 0..this.rows.len() - 1 {
                    this.mount(ix, owner.clone(), cx);
                }
                assert!(!this.cache.contains_key(&key));
                let mut content = this.rows.last().unwrap().entry.content.clone();
                let TranscriptContent::Activity(ActivityContent::Tool { result, .. }) =
                    &mut content
                else {
                    panic!()
                };
                result.push_str("more output");
                this.update_content("tool", content, cx);
            })
        });
        window.draw();
        assert_eq!(scroll.offset().y, px(-90.));
        window.read(|_, cx| {
            let View::Activity(activity) = &transcript.read(cx).cache[&key].view else {
                panic!()
            };
            assert!(activity.read(cx).expanded());
            assert_eq!(activity.read(cx).details_scroll().offset(), scroll.offset());
        });
        for delta in [-100_000., -100., 100_000., 100.] {
            let inner = inner_point(&mut window, &transcript, key);
            window.simulate_scroll(inner, point(px(0.), px(delta)));
            window.draw();
            assert_eq!(
                position(&window, &transcript),
                before,
                "overflow edge leaked wheel {delta}"
            );
        }
        assert_eq!(scroll.offset().y, px(0.));
        let outside = window.read(|_, cx| {
            let viewport = transcript.read(cx).list.viewport_bounds();
            point(viewport.left() + px(5.), viewport.center().y)
        });
        window.simulate_scroll(outside, point(px(0.), px(20.)));
        window.draw();
        assert_ne!(
            position(&window, &transcript),
            before,
            "outer wheel control must move transcript"
        );
    }
}
