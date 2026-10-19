//! Main-thread, Metal-backed paint regressions (`harness = false`).
//! No screenshot files, visible windows, Pi, network, or user content.

#[cfg(target_os = "macos")]
fn main() {
    metal::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("diagram-paint requires macOS and an available Metal renderer");
}

#[cfg(target_os = "macos")]
mod metal {
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, Context, Entity, HeadlessAppContext, InputEvent, IntoElement, Render,
        Window, WindowBounds, WindowOptions, div, platform, prelude::*, px, size,
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use tau_gpui::components::{Assets, diagram_view::DiagramBlock, init, support};

    struct Host {
        block: Entity<DiagramBlock>,
        dark: bool,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            support::root(self.dark).child(
                div()
                    .w(px(320.))
                    .mt(px(40.))
                    .ml(px(40.))
                    .child(self.block.clone()),
            )
        }
    }
    struct CopyHost {
        editor: Entity<gpui_kit::base::input::InputState>,
        markdown: Entity<tau_gpui::components::markdown::MarkdownView>,
        dark: bool,
    }
    impl Render for CopyHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            support::root(self.dark)
                .p(px(40.))
                .flex_col()
                .child(
                    div()
                        .h(px(32.))
                        .child(gpui_kit::base::input::Input::new(&self.editor)),
                )
                .child(self.markdown.clone())
        }
    }

    fn copy_paint(dark: bool) {
        use tau_gpui::components::markdown::MarkdownView;
        let mut cx = HeadlessAppContext::with_platform(
            platform::current_platform(true).text_system(),
            Arc::new(Assets),
            platform::current_headless_renderer,
        );
        cx.update(|cx| init(cx, dark));
        let (handle, _) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: Default::default(),
                            size: size(px(900.), px(700.)),
                        })),
                        show: false,
                        focus: false,
                        ..Default::default()
                    },
                    cx,
                    |w, cx| {
                        cx.new(|cx| CopyHost {
                            editor: cx.new(|cx| gpui_kit::base::input::InputState::new(w, cx)),
                            markdown: cx.new(|cx| {
                                MarkdownView::new(
                                    "plain text\n\n```rs\nlet a = 1;\n```",
                                    dark,
                                    |_| {},
                                    cx,
                                )
                            }),
                            dark,
                        })
                    },
                )
            })
            .unwrap();
        let (button, scale) = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                assert!(!w.find("copy-paint-1").visible());
                (w.find("copy-1").bounds(), w.scale_factor())
            })
            .unwrap();
        assert_eq!(button.size, size(px(21.), px(21.)));
        macro_rules! crop {
            ($frame:expr) => {
                $frame
                    .enumerate_pixels()
                    .filter(|(x, y, _)| {
                        *x as f32 >= f32::from(button.left()) * scale
                            && (*x as f32) < f32::from(button.right()) * scale
                            && *y as f32 >= f32::from(button.top()) * scale
                            && (*y as f32) < f32::from(button.bottom()) * scale
                    })
                    .map(|(_, _, p)| p.0)
                    .collect::<Vec<_>>()
            };
        }
        let idle = crop!(cx.capture_screenshot(handle).unwrap());
        // Use a real input click as the non-button origin; never mutate focus in a case.
        cx.update_window(handle, |_, w, cx| {
            let position = gpui_kit::point(px(60.), px(50.));
            w.dispatch_event(
                gpui_kit::MouseMoveEvent {
                    position,
                    modifiers: Default::default(),
                    pressed_button: None,
                }
                .to_platform_input(),
                cx,
            );
            w.dispatch_event(
                gpui_kit::MouseDownEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            w.dispatch_event(
                gpui_kit::MouseUpEvent {
                    position,
                    button: gpui_kit::MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            w.press("alt-tab", cx);
            assert_eq!(w.find("copy-1").focused(), Some(true));
            assert!(w.find("copy-paint-1").visible());
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| w.render_frame(cx))
            .unwrap();
        let keyboard_frame = cx.capture_screenshot(handle).unwrap();
        let keyboard = crop!(keyboard_frame);
        assert!(
            idle != keyboard,
            "keyboard navigation reveals actual copy ink"
        );
        cx.update_window(handle, |_, w, cx| {
            w.dispatch_event(
                gpui_kit::MouseMoveEvent {
                    position: button.center(),
                    modifiers: Default::default(),
                    pressed_button: None,
                }
                .to_platform_input(),
                cx,
            );
            w.render_frame(cx);
        })
        .unwrap();
        let full = crop!(cx.capture_screenshot(handle).unwrap());
        assert!(
            keyboard == full,
            "keyboard and button hover use full copy opacity"
        );
        cx.update_window(handle, |_, w, cx| {
            w.dispatch_event(
                gpui_kit::MouseMoveEvent {
                    position: gpui_kit::point(button.left() - px(40.), button.top() + px(10.)),
                    modifiers: Default::default(),
                    pressed_button: None,
                }
                .to_platform_input(),
                cx,
            );
            w.render_frame(cx);
            assert!(w.find("copy-paint-1").visible());
        })
        .unwrap();
        let group = crop!(cx.capture_screenshot(handle).unwrap());
        assert_ne!(group, idle, "group hover reveals copy ink");
        assert_ne!(group, full, "group hover reduces copy opacity");
        println!(
            "Metal copy idle/group-hover/button-hover/focus-visible paint passed ({})",
            if dark { "dark" } else { "light" }
        );
    }

    pub(super) fn run() {
        for dark in [false, true] {
            copy_paint(dark);
            let mut nonzero_origin = false;
            for source in [
                "flowchart TD\nA[Native pixels] --> B[Camera crop]",
                "sequenceDiagram\nAlice->>Bob: Native pixels\nBob-->>Alice: Camera crop",
                "stateDiagram-v2\n[*] --> Ready\nReady --> Working\nWorking --> [*]",
            ] {
                let mut cx = HeadlessAppContext::with_platform(
                    platform::current_platform(true).text_system(),
                    Arc::new(Assets),
                    platform::current_headless_renderer,
                );
                cx.update(|cx| init(cx, dark));
                let mut created = None;
                let (handle, _) = cx
                    .update(|cx| {
                        gpui_kit::open_window(
                            WindowOptions {
                                window_bounds: Some(WindowBounds::Windowed(Bounds {
                                    origin: Default::default(),
                                    size: size(px(900.), px(700.)),
                                })),
                                show: false,
                                focus: false,
                                ..Default::default()
                            },
                            cx,
                            |window, cx| {
                                let block = cx.new(|cx| DiagramBlock::new(source.into(), dark, cx));
                                created = Some(block.clone());
                                let host = cx.new(|_| Host { block, dark });
                                cx.new(|cx| gpui_kit::base::Root::new(host, window, cx))
                            },
                        )
                    })
                    .expect("Metal window");
                let block = created.expect("diagram entity");
                cx.update_window(handle, |_, w, _| w.activate_window())
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    cx.run_until_parked();
                    cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                        .unwrap();
                    let settled = block.read_with(&cx, |b, _| {
                        assert!(!b.failed(), "preparation failed");
                        b.ready() && !b.busy()
                    });
                    if settled {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "async preparation/raster did not settle"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                    cx.advance_clock(Duration::from_millis(10));
                }
                cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                    .unwrap();
                let frame = cx.capture_screenshot(handle).expect("Metal inline capture");
                let t = support::tokens(dark);
                let text = [(t.text >> 16) as u8, (t.text >> 8) as u8, t.text as u8];
                let ink = frame
                    .pixels()
                    .filter(|p| (0..3).all(|c| p[c].abs_diff(text[c]) < 20))
                    .count();
                assert!(ink > 100, "diagram missing from actual Metal pixels");
                let idle_frame = frame;
                let (button, scale) = cx
                    .update_window(handle, |_, w, _| {
                        (w.find("diagram-expand").bounds(), w.scale_factor())
                    })
                    .unwrap();
                cx.update_window(handle, |_, w, cx| {
                    assert!(!w.find("diagram-expand-paint").visible());
                    let button = w.find("diagram-expand").bounds();
                    let position = button.center();
                    for event in [
                        gpui_kit::MouseMoveEvent {
                            position,
                            modifiers: Default::default(),
                            pressed_button: None,
                        }
                        .to_platform_input(),
                        gpui_kit::MouseDownEvent {
                            position,
                            button: gpui_kit::MouseButton::Left,
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }
                        .to_platform_input(),
                        gpui_kit::MouseUpEvent {
                            position,
                            button: gpui_kit::MouseButton::Left,
                            modifiers: Default::default(),
                            click_count: 1,
                        }
                        .to_platform_input(),
                    ] {
                        w.dispatch_event(event, cx);
                    }
                    assert!(block.read(cx).is_expanded(), "first native click");
                })
                .unwrap();
                loop {
                    cx.run_until_parked();
                    cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                        .unwrap();
                    if block.read_with(&cx, |b, _| !b.busy()) {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "fullscreen raster did not settle"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                    cx.advance_clock(Duration::from_millis(10));
                }
                cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                    .unwrap();
                let frame = cx
                    .capture_screenshot(handle)
                    .expect("Metal fullscreen capture");
                let ink = frame
                    .enumerate_pixels()
                    .filter(|(x, y, p)| {
                        *x as f64 > frame.width() as f64 * 0.4
                            && (*x as f64) < frame.width() as f64 * 0.6
                            && *y as f64 > frame.height() as f64 * 0.2
                            && (*y as f64) < frame.height() as f64 * 0.8
                            && (0..3).all(|c| p[c].abs_diff(text[c]) < 20)
                    })
                    .count();
                assert!(
                    ink > 100,
                    "fullscreen diagram missing from centered Metal pixels (outside inline bounds)"
                );
                // Genuine input, with captures before the asynchronous worker can settle.
                // Compare text-colored ink to the production SVG raster, never the dot grid.
                let renderer = tau_gpui::components::diagram::DiagramRenderer::new().unwrap();
                let op = tau_gpui::components::diagram::Operation::new(Duration::from_secs(30));
                let diagram = renderer
                    .render(
                        source,
                        tau_gpui::components::diagram::DiagramTheme::ayu(dark),
                        &op,
                    )
                    .unwrap();
                let bounds = diagram.view_box();
                nonzero_origin |= bounds.x != 0. || bounds.y != 0.;
                assert_eq!(
                    block.read_with(&cx, |b, _| b.rendered_svg().unwrap().to_owned()),
                    diagram.svg()
                );
                for pass in 0..2 {
                    for step in 0..12 {
                        let position = cx
                            .update_window(handle, |_, w, _| {
                                let bounds = w.find("diagram-fullscreen").bounds();
                                let fraction = if step % 2 == 0 {
                                    [0.47, 0.45]
                                } else {
                                    [0.53, 0.52]
                                };
                                bounds.origin
                                    + gpui_kit::point(
                                        bounds.size.width * fraction[0],
                                        bounds.size.height * fraction[1],
                                    )
                            })
                            .unwrap();
                        cx.update_window(handle, |_, w, cx| {
                            w.dispatch_event(
                                gpui_kit::MouseMoveEvent {
                                    position,
                                    modifiers: Default::default(),
                                    pressed_button: None,
                                }
                                .to_platform_input(),
                                cx,
                            );
                            if step % 2 == 0 {
                                w.dispatch_event(
                                    gpui_kit::ScrollWheelEvent {
                                        position,
                                        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                            px(0.),
                                            px(if step < 6 { 8. } else { -8. }),
                                        )),
                                        modifiers: gpui_kit::Modifiers {
                                            control: true,
                                            ..Default::default()
                                        },
                                        touch_phase: gpui_kit::TouchPhase::Moved,
                                    }
                                    .to_platform_input(),
                                    cx,
                                );
                            } else {
                                w.dispatch_event(
                                    gpui_kit::PinchEvent {
                                        position,
                                        delta: if step < 6 { 0.08 } else { -0.08 },
                                        phase: gpui_kit::TouchPhase::Moved,
                                        modifiers: Default::default(),
                                    }
                                    .to_platform_input(),
                                    cx,
                                );
                            }
                            w.render_frame(cx);
                        })
                        .unwrap();
                        if pass == 1 {
                            let deadline = Instant::now() + Duration::from_secs(6);
                            loop {
                                cx.run_until_parked();
                                cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                                    .unwrap();
                                if block.read_with(&cx, |b, _| !b.busy()) {
                                    break;
                                }
                                assert!(Instant::now() < deadline, "zoom raster did not settle");
                                std::thread::sleep(Duration::from_millis(10));
                                cx.advance_clock(Duration::from_millis(10));
                            }
                        }
                        let frame = cx.capture_screenshot(handle).unwrap();
                        let [zoom, tx, ty] = block.read_with(&cx, |b, _| b.camera_state().unwrap());
                        let backing = f64::from(scale);
                        let expected = renderer
                            .rasterize(
                                &diagram,
                                tau_gpui::components::diagram::RasterCamera {
                                    viewport: tau_gpui::components::diagram::ViewBox {
                                        x: -tx / zoom,
                                        y: -ty / zoom,
                                        width: 900. / zoom,
                                        height: 700. / zoom,
                                    },
                                    zoom,
                                    backing_scale: backing,
                                },
                                &op,
                            )
                            .unwrap();
                        let mut semantic = 0;
                        let mut retained = 0;
                        for (index, p) in expected.rgba.as_chunks::<4>().0.iter().enumerate() {
                            if p[3] < 240 || !(0..3).all(|c| p[c].abs_diff(text[c]) < 20) {
                                continue;
                            }
                            let x = (index as u32 % expected.width) as i32;
                            let y = (index as u32 / expected.width) as i32;
                            semantic += 1;
                            if x >= 0
                                && y >= 0
                                && x < frame.width() as i32
                                && y < frame.height() as i32
                                && (0..3).all(|c| {
                                    frame.get_pixel(x as u32, y as u32)[c].abs_diff(text[c]) < 35
                                })
                            {
                                retained += 1;
                            }
                        }
                        assert!(semantic > 100, "production SVG mask contains diagram ink");
                        assert!(
                            retained as f64 / semantic as f64 > 0.65,
                            "zoom pass {pass} step {step}: actual Metal retained {retained}/{semantic} semantic ink pixels"
                        );
                    }
                }
                cx.update_window(handle, |_, w, cx| {
                    w.press("escape", cx);
                    assert!(!block.read(cx).is_expanded());
                    assert!(block.read(cx).expand_focused(w));
                    w.render_frame(cx);
                    assert!(w.find("diagram-expand-paint").visible());
                })
                .unwrap();
                let keyboard_frame = cx.capture_screenshot(handle).expect("focus-visible ink");
                macro_rules! crop {
                    ($frame:expr) => {
                        $frame
                            .enumerate_pixels()
                            .filter(|(x, y, _)| {
                                *x as f32 >= f32::from(button.left()) * scale
                                    && (*x as f32) < f32::from(button.right()) * scale
                                    && *y as f32 >= f32::from(button.top()) * scale
                                    && (*y as f32) < f32::from(button.bottom()) * scale
                            })
                            .map(|(_, _, p)| p.0)
                            .collect::<Vec<_>>()
                    };
                }
                let idle = crop!(idle_frame);
                let keyboard = crop!(keyboard_frame);
                assert_ne!(
                    idle, keyboard,
                    "idle ink must differ from keyboard focus pixels"
                );
                for (position, full) in [
                    (button.center(), true),
                    (
                        gpui_kit::point(button.left() - px(20.), button.bottom() + px(20.)),
                        false,
                    ),
                ] {
                    cx.update_window(handle, |_, w, cx| {
                        w.dispatch_event(
                            gpui_kit::MouseMoveEvent {
                                position,
                                modifiers: Default::default(),
                                pressed_button: None,
                            }
                            .to_platform_input(),
                            cx,
                        );
                        w.render_frame(cx);
                        assert!(w.find("diagram-expand-paint").visible());
                    })
                    .unwrap();
                    let hover = crop!(cx.capture_screenshot(handle).expect("hover ink"));
                    if full {
                        assert_eq!(
                            hover, keyboard,
                            "button hover and keyboard focus paint at full opacity"
                        );
                    } else {
                        assert_ne!(hover, idle, "group hover reveals ink");
                        assert_ne!(hover, keyboard, "group hover paints reduced opacity");
                    }
                }
                println!(
                    "Metal repeated wheel/pinch semantic ink, first-click, fullscreen, idle/group-hover/button-hover/focus-visible paint passed ({})",
                    if dark { "dark" } else { "light" }
                );
            }
            assert!(
                nonzero_origin,
                "exercise production SVGs with nonzero origins"
            );
        }
    }
}
