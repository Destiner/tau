//! Real shell/Root surfaces: input and delayed feedback, not presentation metadata.
#[path = "tooltip_geometry_tests.rs"]
mod geometry;
use super::*;
use crate::components::support;
use gpui_kit::{MouseButton, px, rgb, test::TestWindowExt};

fn assert_feedback_paint(window: &mut Window, dark: bool) {
    let bounds = window.find("file-link-feedback").bounds();
    let label = window.find("file-link-feedback-label").bounds();
    assert_eq!(label.left() - bounds.left(), px(9.));
    assert_eq!(label.top() - bounds.top(), px(6.));
    assert_eq!(bounds.right() - label.right(), px(9.));
    assert_eq!(bounds.bottom() - label.bottom(), px(6.));
    assert_eq!(label.size.height, px(13.));
    assert_eq!(bounds.size.height, px(25.));
    assert!(bounds.size.width <= px(240.));
    let scale = window.scale_factor();
    let quads = window.painted_quads();
    let quad = quads
        .iter()
        .find(|q| {
            (q.bounds.left().0 / scale - f32::from(bounds.left())).abs() < 0.01
                && (q.bounds.top().0 / scale - f32::from(bounds.top())).abs() < 0.01
                && (q.bounds.size.width.0 / scale - f32::from(bounds.size.width)).abs() < 0.01
                && q.background == rgb(support::tokens(dark).panel_raised).into()
        })
        .expect("feedback must paint the production raised surface");
    assert_eq!(
        quads
            .iter()
            .filter(|q| q.bounds == quad.bounds && q.background == quad.background)
            .count(),
        1
    );
    let border = quads
        .iter()
        .find(|q| q.bounds == quad.bounds && q.border_widths.top.0 / scale == 1.)
        .expect("feedback must paint a border at the surface bounds");
    assert_eq!(border.border_color, support::tooltip_border(dark).into());
    assert_eq!(border.border_widths.top.0 / scale, 1.);
    assert_eq!(quad.corner_radii.top_left.0 / scale, 6.);
    assert_eq!(quad.corner_radii.bottom_right.0 / scale, 6.);
}

#[test]
fn root_link_feedback_paints_markdown_surface_after_real_click_and_200ms() {
    for dark in [false, true] {
        let (app, mut window, shell) = super::tests::production_link_fixture();
        window.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.preferences.theme = if dark { "dark" } else { "light" }.into();
                s.sync(w, cx);
                cx.notify();
            })
        });
        window.draw();
        // Use the genuine production link hit box; dismiss only the menu opened
        // by the existing fixture's right-click helper before left activation.
        let anchor = super::tests::production_context_point(&mut window, &shell);
        window.draw();
        window.simulate_keystroke("escape");
        app.run_until_parked();
        window.draw();
        let (prepared, receiver) = async_channel::unbounded();
        window.update(|_, _, cx| {
            shell.update(cx, |s, _| {
                s.preview.prepare_receiver = Some(receiver);
            })
        });
        window.simulate_click(anchor, MouseButton::Left);
        app.run_until_parked();
        // Activation may legitimately focus the transcript; delayed feedback must not.
        let focus = window.update(|_, w, cx| w.focused(cx));
        assert!(
            focus.is_some(),
            "real link activation must leave a valid focus target"
        );
        app.advance_clock(Duration::from_millis(199));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert!(w.try_find("file-link-feedback").is_none());
            assert_eq!(w.focused(cx), focus);
        });
        app.advance_clock(Duration::from_millis(1));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert_eq!(shell.read(cx).preview_feedback(), Some("Opening…"));
            assert_eq!(w.focused(cx), focus);
            assert_feedback_paint(w, dark);
        });
        prepared
            .try_send(Err("synthetic unavailable".into()))
            .unwrap();
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert_eq!(
                shell.read(cx).preview_feedback(),
                Some("Preview unavailable")
            );
            assert_eq!(w.focused(cx), focus);
            assert_feedback_paint(w, dark);
        });
        app.advance_clock(Duration::from_millis(1_200));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert!(w.try_find("file-link-feedback").is_none());
            assert_eq!(w.focused(cx), focus);
        });
    }
}

#[test]
fn root_composer_real_hover_paints_compact_surface_and_distinct_shortcut() {
    for dark in [false, true] {
        let (app, mut window, shell) = super::tests::production_link_fixture();
        window.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.preferences.theme = if dark { "dark" } else { "light" }.into();
                s.sync(w, cx);
                cx.notify();
            })
        });
        window.draw();
        let anchor = window.update(|_, w, _| w.find("send-tooltip-trigger").bounds().center());
        let focus = window.update(|_, w, cx| w.focused(cx));
        window.simulate_mouse_move(anchor);
        app.run_until_parked();
        app.advance_clock(Duration::from_millis(449));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, _| assert!(w.try_find("composer-tooltip-shortcut").is_none()));
        app.advance_clock(Duration::from_millis(1));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert_eq!(w.focused(cx), focus);
            let shortcut = w.find("composer-tooltip-shortcut").bounds();
            assert_eq!(shortcut.size.height, px(16.));
            let scale = w.scale_factor();
            let quads = w.painted_quads();
            let quad = quads
                .iter()
                .find(|q| {
                    q.background == rgb(support::tokens(dark).panel_raised).into()
                        && q.bounds.size.width.0 / scale <= 260.
                        && q.bounds.left().0 / scale < f32::from(shortcut.left())
                        && q.bounds.right().0 / scale > f32::from(shortcut.right())
                        && q.bounds.top().0 / scale < f32::from(shortcut.top())
                        && q.bounds.bottom().0 / scale > f32::from(shortcut.bottom())
                })
                .expect("hover must paint the compact tooltip, not just set a label");
            assert_eq!(quad.corner_radii.top_left.0 / scale, 4.);
            assert_eq!(
                quads
                    .iter()
                    .filter(|q| {
                        q.background == quad.background
                            && q.bounds.left() <= shortcut.scale(scale).left()
                            && q.bounds.right() >= shortcut.scale(scale).right()
                            && q.bounds.top() <= shortcut.scale(scale).top()
                            && q.bounds.bottom() >= shortcut.scale(scale).bottom()
                    })
                    .count(),
                1,
                "only one tooltip backplate may surround the shortcut"
            );
            let border = quads
                .iter()
                .find(|q| q.bounds == quad.bounds && q.border_widths.top.0 / scale == 1.)
                .expect("compact tooltip must paint its border at the fill bounds");
            assert_eq!(border.border_widths.top.0 / scale, 1.);
            assert_eq!(border.border_color, support::tooltip_border(dark).into());
            assert_eq!(quad.bounds.size.height.0 / scale, 24.);
            assert_eq!(f32::from(shortcut.top()) - quad.bounds.top().0 / scale, 4.);
        });
        window.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)));
        window.draw();
        window.update(|_, w, _| assert!(w.try_find("composer-tooltip-shortcut").is_none()));
    }
}

#[test]
fn first_run_root_tooltips_paint_distinct_vue_shortcut_variants() {
    for dark in [false, true] {
        let mut app = crate::app::tests::app();
        let (commands, _requests) = async_channel::bounded(64);
        let shell = app.new_entity(|_| {
            let mut s = crate::app::tests::shell(commands);
            s.loaded = true;
            s.preferences.theme = if dark { "dark" } else { "light" }.into();
            s.workspace = Some(crate::app::WorkspaceSnapshot {
                projects: Vec::new(),
                active_project_path: String::new(),
                pi_path: None,
            });
            s
        });
        let mut window = app.open_window(|w, cx| {
            shell.update(cx, |s, cx| {
                s.window = Some(w.window_handle());
                s.mount(w, cx);
                s.sync(w, cx);
            });
            gpui_kit::base::Root::new(shell.clone(), w, cx)
        });
        window.draw();
        for (trigger, radius, height) in [
            ("first-run-project", 3., 16.),
            ("first-run-palette", 3., 16.5),
        ] {
            let anchor = window.update(|_, w, _| w.find(trigger).bounds().center());
            window.simulate_mouse_move(anchor);
            app.run_until_parked();
            app.advance_clock(Duration::from_millis(450));
            app.run_until_parked();
            window.draw();
            window.update(|_, w, _| {
                let shortcut = w.find("first-run-tooltip-shortcut").bounds();
                // Palette inherits 10px × 1.25 leading, plus 2px padding and 2px border.
                assert_eq!(shortcut.size.height, px(height));
                if trigger == "first-run-palette" {
                    // WebKit's plain monospace kbd resolves to Courier, not the body font.
                    assert!(
                        (22. ..23.).contains(&f32::from(shortcut.size.width)),
                        "palette shortcut must retain the measured monospace width: {shortcut:?}"
                    );
                }
                let scale = w.scale_factor();
                let quads = w.painted_quads();
                let kbd = quads
                    .iter()
                    .find(|q| {
                        (q.bounds.left().0 / scale - f32::from(shortcut.left())).abs() < 0.01
                            && (q.bounds.top().0 / scale - f32::from(shortcut.top())).abs() < 0.01
                            && q.border_widths.top.0 / scale == 1.
                    })
                    .expect("the shortcut must be a painted kbd, not inline dot-separated copy");
                assert_eq!(kbd.corner_radii.top_left.0 / scale, radius);
                let expected_border = if trigger == "first-run-palette" {
                    support::tooltip_border(dark)
                } else {
                    gpui_kit::Rgba {
                        a: 0.35,
                        ..rgb(support::tokens(dark).muted)
                    }
                };
                assert_eq!(kbd.border_color, expected_border.into());
            });
            window.simulate_mouse_move(gpui_kit::point(px(500.), px(300.)));
            app.run_until_parked();
            app.advance_clock(Duration::from_millis(300));
            app.run_until_parked();
            window.draw();
        }
    }
}
