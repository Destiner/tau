use super::*;
use gpui_kit::{
    Entity, MouseButton, TestApp, TestAppWindow, WindowBounds, WindowOptions, base::Root, size,
    test::TestWindowExt,
};

struct QueueRootHost {
    queue: Entity<MessageQueue>,
    left: Pixels,
    top: Pixels,
    width: Pixels,
}
impl Render for QueueRootHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .absolute()
                .left(self.left)
                .top(self.top)
                .w(self.width)
                .child(self.queue.clone()),
        )
    }
}

fn root_fixture(width: f32, height: f32) -> (TestApp, TestAppWindow<Root>, Entity<MessageQueue>) {
    root_fixture_at(width, height, height - 70.)
}

fn root_fixture_at(
    width: f32,
    height: f32,
    top: f32,
) -> (TestApp, TestAppWindow<Root>, Entity<MessageQueue>) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut queue = None;
    let window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Default::default(),
                size: size(px(width), px(height)),
            })),
            ..Default::default()
        },
        |window, cx| {
            window.activate_window();
            let host = cx.new(|cx| QueueRootHost {
                queue: cx.new(|cx| {
                    MessageQueue::new(
                        "queue",
                        QueueProps {
                            steering: vec!["  exact\n\n  queue text  ".repeat(150)],
                            follow_up: vec!["follow up".into()],
                            failed: (0..40)
                                .map(|i| format!("  unsent {i}\n  preserve spaces  "))
                                .collect(),
                            ..Default::default()
                        },
                        false,
                        cx,
                    )
                }),
                left: px(width - (width - 16.).min(400.) - 8.),
                top: px(top),
                width: px((width - 16.).min(400.)),
            });
            queue = Some(host.read(cx).queue.clone());
            Root::new(host, window, cx)
        },
    );
    (app, window, queue.unwrap())
}

#[test]
fn root_review_restore_click_disabled_and_escape_return_focus() {
    let (_, mut window, queue) = root_fixture(720., 480.);
    let events = Rc::new(RefCell::new(vec![]));
    let received = events.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&queue, move |_, _, e: &QueueEvent, _| {
            received.borrow_mut().push(e.clone())
        })
    });
    window.draw();
    let recover = window.update(|_, w, _| w.find("recover").bounds().center());
    window.simulate_click(recover, MouseButton::Left);
    window.draw();
    window.update(|_, w, _| {
        let review = w.find("review-panel").bounds();
        assert_eq!(
            review.size.height,
            w.pixel_snap(w.viewport_size().height * 0.36)
        );
        assert!(review.size.height <= px(260.));
    });
    let restore = window.update(|_, w, _| w.find("restore").bounds().center());
    window.simulate_click(restore, MouseButton::Left);
    assert!(matches!(
        events.borrow().last(),
        Some(QueueEvent::Recover(0))
    ));
    window.simulate_keystroke("escape");
    window.draw();
    window.update(|_, w, cx| {
        assert!(!queue.read(cx).details_open());
        assert!(queue.read(cx).recover_focus.is_focused(w));
    });
    window.simulate_click(recover, MouseButton::Left);
    window.draw();
    window.update(|_, _, cx| {
        queue.update(cx, |q, cx| {
            let mut props = q.props().clone();
            props.can_recover = false;
            q.set_props(props, cx);
        })
    });
    window.draw();
    let count = events.borrow().len();
    window.simulate_click(restore, MouseButton::Left);
    assert_eq!(events.borrow().len(), count);
    window.simulate_keystroke("escape");
    window.draw();
    window.update(|_, w, cx| {
        assert!(!queue.read(cx).details_open());
        assert!(queue.read(cx).recover_focus.is_focused(w));
    });
}

#[test]
fn root_long_preview_clamps_to_actual_viewport_and_keyboard_focus_opens_it() {
    for (width, height) in [(480., 480.), (720., 480.), (900., 700.)] {
        let (app, mut window, queue) = root_fixture(width, height);
        window.draw();
        window.update(|_, w, cx| {
            let focus = queue.read(cx).chip_focus[0].clone();
            focus.focus(w, cx);
        });
        window.draw();
        app.run_until_parked();
        window.draw();
        window.update(|_, w, _| {
            let preview = w.find("preview").bounds();
            assert!(preview.left() >= px(8.));
            assert!(preview.right() <= w.viewport_size().width - px(8.));
            assert!(preview.size.height <= w.pixel_snap(w.viewport_size().height * 0.45));
            assert!(preview.size.height <= px(340.));
        });
        window.simulate_keystroke("escape");
        app.run_until_parked();
        window.draw();
        assert!(window.update(|_, _, cx| queue.read(cx).preview().is_none()));
    }
}

#[test]
fn root_preview_opens_below_when_there_is_more_room_and_keeps_the_five_pixel_bridge() {
    let (app, mut window, _) = root_fixture_at(720., 480., 40.);
    window.draw();
    let chip = window.update(|_, w, _| {
        let chip = w.find("steer").bounds();
        assert!(w.find("rail").bounds().contains(&chip.center()));
        chip
    });
    window.simulate_mouse_move(chip.center());
    app.advance_clock(Duration::from_millis(200));
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    window.update(|_, w, _| {
        let preview = w.find("preview").bounds();
        assert_eq!(preview.top() - chip.bottom(), px(5.));
        assert!(preview.bottom() <= w.viewport_size().height - px(8.));
    });
}

#[test]
fn root_clear_all_hint_remains_available_while_clear_is_busy() {
    let (app, mut window, queue) = root_fixture(900., 700.);
    window.update(|_, _, cx| {
        queue.update(cx, |q, cx| {
            let mut props = q.props().clone();
            props.busy = true;
            q.set_props(props, cx);
        })
    });
    window.draw();
    let point = window.update(|_, w, _| w.find("clear-tooltip-trigger").bounds().center());
    window.simulate_mouse_move(point);
    app.advance_clock(Duration::from_millis(1000));
    app.run_until_parked();
    window.draw();
    window.update(|_, w, _| {
        assert!(w.find("queue-clear-tooltip").visible());
        assert_eq!(w.find("clear").disabled(), Some(true));
    });
}
