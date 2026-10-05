//! Metal-backed activity paint regression. Run on the process main thread.
use gpui_kit::{
    Bounds, Context, Entity, IntoElement, MouseButton, Render, Window, WindowBounds, WindowOptions,
    div, point, prelude::*, px, size, test::TestWindowExt,
};
use tau_gpui_prototype::components::{
    Assets,
    activity::{ActivityContent, ActivityRow},
    init, support,
};

fn tool() -> ActivityContent {
    ActivityContent::Tool {
        name: "read".into(),
        argument: "file.rs".into(),
        arguments: "{\"path\":\"file.rs\"}".into(),
        result: "Found".into(),
        running: false,
        failed: false,
    }
}

// Unlike TestApp, this exercises the Metal painter and the pointer hit-test path.
struct PaintedRow {
    row: Entity<ActivityRow>,
    dark: bool,
}
impl Render for PaintedRow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(self.dark).child(
            div()
                .mt(px(40.))
                .ml(px(40.))
                .w(px(320.))
                .child(self.row.clone()),
        )
    }
}

fn chevron_paint_follows_pointer_and_keyboard_not_clicked_focus() {
    use gpui_kit::{
        AppContext, HeadlessAppContext, InputEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
        platform,
    };
    use std::sync::Arc;

    for dark in [false, true] {
        let mut cx = HeadlessAppContext::with_platform(
            platform::current_platform(true).text_system(),
            Arc::new(Assets),
            platform::current_headless_renderer,
        );
        cx.update(|cx| init(cx, dark));
        let (handle, host) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: Default::default(),
                            size: size(px(420.), px(200.)),
                        })),
                        show: false,
                        focus: false,
                        ..Default::default()
                    },
                    cx,
                    |_, cx| {
                        let row = cx.new(|cx| ActivityRow::new("paint-row", tool(), dark, cx));
                        cx.new(|_| PaintedRow { row, dark })
                    },
                )
            })
            .expect("open Metal test window");
        let row = host.read_with(&cx, |host, _| host.row.clone());
        cx.update_window(handle, |_, window, _| window.activate_window())
            .unwrap();

        let frame = |cx: &mut HeadlessAppContext| {
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.capture_screenshot(handle).expect("capture Metal pixels")
        };
        frame(&mut cx);
        let bounds = cx
            .update_window(handle, |_, window, _| window.find("paint-row").bounds())
            .unwrap();
        let t = support::tokens(dark);
        let measure = |cx: &mut HeadlessAppContext| {
            let image = frame(cx);
            let scale = image.width() as f32 / 420.;
            let ink = |x0: f32, x1: f32, color: u32| {
                let rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
                let mut count = 0;
                for y in ((bounds.origin.y.as_f32() + 3.) * scale) as u32
                    ..((bounds.origin.y.as_f32() + bounds.size.height.as_f32() - 3.) * scale) as u32
                {
                    for x in (x0 * scale) as u32..(x1 * scale) as u32 {
                        let pixel = image.get_pixel(x, y);
                        if (0..3).all(|channel| pixel[channel].abs_diff(rgb[channel]) < 12) {
                            count += 1;
                        }
                    }
                }
                count
            };
            let right = bounds.origin.x.as_f32() + bounds.size.width.as_f32();
            (
                ink(right - 18., right - 6., t.faint),
                ink(
                    bounds.origin.x.as_f32() + 6.,
                    bounds.origin.x.as_f32() + 36.,
                    t.text,
                ),
            )
        };
        let idle = measure(&mut cx);
        assert_eq!(idle.0, 0, "{dark}: idle chevron painted");
        assert_eq!(idle.1, 0, "{dark}: idle name uses hover foreground");

        let inside = bounds.center();
        let outside = point(px(390.), px(150.));
        let move_to = |cx: &mut HeadlessAppContext, position| {
            cx.update_window(handle, |_, window, cx| {
                window.dispatch_event(
                    MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            })
            .unwrap();
        };
        move_to(&mut cx, inside);
        let hovered = measure(&mut cx);
        assert!(
            hovered.0 > 8,
            "{dark}: hovered chevron missing: {hovered:?}"
        );
        assert!(
            hovered.1 > 8,
            "{dark}: hovered name foreground missing: {hovered:?}"
        );
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: inside,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position: inside,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
        })
        .unwrap();
        let clicked = measure(&mut cx);
        assert!(row.read_with(&cx, |row, _| row.expanded()));
        assert!(
            clicked.0 > 8,
            "{dark}: clicked hover chevron missing: {clicked:?}"
        );
        move_to(&mut cx, outside);
        let left = measure(&mut cx);
        assert!(row.read_with(&cx, |row, _| row.expanded()));
        assert!(
            cx.update_window(handle, |_, window, cx| row.read(cx).focused(window))
                .unwrap()
        );
        assert_eq!(
            left.0, 0,
            "{dark}: clicked focus left chevron stuck painted"
        );
        assert_eq!(left.1, 0, "{dark}: name hover foreground stuck painted");

        cx.update_window(handle, |_, window, cx| {
            row.update(cx, |row, cx| row.focus(window, cx));
            window.press("space", cx);
        })
        .unwrap();
        let keyboard = measure(&mut cx);
        assert!(
            cx.update_window(handle, |_, window, _| window.last_input_was_keyboard())
                .unwrap()
        );
        assert!(
            keyboard.0 > 8,
            "{dark}: keyboard chevron missing: {keyboard:?}"
        );
    }
}

fn main() -> anyhow::Result<()> {
    chevron_paint_follows_pointer_and_keyboard_not_clicked_focus();
    Ok(())
}
