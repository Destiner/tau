//! Offscreen Metal regression: the production Composer and Base Root, never an OS window.
//! Glyph coverage is checked in rendered pixels; selection/caret also assert scene masks.
use gpui_kit::{
    AppContext, Bounds, Context, Entity, HeadlessAppContext, InputEvent, IntoElement, Render,
    ScrollDelta, ScrollWheelEvent, TouchPhase, Window, WindowBounds, WindowOptions, base::Root,
    div, platform, point, prelude::*, px, rgb, size, test::TestWindowExt,
};
use std::sync::Arc;
use tau_gpui::components::{
    Assets,
    composer::{Composer, ComposerOptions},
    init, support,
};

const WIDTH: f32 = 420.;
const HEIGHT: f32 = 400.;
const INK: u32 = 0xe619bd;
const SELECTION: u32 = 0x05b8ce;
const CARET: u32 = 0x18c829;

struct Host {
    composer: Entity<Composer>,
    dark: bool,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(self.dark).child(
            div()
                .size_full()
                .p(px(24.))
                .flex()
                .flex_col()
                .text_color(rgb(INK))
                .child(div().h(px(40.)).flex_shrink_0())
                .child(self.composer.clone())
                .child(div().flex_1()),
        )
    }
}

fn regression(dark: bool) {
    let mut cx = HeadlessAppContext::with_platform(
        platform::current_platform(true).text_system(),
        Arc::new(Assets),
        platform::current_headless_renderer,
    );
    cx.update(|cx| init(cx, dark));
    // One long paragraph followed by hard lines exercises wrapping and the row cap together.
    let draft = format!(
        "gypqj first row\n{}\n{}gypqj final row",
        "gypqj café wrapped words ".repeat(140),
        "gypqj café hard line\n".repeat(40)
    );
    let mut host = None;
    let (handle, _) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(WIDTH), px(HEIGHT)),
                    ))),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(|cx| Host {
                        composer: cx.new(|cx| {
                            Composer::new(
                                ComposerOptions {
                                    models: vec![],
                                    model_heading: "Models".into(),
                                    efforts: vec![],
                                    model: String::new(),
                                    effort: String::new(),
                                    draft: draft.clone(),
                                    dark,
                                    disabled: false,
                                    streaming: false,
                                },
                                window,
                                cx,
                            )
                        }),
                        dark,
                    });
                    host = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
        })
        .expect("open offscreen Metal root");
    let host = host.unwrap();
    let composer = host.read_with(&cx, |host, _| host.composer.clone());
    let editor = composer.read_with(&cx, |composer, _| composer.editor());
    cx.update_window(handle, |_, window, cx| {
        window.activate_window();
        composer.update(cx, |composer, cx| {
            // Only colours distinguish the paint passes; font, leading and padding are real.
            let mut style = support::input_style(dark);
            style.macos_textarea_selection = true;
            style.auto_grow_cursor_margin = Some(px(13.));
            style.disabled_opacity = 0.45;
            style.foreground = rgb(INK).into();
            style.selection = rgb(SELECTION).into();
            style.caret = rgb(CARET).into();
            composer.set_editor_style(style, cx);
        });
        editor.update(cx, |editor, cx| {
            editor.focus(window, cx);
            editor.set_selected_range(0..0, cx);
            editor.set_scroll_offset(point(px(0.), px(0.)), cx);
        });
    })
    .unwrap();

    let frame = |cx: &mut HeadlessAppContext| {
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
        cx.capture_screenshot(handle)
            .expect("offscreen Metal pixels")
    };
    frame(&mut cx);
    let viewport = cx
        .update_window(handle, |_, window, _| window.find("editor").bounds())
        .unwrap();
    assert!(viewport.size.height <= px(196.));
    assert!(viewport.top() > px(24.));
    editor.read_with(&cx, |state, _| {
        let content = state.input_bounds();
        assert_eq!(content.top() - viewport.top(), px(4.));
        assert!(
            ((viewport.bottom() - content.bottom()) - px(3.)).abs() <= px(1.),
            "rounded bottom padding: viewport={viewport:?}, content={content:?}"
        );
        assert_eq!(content.left() - viewport.left(), px(4.));
        assert_eq!(viewport.right() - content.right(), px(4.));
    });

    let inspect = |cx: &mut HeadlessAppContext, require_selection: bool, require_caret: bool| {
        let image = frame(cx);
        let scale = image.width() as f32 / WIDTH;
        let mask = viewport.scale(scale);
        let inside = |x: u32, y: u32| {
            x as f32 >= mask.left().as_f32().floor()
                && (x as f32) < mask.right().as_f32().ceil()
                && y as f32 >= mask.top().as_f32().floor()
                && (y as f32) < mask.bottom().as_f32().ceil()
        };
        let mut ink = 0;
        let mut selected = 0;
        for (x, y, pixel) in image.enumerate_pixels() {
            for (color, count) in [(INK, &mut ink), (SELECTION, &mut selected)] {
                let channels = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
                if (0..3).all(|c| pixel[c].abs_diff(channels[c]) < 18) {
                    assert!(
                        inside(x, y),
                        "paint {color:x} escaped viewport at {x},{y}, dark={dark}"
                    );
                    *count += 1;
                }
            }
        }
        assert!(
            ink > 80,
            "must exercise real rasterized glyph ink, not an empty scene: ink={ink}, selection={selected}, dark={dark}, viewport={viewport:?}"
        );
        if require_selection {
            assert!(selected > 80, "selection must actually be visible");
        }
        cx.update_window(handle, |_, window, _| {
            let padded_viewport_mask = |bounds: Bounds<gpui_kit::ScaledPixels>| {
                // Equality also rejects clipping to the inner text box: padding is ink safety,
                // not a reason to shave the first/last row's descenders or caret.
                for (actual, expected) in [
                    (bounds.left(), mask.left()),
                    (bounds.right(), mask.right()),
                    (bounds.top(), mask.top()),
                    (bounds.bottom(), mask.bottom()),
                ] {
                    assert!((actual.as_f32() - expected.as_f32()).abs() <= 1.);
                }
            };
            let paths = window.painted_paths();
            let selection: Vec<_> = paths
                .iter()
                .filter(|p| p.color == rgb(SELECTION).into())
                .collect();
            if require_selection {
                assert!(
                    !selection.is_empty(),
                    "selection path must reach the painter"
                );
            }
            for path in selection {
                padded_viewport_mask(path.content_mask.bounds);
            }
            let quads = window.painted_quads();
            let carets: Vec<_> = quads
                .iter()
                .filter(|q| q.background == rgb(CARET).into())
                .collect();
            if require_caret {
                assert!(!carets.is_empty(), "focused caret must reach the painter");
            }
            for caret in carets {
                padded_viewport_mask(caret.content_mask.bounds);
            }
        })
        .unwrap();
    };
    inspect(&mut cx, false, true);

    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| editor.select_all(window, cx));
    })
    .unwrap();
    frame(&mut cx);
    // First discover the clamped lower endpoint using the actual shaped draft.
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(-1_000_000.), px(-1_000_000.)), cx)
        });
    })
    .unwrap();
    frame(&mut cx);
    let bottom = editor.read_with(&cx, |editor, _| editor.scroll_offset());
    assert_eq!(
        bottom.x,
        px(0.),
        "wrapped textarea must not acquire horizontal scroll"
    );
    assert!(
        bottom.y < -viewport.size.height * 2.,
        "draft must exceed the cap"
    );

    for y in [
        px(0.),
        px(-37.25),
        bottom.y / 2.,
        bottom.y,
        bottom.y / 2.,
        px(-37.25),
        px(0.),
        bottom.y,
    ] {
        cx.update_window(handle, |_, _, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_scroll_offset(point(px(-123.), y), cx)
            });
        })
        .unwrap();
        inspect(&mut cx, true, false);
        editor.read_with(&cx, |state, _| {
            assert_eq!(state.value().to_string(), draft);
            assert_eq!(state.selected_range(), 0..draft.len());
            assert_eq!(state.scroll_offset().x, px(0.));
            assert!((state.scroll_offset().y - y).abs() < px(1.));
        });
        assert_eq!(composer.read_with(&cx, |c, _| c.editor()), editor);
    }
    // Both positive axes clamp back to the top without changing the draft/selection.
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(1_000_000.), px(1_000_000.)), cx)
        });
    })
    .unwrap();
    inspect(&mut cx, true, false);
    assert_eq!(
        editor.read_with(&cx, |e, _| e.scroll_offset()),
        point(px(0.), px(0.))
    );
    // Exercise the wheel listener too, including reversal and fractional pixel deltas.
    for delta in [-74.5, -91., 33.25, 1_000_000., -1_000_000.] {
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position: viewport.center(),
                    delta: ScrollDelta::Pixels(point(px(-31.), px(delta))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }
                .to_platform_input(),
                cx,
            );
        })
        .unwrap();
        inspect(&mut cx, true, false);
        editor.read_with(&cx, |e, _| {
            assert_eq!(e.value().to_string(), draft);
            assert_eq!(e.selected_range(), 0..draft.len());
            assert_eq!(e.scroll_offset().x, px(0.));
            assert!(e.scroll_offset().y >= bottom.y && e.scroll_offset().y <= px(0.));
        });
    }
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(draft.len()..draft.len(), cx);
            editor.set_scroll_offset(bottom, cx);
            editor.focus(window, cx);
        });
    })
    .unwrap();
    inspect(&mut cx, false, true);
}

fn main() {
    for dark in [false, true] {
        regression(dark);
    }
    println!(
        "PASS retained Root composer: wrapped/multiline ink, selection/caret masks, two-axis scroll bounds"
    );
}
