use super::*;
use gpui_kit::{MouseButton, TestApp, base::Root, point, size, test::TestWindowExt};

struct Host(Entity<NativeDialog>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(false).size_full().child(self.0.clone())
    }
}

#[test]
fn root_option_cursor_wash_survives_keyboard_pointer_and_admission_states() {
    for dark in [false, true] {
        for embedded in [false, true] {
            let mut app = TestApp::new();
            app.update(|cx| crate::components::init(cx, dark));
            let mut dialog = None;
            let mut window = app.open_window(|w, cx| {
                let entity = cx.new(|cx| {
                    let d = NativeDialog::new(
                        DialogProps {
                            kind: DialogKind::ExtensionSelect,
                            title: "Question".into(),
                            description: "Choose one".into(),
                            placeholder: String::new(),
                            options: (0..3)
                                .map(|i| DialogOption {
                                    id: format!("wash-option-{i}"),
                                    label: format!("Choice {i}"),
                                })
                                .collect(),
                        },
                        dark,
                        w,
                        cx,
                    )
                    .keyboard_submission();
                    if embedded { d.embedded() } else { d }
                });
                dialog = Some(entity.clone());
                let host = cx.new(|_| Host(entity));
                Root::new(host, w, cx)
            });
            let dialog = dialog.unwrap();
            window.simulate_resize(size(px(900.), px(700.)));
            window.update(|_, w, cx| {
                w.activate_window();
                w.bounds_changed(cx);
            });
            for _ in 0..3 {
                window.draw();
            }
            let assert_paint = |window: &mut gpui_kit::TestAppWindow<Root>, active: usize| {
                window.draw();
                window.read(|_, cx| assert_eq!(dialog.read(cx).cursor, active));
                window.update(|_, w, cx| {
                    let d = dialog.read(cx);
                    let opacity = if d.busy || d.readonly { 0.45 } else { 1. };
                    let mut wash: gpui_kit::Hsla = rgba(if dark { 0x47526633 } else { 0x6b7d8f1a }).into();
                    wash.a *= opacity;
                    let mut stripe: gpui_kit::Hsla = rgb(tokens(dark).accent).into();
                    stripe.a *= opacity;
                    let scale = w.scale_factor();
                    for i in 0..3 {
                        let bounds = w.find(format!("wash-option-{i}")).bounds();
                        let row = bounds.scale(scale);
                        // Native hitboxes suppress hover in keyboard input modality.
                        let hovered = !d.busy
                            && !d.readonly
                            && !w.last_input_was_keyboard()
                            && bounds.contains(&w.mouse_position());
                        assert_eq!(
                            w.painted_quads()
                                .iter()
                                .any(|q| q.background == wash.into() && q.bounds == row),
                            i == active || hovered,
                            "dark={dark}, embedded={embedded}, row={i}, active={active}, busy={}, readonly={}", d.busy, d.readonly
                        );
                        assert_eq!(
                            w.painted_quads().iter().any(|q| q.background
                                == stripe.into()
                                && (q.bounds.size.width.0 - px(2.).scale(scale).0).abs() < 0.5
                                && (q.bounds.left().0 - row.left().0).abs() < 0.5
                                && (q.bounds.top().0 - row.top().0 - px(3.).scale(scale).0).abs()
                                    < 0.5
                                && (q.bounds.bottom().0 - row.bottom().0 + px(3.).scale(scale).0)
                                    .abs()
                                    < 0.5),
                            i == active
                        );
                    }
                });
            };
            assert_paint(&mut window, 0);
            window.simulate_keystroke("down");
            assert_paint(&mut window, 1);
            window.simulate_keystroke("up");
            assert_paint(&mut window, 0);
            let at = window.update(|_, w, _| w.find("wash-option-2").bounds().center());
            window.simulate_mouse_move(at);
            assert_paint(&mut window, 2);
            // Keyboard modality suppresses native hover without moving the pointer.
            window.simulate_keystroke("up");
            assert_paint(&mut window, 1);
            window.simulate_keystroke("down");
            assert_paint(&mut window, 2);
            // Move outside without changing focus: the cursor is not transient hover.
            window.simulate_mouse_move(point(px(1.), px(1.)));
            assert_paint(&mut window, 2);
            for readonly in [false, true] {
                window.update(|_, _, cx| {
                    dialog.update(cx, |d, cx| {
                        d.set_busy(!readonly, cx);
                        d.set_readonly(readonly, cx);
                    })
                });
                window.draw();
                let other = window.update(|_, w, _| w.find("wash-option-0").bounds().center());
                window.simulate_mouse_move(other);
                window.simulate_click(other, MouseButton::Left);
                window.simulate_keystroke("down");
                assert_paint(&mut window, 2);
                window.read(|_, cx| assert!(dialog.read(cx).selected().is_empty()));
            }
            window.update(|_, w, cx| w.blur(cx));
            app.run_until_parked();
        }
    }
}
