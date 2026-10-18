//! Source-authored offscreen Metal regression; run only in the parent's exclusive slot.
use gpui_kit::{
    AppContext, Bounds, Context, Entity, EntityInputHandler, HeadlessAppContext, InputEvent,
    IntoElement, KeyUpEvent, Keystroke, Render, Window, WindowBounds, WindowOptions, base::Root,
    div, platform, point, prelude::*, px, rgb, size, test::TestWindowExt,
};
use std::{cell::RefCell, sync::Arc};
use tau_gpui_prototype::components::{
    Assets,
    composer::{Composer, ComposerOptions},
    init, support,
};

const WIDTH: f32 = 420.;
const HEIGHT: f32 = 400.;
const INK: u32 = 0xe619bd;
const CARET: u32 = 0x18c829;
struct Host {
    composer: Entity<Composer>,
    dark: bool,
    fraction: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(self.dark).child(
            div()
                .size_full()
                .text_color(rgb(INK))
                .p(px(24.))
                .flex()
                .flex_col()
                .child(div().h(px(40. + self.fraction)).flex_shrink_0())
                .child(self.composer.clone())
                .child(div().flex_1()),
        )
    }
}
fn key(window: &mut Window, cx: &mut gpui_kit::App, text: &str) {
    let keystroke = Keystroke::parse(text).expect("test key");
    // Dispatches actual KeyDown plus the platform text fallback if no action consumes it.
    window.dispatch_keystroke(keystroke.clone(), cx);
    window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
}
fn regression(dark: bool, wrapped: bool, fraction: f32) {
    let mut cx = HeadlessAppContext::with_platform(
        platform::current_platform(true).text_system(),
        Arc::new(Assets),
        platform::current_headless_renderer,
    );
    cx.update(|cx| init(cx, dark));
    let draft = if wrapped {
        format!("{}gypqj tail", "gypqj café wrapped words ".repeat(70))
    } else {
        format!("{}gypqj tail", "gypqj hard line\n".repeat(24))
    };
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
                        fraction,
                    });
                    host = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
        })
        .expect("offscreen Root");
    let composer = host.unwrap().read_with(&cx, |h, _| h.composer.clone());
    let editor = composer.read_with(&cx, |c, _| c.editor());
    cx.update_window(handle, |_, window, cx| {
        window.activate_window();
        composer.update(cx, |c, cx| {
            let mut style = support::input_style(dark);
            style.auto_grow_cursor_margin = Some(px(13.));
            style.macos_textarea_selection = true;
            style.foreground = rgb(INK).into();
            style.caret = rgb(CARET).into();
            c.set_editor_style(style, cx);
        });
        editor.update(cx, |e, cx| {
            e.focus(window, cx);
            e.set_selected_range(draft.len()..draft.len(), cx);
            e.set_scroll_offset(point(px(0.), px(-1_000_000.)), cx);
        });
    })
    .unwrap();
    let frame = |cx: &mut HeadlessAppContext| {
        cx.run_until_parked();
        for _ in 0..2 {
            cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
        cx.capture_screenshot(handle).expect("Metal readback")
    };
    frame(&mut cx);
    let initial_viewport = cx
        .update_window(handle, |_, w, _| w.find("editor").bounds())
        .unwrap();
    assert!(initial_viewport.size.height <= px(196.));
    assert!(editor.read_with(&cx, |e, _| e.scroll_offset().y) < -initial_viewport.size.height);
    // Caret-driven reveal only: a stationary caret must not reclaim a wheel/programmatic offset.
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |e, cx| {
            e.set_scroll_offset(point(px(0.), px(-37.25)), cx);
        })
    })
    .unwrap();
    frame(&mut cx);
    assert_eq!(
        editor.read_with(&cx, |e, _| e.scroll_offset().y),
        px(-37.25)
    );
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |e, cx| {
            e.set_scroll_offset(point(px(0.), px(-1_000_000.)), cx);
        })
    })
    .unwrap();
    frame(&mut cx);
    cx.update_window(handle, |_, w, cx| key(w, cx, "shift-enter"))
        .unwrap();
    let newline = format!("{draft}\n");
    editor.read_with(&cx, |e, _| {
        assert_eq!(e.value().as_ref(), newline);
        assert_eq!(e.selected_range(), newline.len()..newline.len());
    });
    // Check the empty final row's caret before typing any glyph or manual scroll.
    let failures = RefCell::new(Vec::new());
    macro_rules! check {
        ($condition:expr, $($message:tt)*) => {
            if !$condition { failures.borrow_mut().push(format!($($message)*)); }
        };
    }
    let inspect = |cx: &mut HeadlessAppContext, label: &str| {
        let image = if label.ends_with("first") {
            // No executor pump or hidden layout frame before this first complete paint.
            cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                .unwrap();
            cx.capture_screenshot(handle).expect("first Metal readback")
        } else {
            frame(cx)
        };
        if let Ok(directory) = std::env::var("TAU_COMPOSER_TAIL_CAPTURE") {
            std::fs::create_dir_all(&directory).unwrap();
            image
                .save(format!(
                    "{directory}/{dark}-{wrapped}-{fraction}-{label}.png"
                ))
                .unwrap();
        }
        let scale = image.width() as f32 / WIDTH;
        let caret = cx
            .update_window(handle, |_, w, _| {
                // Never cache viewport geometry across input/layout.
                let viewport = w.find("editor").bounds().scale(scale);
                let carets: Vec<_> = w
                    .painted_quads()
                    .into_iter()
                    .filter(|q| q.background == rgb(CARET).into())
                    .collect();
                assert_eq!(carets.len(), 1, "{label}: visible focused caret");
                let c = &carets[0];
                check!(
                    c.bounds.top() >= viewport.top() && c.bounds.bottom() <= viewport.bottom(),
                    "{label}: caret clipped: {:?} vs {viewport:?}",
                    c.bounds
                );
                assert!(
                    (c.content_mask.bounds.left() - viewport.left())
                        .as_f32()
                        .abs()
                        <= 1.
                );
                assert!(
                    (c.content_mask.bounds.right() - viewport.right())
                        .as_f32()
                        .abs()
                        <= 1.
                );
                assert!(
                    (c.content_mask.bounds.top() - viewport.top())
                        .as_f32()
                        .abs()
                        <= 1.
                );
                assert!(
                    (c.content_mask.bounds.bottom() - viewport.bottom())
                        .as_f32()
                        .abs()
                        <= 1.
                );
                (c.bounds, viewport)
            })
            .unwrap();
        let (caret, viewport) = caret;
        let row_height = caret.size.height.as_f32() / 0.85;
        let row_top = caret.top().as_f32() - row_height * 0.075;
        let row_bottom = row_top + row_height;
        check!(
            row_top >= viewport.top().as_f32() && row_bottom <= viewport.bottom().as_f32(),
            "{label}: full row clipped: {row_top}..{row_bottom} vs {viewport:?}"
        );
        let mut tail_ink = 0usize;
        let mut caret_pixels = 0usize;
        for (x, y, p) in image.enumerate_pixels() {
            if p[0].abs_diff(0x18) < 18 && p[1].abs_diff(0xc8) < 18 && p[2].abs_diff(0x29) < 18 {
                assert!(
                    x as f32 >= viewport.left().as_f32().floor()
                        && (x as f32) < viewport.right().as_f32().ceil()
                        && y as f32 >= viewport.top().as_f32().floor()
                        && (y as f32) < viewport.bottom().as_f32().ceil(),
                    "{label}: caret pixels escaped viewport"
                );
                caret_pixels += 1;
            }
            if p[0].abs_diff(0xe6) < 18 && p[1].abs_diff(0x19) < 18 && p[2].abs_diff(0xbd) < 18 {
                assert!(
                    x as f32 >= viewport.left().as_f32().floor()
                        && (x as f32) < viewport.right().as_f32().ceil()
                        && y as f32 >= viewport.top().as_f32().floor()
                        && (y as f32) < viewport.bottom().as_f32().ceil(),
                    "{label}: ink escaped padded viewport"
                );
                if y as f32 >= row_top.floor() && (y as f32) < row_bottom.ceil() {
                    tail_ink += 1;
                }
            }
        }
        assert!(caret_pixels > 10, "{label}: caret must actually rasterize");
        println!(
            "CASE dark={dark} wrapped={wrapped} fraction={fraction} stage={label} scale={scale} row={row_top}..{row_bottom} viewport={viewport:?} caret={caret:?} ink={tail_ink} caret_pixels={caret_pixels}"
        );
        tail_ink
    };
    inspect(&mut cx, "newline-first");
    inspect(&mut cx, "newline");
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |e, _| e.break_undo_transaction())
    })
    .unwrap();
    // Real key-down/up for text too, not a draft replacement.
    for text in ["g", "y", "p", "q", "j"] {
        cx.update_window(handle, |_, w, cx| key(w, cx, text))
            .unwrap();
        inspect(&mut cx, &format!("typed-{text}-first"));
    }
    let typed = format!("{newline}gypqj");
    let ink = inspect(&mut cx, "typed");
    check!(
        ink > 20,
        "final-row descender glyphs must actually rasterize"
    );
    editor.read_with(&cx, |e, _| {
        assert_eq!(e.value().as_ref(), typed);
        assert_eq!(e.selected_range(), typed.len()..typed.len());
    });
    // An integer-pixel scroll exposes a negative control without changing glyph shaping.
    let before = editor.read_with(&cx, |e, _| e.scroll_offset());
    cx.update_window(handle, |_, _, cx| {
        editor.update(cx, |e, cx| {
            e.set_scroll_offset(point(before.x, before.y - px(8.)), cx);
        })
    })
    .unwrap();
    let exposed = inspect(&mut cx, "manual-scroll");
    check!(
        ink >= exposed,
        "manual scroll recovered clipped final glyph ink: {ink} -> {exposed}"
    );
    cx.update_window(handle, |_, w, cx| key(w, cx, "cmd-z"))
        .unwrap();
    frame(&mut cx);
    editor.read_with(&cx, |e, _| assert_eq!(e.value().as_ref(), newline));
    cx.update_window(handle, |_, w, cx| key(w, cx, "cmd-shift-z"))
        .unwrap();
    frame(&mut cx);
    editor.read_with(&cx, |e, _| assert_eq!(e.value().as_ref(), typed));
    cx.update_window(handle, |_, w, cx| {
        editor.update(cx, |e, cx| {
            e.break_undo_transaction();
            e.replace_and_mark_text_in_range(None, "に", Some(1..1), w, cx);
            assert!(e.marked_text_range(w, cx).is_some());
        })
    })
    .unwrap();
    inspect(&mut cx, "marked-first");
    inspect(&mut cx, "marked");
    cx.update_window(handle, |_, w, cx| {
        editor.update(cx, |e, cx| {
            assert!(e.marked_text_range(w, cx).is_some());
            e.replace_text_in_range(None, "日本", w, cx);
            assert!(e.marked_text_range(w, cx).is_none());
        })
    })
    .unwrap();
    frame(&mut cx);
    cx.update_window(handle, |_, w, cx| key(w, cx, "cmd-z"))
        .unwrap();
    frame(&mut cx);
    editor.read_with(&cx, |e, _| assert_eq!(e.value().as_ref(), typed));
    assert_eq!(composer.read_with(&cx, |c, _| c.editor()), editor);
    assert!(
        failures.borrow().is_empty(),
        "{}",
        failures.borrow().join("\n")
    );
}
fn main() {
    let mut failed = 0;
    for dark in [false, true] {
        for wrapped in [false, true] {
            for fraction in [0., 0.25, 0.75] {
                if std::panic::catch_unwind(|| regression(dark, wrapped, fraction)).is_err() {
                    failed += 1;
                    eprintln!("FAILED dark={dark} wrapped={wrapped} fraction={fraction}");
                }
            }
        }
    }
    assert_eq!(
        failed, 0,
        "{failed}/12 composer-tail cases failed; captures retained"
    );
    println!(
        "PASS 12 Root Composer tail cases: first/settled newline and text, full row, descenders/caret, masks, history/IME, stationary scroll"
    );
}
