//! Real asynchronous preparation and layout; no seeded diagram/image or accessibility queries.
use super::super::{Camera, DiagramBlock, DiagramError, Spec, ViewBox, tokens, visible_spec};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, Render, TestApp, TestAppWindow, Window, base::Root,
    div, prelude::*, px, size,
};
use std::time::Duration;
use std::time::Instant;

struct Host {
    block: Entity<DiagramBlock>,
    width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(self.width)).child(self.block.clone())
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    host: Entity<Host>,
    block: Entity<DiagramBlock>,
    app: TestApp,
}
impl Fixture {
    fn new() -> Self {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let mut entities = None;
        let mut window = app.open_window(|w, cx| {
            let block = cx.new(|cx| {
                DiagramBlock::new(
                    "flowchart LR\nA[First lengthy label] --> B[Second lengthy label]".into(),
                    false,
                    cx,
                )
            });
            let host = cx.new(|_| Host {
                block: block.clone(),
                width: 240.,
            });
            entities = Some((host.clone(), block));
            Root::new(host, w, cx)
        });
        window.simulate_resize(size(px(900.), px(700.)));
        let (host, block) = entities.unwrap();
        Self {
            app,
            window,
            host,
            block,
        }
    }
    fn pump(&mut self) {
        self.app.advance_clock(Duration::from_millis(8));
        self.app.run_until_parked();
        self.window.draw();
    }
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            self.pump();
            let settled = self.window.update(|_, _, cx| {
                let block = self.block.read(cx);
                assert!(!block.failed(), "unexpected {:?}", block.error_kind());
                block.ready() && !block.busy() && block.painted.is_some()
            });
            if settled {
                break;
            }
            assert!(Instant::now() < deadline, "live view did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn assert_inline_pixels(&mut self) -> Spec {
        self.window.update(|_, w, cx| {
            let block = self.block.read(cx);
            let spec = block.painted.unwrap();
            assert!(!spec.fullscreen);
            assert!(spec.rect[2] > 20.);
            assert_eq!(spec.backing, f64::from(w.scale_factor()));
            let pixels = block.inline_image.as_ref().unwrap().size(0);
            assert_eq!(pixels.width.0, (spec.rect[2] * spec.backing).ceil() as i32);
            assert_eq!(pixels.height.0, (spec.rect[3] * spec.backing).ceil() as i32);
            spec
        })
    }
}
#[test]
fn first_inline_layout_and_width_changes_reraster_at_window_backing_scale() {
    let mut f = Fixture::new();
    f.settle();
    let first = f.assert_inline_pixels();
    f.window.update(|_, _, cx| {
        f.host.update(cx, |s, cx| {
            s.width = 120.;
            cx.notify();
        })
    });
    // A first frame discovers the changed canvas size, then its deferred sync schedules work.
    f.pump();
    f.settle();
    let narrower = f.assert_inline_pixels();
    assert!(narrower.zoom < first.zoom);
    assert!(narrower.rect[2] < first.rect[2]);
}
#[test]
fn live_source_theme_supersession_and_rejection_do_not_accept_stale_results() {
    let mut f = Fixture::new();
    f.app.run_until_parked();
    f.window.update(|_, _, cx| {
        f.block.update(cx, |s, cx| {
            let old = s.operation.clone().unwrap();
            s.set_source("flowchart TD\nX[Replacement marker] --> Y".into(), cx);
            assert_eq!(old.checkpoint(), Err(DiagramError::Cancelled));
            let source_op = s.operation.clone().unwrap();
            s.set_theme(true, cx);
            assert_eq!(source_op.checkpoint(), Err(DiagramError::Cancelled));
        })
    });
    f.settle();
    f.window.update(|_, _, cx| {
        let block = f.block.read(cx);
        let svg = block.rendered_svg().unwrap();
        let model = block.semantic_model().unwrap().to_string();
        assert!(model.contains("Replacement marker"));
        assert!(!model.contains("First lengthy label"));
        assert!(svg.contains(&format!("#{:06x}", tokens(true).text)));
    });
    f.window.update(|_, _, cx| {
        f.block.update(cx, |s, cx| {
            s.set_theme(false, cx);
            s.set_source("x".repeat(24001), cx);
        })
    });
    for _ in 0..32 {
        f.pump();
        std::thread::sleep(Duration::from_millis(1));
    }
    f.window.update(|_, _, cx| {
        let block = f.block.read(cx);
        assert_eq!(block.error_kind(), Some(DiagramError::SourceLimit));
        assert!(!block.busy());
        assert!(!block.ready());
        assert!(block.rendered_svg().is_none());
    });
}
#[test]
fn live_zoom_rasters_survive_atlas_eviction_and_keep_camera_crop() {
    use gpui_kit::{
        Modifiers, MouseButton, PinchEvent, ScrollDelta, ScrollWheelEvent, TouchPhase, point,
        test::TestWindowExt,
    };
    let mut f = Fixture::new();
    f.settle();
    let expand = f
        .window
        .update(|_, w, _| w.find("diagram-expand").bounds().center());
    f.window.simulate_mouse_move(expand);
    f.window.simulate_click(expand, MouseButton::Left);
    f.settle();
    for step in 0..12 {
        let position = f.window.update(|_, w, _| {
            let bounds = w.find("diagram-fullscreen").bounds();
            bounds.origin + point(bounds.size.width * 0.47, bounds.size.height * 0.46)
        });
        f.window.simulate_mouse_move(position);
        let before = f
            .window
            .update(|_, _, cx| f.block.read(cx).viewer.as_ref().unwrap().camera);
        if step % 2 == 0 {
            f.window.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(0.), px(if step < 6 { 8. } else { -8. }))),
                modifiers: Modifiers {
                    control: true,
                    ..Default::default()
                },
                touch_phase: TouchPhase::Moved,
            });
        } else {
            f.window.simulate_event(PinchEvent {
                position,
                delta: if step < 6 { 0.08 } else { -0.08 },
                phase: TouchPhase::Moved,
                modifiers: Default::default(),
            });
        }
        f.window.draw();
        f.window.update(|_, _, cx| {
            let after = f.block.read(cx).viewer.as_ref().unwrap().camera;
            assert_eq!(after.scale > before.scale, step < 6);
            for (anchor, a, b) in [
                (f64::from(position.x), before.tx, after.tx),
                (f64::from(position.y), before.ty, after.ty),
            ] {
                assert!(((anchor - a) / before.scale - (anchor - b) / after.scale).abs() < 1e-8);
            }
        });
        f.settle();
        f.window.update(|_, w, cx| {
            let b = f.block.read(cx);
            let spec = b.painted.unwrap();
            assert!(spec.fullscreen);
            assert_eq!(spec.zoom, b.viewer.as_ref().unwrap().camera.scale);
            let image = b.image.as_ref().unwrap().clone();
            assert!(w.has_image_atlas_entry(&image));
            w.drop_image(image.clone()).unwrap();
            assert!(!w.has_image_atlas_entry(&image));
        });
        f.window.draw();
        f.window.update(|_, w, cx| {
            assert!(w.has_image_atlas_entry(f.block.read(cx).image.as_ref().unwrap()));
        });
    }
}

#[test]
fn backing_scale_changes_are_part_of_inline_raster_identity() {
    let b = ViewBox {
        x: -20.,
        y: 30.,
        width: 300.,
        height: 100.,
    };
    let c = Camera {
        scale: 0.5,
        tx: 10.,
        ty: -15.,
    };
    let one = visible_spec(c, [150., 50.], b, 1., false).unwrap();
    let two = visible_spec(c, [150., 50.], b, 2., false).unwrap();
    assert_ne!(one, two);
    assert_eq!(one.viewport, two.viewport);
    assert_eq!(one.rect, two.rect);
    assert_eq!(two.backing, 2.);
}
