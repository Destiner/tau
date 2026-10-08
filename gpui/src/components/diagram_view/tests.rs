use super::{Camera, DiagramBlock, DiagramError, ViewBox, visible_spec};
use gpui_kit::{AppContext, TestAppContext};
#[path = "input_tests.rs"]
mod input;
#[path = "lifecycle_tests.rs"]
mod lifecycle;
#[test]
fn camera_crop_is_bounded_at_eight_times_zoom() {
    let b = ViewBox {
        x: -20.,
        y: 30.,
        width: 16000.,
        height: 16000.,
    };
    let c = Camera {
        scale: 8.,
        tx: 200.,
        ty: -400.,
    };
    let s = visible_spec(c, [900., 700.], b, 2., true).unwrap();
    assert!(s.viewport.width <= 900. / 8.);
    assert!(s.viewport.height <= 700. / 8.);
    assert!(
        s.viewport.width * s.viewport.height * s.zoom * s.zoom * s.backing * s.backing
            <= 16_777_216.
    );
}
#[test]
fn empty_camera_intersection_does_not_allocate() {
    let b = ViewBox {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 100.,
    };
    assert!(
        visible_spec(
            Camera {
                scale: 1.,
                tx: 10000.,
                ty: 10000.
            },
            [900., 700.],
            b,
            2.,
            true
        )
        .is_none()
    );
}
#[gpui_kit::test]
fn source_theme_and_drop_cancel_revisions(cx: &mut TestAppContext) {
    let entity = cx.new(|cx| DiagramBlock::new("x".repeat(24001), false, cx));
    entity.update(cx, |s, cx| {
        assert!(s.failed());
        assert!(!s.ready());
        assert!(!s.busy());
        let rev = s.revision;
        s.set_theme(true, cx);
        assert!(s.revision > rev);
        assert!(s.failed());
        s.set_source("flowchart TD\nA --> B".into(), cx);
        assert!(!s.failed());
        assert!(s.busy());
        let op = s.operation.clone().unwrap();
        s.set_source("x".repeat(24001), cx);
        assert_eq!(op.checkpoint(), Err(DiagramError::Cancelled));
    });
    let op = entity.update(cx, |s, cx| {
        s.set_source("flowchart TD\nA --> B".into(), cx);
        s.operation.clone().unwrap()
    });
    let weak = entity.downgrade();
    drop(entity);
    cx.update(|_| {});
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
    assert_eq!(op.checkpoint(), Err(DiagramError::Cancelled));
}
