use super::*;
use gpui_kit::{base::Root, test::TestWindowExt};

fn fixture(
    width: f32,
    title: &str,
    dark: bool,
) -> (
    gpui_kit::TestApp,
    gpui_kit::TestAppWindow<Root>,
    Entity<Header>,
) {
    let mut app = gpui_kit::TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let mut window = app.open_window(|w, cx| {
        let header = cx.new(|cx| Header::new("one".into(), title.into(), dark, w, cx));
        Root::new(header, w, cx).flex().flex_col()
    });
    let header = window.read(|root, _| root.view().clone().downcast::<Header>().unwrap());
    window.simulate_resize(gpui_kit::size(px(width), px(120.)));
    window.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    app.run_until_parked();
    window.draw();
    (app, window, header)
}

fn hover_quad(window: &Window, dark: bool) -> bool {
    let bounds = window.find("session-name").bounds();
    window.painted_quads().iter().any(|quad| {
        quad.bounds
            == Bounds {
                origin: bounds.origin.scale(window.scale_factor()),
                size: bounds.size.scale(window.scale_factor()),
            }
            && quad.background == wash(dark).into()
    })
}

#[test]
fn header_root_hover_background_matches_measured_production_vue() {
    // Actual SessionHeader.vue + styles.css + Inter, Chromium and WebKit:
    // header 30px (1px bottom border), button x=7, y=4.5, height=20.
    // Assert the painted Base Button background, not just the label's ink box.
    for dark in [false, true] {
        for width in [240., 800.] {
            for title in ["gypq debugging".to_owned(), "gypq debugging ".repeat(30)] {
                let (_app, mut window, _) = fixture(width, &title, dark);
                window.update(|_, w, _| assert!(!hover_quad(w, dark)));
                let center = window.update(|_, w, _| w.find("session-name").bounds().center());
                window.simulate_mouse_move(center);
                window.draw();
                window.update(|_, w, _| {
                    let name = w.find("session-name").bounds();
                    let header = w.find("header").bounds();
                    assert_eq!(header.size.height, px(30.));
                    assert_eq!(name.size.height, px(20.)); // Negative control: old h(24.) fails.
                    assert_eq!(w.find("session-name").label(), Some(title.as_str()));
                    assert_eq!(name.left() - header.left(), px(7.));
                    assert!((name.top() - header.top() - px(4.5)).abs() < px(0.01));
                    assert!(name.right() <= w.find("new-session").bounds().left() - px(6.));
                    let font = w
                        .text_system()
                        .resolve_font(&gpui_kit::font(font::family()));
                    let cap = w.text_system().advance(font, px(13.), '0').unwrap().width * 60.;
                    assert!(name.size.width <= cap + px(0.01));
                    assert!(
                        hover_quad(w, dark),
                        "hover must paint the measured 20px root bounds"
                    );
                });
            }
        }
    }
}

#[test]
fn header_root_compact_title_keeps_focus_tooltip_and_edge_activation() {
    for edge in [0.5, 19.5] {
        let (app, mut window, header) = fixture(800., "gypq debugging", false);
        window.simulate_keystroke("tab");
        window.draw();
        window.update(|_, w, _| {
            assert_eq!(w.find("session-name").focused(), Some(true));
            assert!(
                hover_quad(w, false),
                "keyboard focus uses the same compact background"
            );
        });
        let point = window.update(|_, w, _| {
            let name = w.find("session-name").bounds();
            Point::new(name.center().x, name.top() + px(edge))
        });
        window.simulate_mouse_move(point);
        app.advance_clock(Duration::from_millis(800));
        window.draw();
        window.update(|_, _, cx| {
            assert_eq!(
                header.read(cx).tooltip_label().as_deref(),
                Some("Rename Session ⌘⇧R")
            );
        });
        window.simulate_click(point, MouseButton::Left);
        window.draw();
        window.update(|_, w, cx| {
            let state = header.read(cx);
            assert!(state.renaming());
            assert!(state.titlebar_gestures.is_empty());
            assert!(state.editor().read(cx).focus_handle(cx).is_focused(w));
            assert_eq!(state.editor().read(cx).value().as_ref(), "gypq debugging");
            let field = w.find("rename-field").bounds();
            let header = w.find("header").bounds();
            // Layout snaps authored lengths, then absolute edges; half ties go toward zero.
            let snap = |value: gpui_kit::Pixels| {
                let scaled = f32::from(value) * w.scale_factor();
                let floor = scaled.floor();
                px((floor + if scaled - floor > 0.5 { 1. } else { 0. }) / w.scale_factor())
            };
            let height = snap(px(21.4));
            let center = header.top() + px(14.5);
            assert_eq!(field.top(), snap(center - height / 2.));
            assert_eq!(field.bottom(), snap(center + height / 2.));
        });
        window.update(|_, _, cx| header.update(cx, |s, _| s.prepare_palette_open()));
        window.update(|_, w, cx| w.blur(cx));
        window.draw();
        window.update(|_, _, cx| assert!(header.read(cx).renaming()));
        window.update(|_, w, cx| {
            header.update(cx, |s, cx| {
                s.finish_palette_focus();
                s.close(false, w, cx);
            });
        });
        window.draw();
        window.update(|_, w, cx| {
            assert!(!header.read(cx).renaming());
            assert_eq!(w.find("session-name").bounds().size.height, px(20.));
        });
    }
}
