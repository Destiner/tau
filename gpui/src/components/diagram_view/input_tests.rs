//! Real GPUI dispatch through Base Root, not direct calls to event callbacks.
use super::super::{DiagramBlock, worker};
use crate::components::{
    diagram::{DiagramRenderer, DiagramTheme, Operation, RasterCamera},
    support,
};
use gpui_kit::base::{
    Root,
    input::{Input, InputState},
};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Context, Entity, Focusable, IntoElement, MouseButton, Render, TestSupportExt,
    Window, div, point, prelude::*, px, size,
};
use std::time::Duration;

struct Host {
    block: Entity<DiagramBlock>,
    editor: Entity<InputState>,
    downs: usize,
    scrolls: usize,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        support::root(false)
            .flex_col()
            .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, _| s.downs += 1))
            .on_scroll_wheel(cx.listener(|s, _, _, _| s.scrolls += 1))
            .child(
                div()
                    .id("draft")
                    .test_support()
                    .w(px(320.))
                    .h(px(32.))
                    .child(Input::new(&self.editor)),
            )
            .child(div().w(px(320.)).child(self.block.clone()))
    }
}
struct Fixture {
    window: gpui_kit::TestAppWindow<Root>,
    host: Entity<Host>,
    block: Entity<DiagramBlock>,
    editor: Entity<InputState>,
    _app: gpui_kit::TestApp,
}
impl Fixture {
    fn new() -> Self {
        let renderer = DiagramRenderer::new().unwrap();
        let op = Operation::new(Duration::from_secs(5));
        let diagram = renderer
            .render("flowchart TD\nA --> B", DiagramTheme::ayu(false), &op)
            .unwrap();
        let raster = renderer
            .rasterize(
                &diagram,
                RasterCamera {
                    viewport: diagram.view_box(),
                    zoom: 1.,
                    backing_scale: 2.,
                },
                &op,
            )
            .unwrap();
        let mut app = gpui_kit::TestApp::new();
        app.update(|cx| {
            gpui_kit::init(cx);
            crate::components::bind_native_navigation(cx);
        });
        let mut entities = None;
        let mut window = app.open_window(|w, cx| {
            let image = worker::upload(raster, cx.svg_renderer()).unwrap();
            let block = cx.new(|cx| {
                let mut b = DiagramBlock::new("x".repeat(24001), false, cx);
                b.source = std::sync::Arc::from("flowchart TD\nA --> B");
                b.source_valid = true;
                b.diagram = Some(diagram);
                b.image = Some(image.clone());
                b.inline_image = Some(image);
                b.error = None;
                b
            });
            let editor = cx.new(|cx| {
                let mut input = InputState::new(w, cx);
                input.set_value("retained draft", w, cx);
                input
            });
            let host = cx.new(|_| Host {
                block: block.clone(),
                editor: editor.clone(),
                downs: 0,
                scrolls: 0,
            });
            entities = Some((host.clone(), block, editor));
            Root::new(host, w, cx)
        });
        let (host, block, editor) = entities.unwrap();
        window.simulate_resize(size(px(900.), px(700.)));
        window.update(|_, w, cx| w.bounds_changed(cx));
        window.draw();
        Self {
            window,
            host,
            block,
            editor,
            _app: app,
        }
    }
    fn expanded(&mut self) -> bool {
        self.window
            .update(|_, _, cx| self.block.read(cx).is_expanded())
    }
    fn expand(&mut self) {
        self.window.update(|_, w, cx| {
            self.block.update(cx, |s, cx| {
                s.expand_focus.focus(w, cx);
                s.expand(w, cx);
            })
        });
        self.window.draw();
    }
}
#[test]
fn initial_hidden_expand_accepts_first_pointer_dispatch_without_hover_paint() {
    let mut f = Fixture::new();
    let button = f.window.update(|_, w, _| {
        assert!(!w.find("diagram-expand-paint").visible());
        let button = w.find("diagram-expand").bounds();
        assert_eq!(button.size, size(px(21.), px(21.)));
        button
    });
    f.window.simulate_mouse_move(button.center());
    f.window
        .simulate_mouse_down(button.center(), MouseButton::Left);
    f.window
        .simulate_mouse_up(button.center(), MouseButton::Left);
    assert!(f.expanded());
    f.window.draw();
    f.window.simulate_keystroke("escape");
    f.window.draw();
    assert!(!f.expanded());
    f.window.update(|_, w, cx| {
        assert!(f.block.read(cx).expand_focused(w));
        assert!(w.find("diagram-expand-paint").visible());
        assert_eq!(w.find("diagram-expand").bounds(), button);
    });
    f.window.simulate_mouse_move(button.center());
    f.window
        .simulate_mouse_down(button.center(), MouseButton::Left);
    f.window
        .simulate_mouse_up(button.center(), MouseButton::Left);
    assert!(f.expanded(), "second click after Escape");
}

#[test]
fn root_capture_click_reopens_after_drag_zoom_and_escape() {
    let mut f = Fixture::new();
    let handle = f._app.windows()[0];
    f._app.update(|cx| {
        cx.update_window(handle, |_, w, cx| w.click("diagram-expand", cx))
            .unwrap()
    });
    assert!(f.expanded());
    f._app.update(|cx| {
        cx.update_window(handle, |_, w, cx| {
            let from = w.find("diagram-fullscreen").bounds().center();
            let to = w.find("diagram-expand").bounds().center();
            w.drag(from, to, cx);
            w.press("escape", cx);
        })
        .unwrap()
    });
    assert!(!f.expanded());
    f._app.update(|cx| {
        cx.update_window(handle, |_, w, cx| w.click("diagram-expand", cx))
            .unwrap()
    });
    assert!(f.expanded(), "capture click after Escape");
}

#[test]
fn root_tab_and_option_tab_from_real_input_reveal_activate_and_restore_expand() {
    for tab in ["tab", "alt-tab"] {
        for key in ["enter", "space"] {
            let mut f = Fixture::new();
            f.window
                .update(|_, w, _| assert!(!w.find("diagram-expand-paint").visible()));
            let draft = f.window.update(|_, w, _| w.find("draft").bounds());
            f.window.simulate_click(draft.center(), MouseButton::Left);
            f.window.draw();
            f.window
                .update(|_, w, cx| assert!(f.editor.read(cx).focus_handle(cx).is_focused(w)));
            for _ in 0..3 {
                let handle = f._app.windows()[0];
                f._app.update(|cx| {
                    cx.update_window(handle, |_, w, cx| w.press(tab, cx))
                        .unwrap()
                });
                f.window.draw();
                if f.window
                    .update(|_, w, cx| f.block.read(cx).expand_focused(w))
                {
                    break;
                }
            }
            f.window.update(|_, w, cx| {
                assert!(f.block.read(cx).expand_focused(w), "{tab}");
                assert!(w.find("diagram-expand-paint").visible(), "{tab}");
                let ink = w.find("diagram-expand-paint").bounds();
                assert!(w.painted_quads().iter().any(|q| q.bounds
                    == gpui_kit::Bounds {
                        origin: ink.origin.scale(w.scale_factor()),
                        size: ink.size.scale(w.scale_factor())
                    }
                    && q.background
                        == gpui_kit::rgb(crate::components::support::tokens(false).canvas).into()));
            });
            let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
            f.window.simulate_event(gpui_kit::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            f.window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
            assert!(f.expanded(), "{tab} -> {key}");
            f.window.draw();
            f.window.simulate_keystroke("escape");
            f.window.draw();
            f.window
                .update(|_, w, cx| assert!(f.block.read(cx).expand_focused(w)));
            let prev = if tab == "tab" {
                "shift-tab"
            } else {
                "alt-shift-tab"
            };
            let handle = f._app.windows()[0];
            f._app.update(|cx| {
                cx.update_window(handle, |_, w, cx| w.press(prev, cx))
                    .unwrap()
            });
            f.window.draw();
            f.window.update(|_, w, cx| {
                assert!(f.editor.read(cx).focus_handle(cx).is_focused(w));
                assert!(!w.find("diagram-expand-paint").visible());
            });
        }
    }
}

#[test]
fn root_viewer_occludes_input_preserves_draft_and_dismisses_only_clicks() {
    let mut f = Fixture::new();
    let before = f.window.update(|_, w, _| w.find("diagram-block").bounds());
    assert!(before.size.width <= px(320.));
    f.expand();
    let layer = f
        .window
        .update(|_, w, _| w.find("diagram-fullscreen").bounds());
    assert_eq!(layer.origin, point(px(0.), px(0.)));
    assert_eq!(layer.size, size(px(900.), px(700.)));
    f.window.simulate_keystrokes("x enter space backspace");
    f.window
        .simulate_scroll(point(px(150.), px(200.)), point(px(10.), px(-20.)));
    f.window.draw();
    f.window
        .simulate_mouse_down(point(px(100.), px(100.)), MouseButton::Left);
    f.window.simulate_event(gpui_kit::MouseMoveEvent {
        position: point(px(120.), px(140.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    f.window
        .simulate_mouse_up(point(px(120.), px(140.)), MouseButton::Left);
    assert!(f.expanded());
    f.window.update(|_, _, cx| {
        assert_eq!(f.editor.read(cx).value().as_ref(), "retained draft");
        assert_eq!((f.host.read(cx).downs, f.host.read(cx).scrolls), (0, 0));
    });
    f.window
        .simulate_click(point(px(100.), px(100.)), MouseButton::Left);
    assert!(!f.expanded());
    f.window.draw();
    let after = f.window.update(|_, w, _| w.find("diagram-block").bounds());
    assert_eq!(before, after);
    f.window
        .update(|_, w, cx| assert!(f.block.read(cx).expand_focus.is_focused(w)));
}
#[test]
fn keyboard_expand_escape_upper_focus_and_resize() {
    let mut f = Fixture::new();
    for key in ["enter", "space"] {
        f.window.update(|_, w, cx| {
            let focus = f.block.read(cx).expand_focus.clone();
            focus.focus(w, cx);
        });
        f.window.draw();
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        f.window.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        f.window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        f.window.draw();
        assert!(f.expanded(), "{key}");
        f.window.simulate_keystroke("escape");
        f.window.draw();
        assert!(!f.expanded());
    }
    f.expand();
    let camera = f
        .window
        .update(|_, _, cx| f.block.read(cx).viewer.as_ref().unwrap().camera);
    f.window.simulate_resize(size(px(700.), px(500.)));
    f.window.update(|_, w, cx| w.bounds_changed(cx));
    f.window.draw();
    f.window.update(|_, w, cx| {
        let c = f.block.read(cx).viewer.as_ref().unwrap().camera;
        assert_eq!(c.tx, camera.tx - 100.);
        assert_eq!(c.ty, camera.ty - 100.);
        let upper = cx.focus_handle();
        upper.focus(w, cx);
    });
    f.window.simulate_keystroke("escape");
    assert!(f.expanded());
    f.window
        .update(|_, w, cx| f.editor.update(cx, |s, cx| s.focus(w, cx)));
    f.window
        .update(|_, w, cx| f.block.update(cx, |s, cx| s.dismiss(w, cx)));
    f.window.simulate_keystroke("x");
    f.window
        .update(|_, _, cx| assert_ne!(f.editor.read(cx).value().as_ref(), "retained draft"));
}

#[test]
fn wheel_pinch_capture_cancel_and_in_place_theme() {
    use gpui_kit::{PinchEvent, ScrollDelta, ScrollWheelEvent, TouchPhase};
    let mut f = Fixture::new();
    f.expand();
    let initial = f
        .window
        .update(|_, _, cx| f.block.read(cx).viewer.as_ref().unwrap().camera);
    f.window
        .simulate_scroll(point(px(450.), px(350.)), point(px(10.), px(-20.)));
    f.window.draw();
    let panned = f
        .window
        .update(|_, _, cx| f.block.read(cx).viewer.as_ref().unwrap().camera);
    assert_eq!(panned.tx, initial.tx + 10.);
    assert_eq!(panned.ty, initial.ty - 20.);
    let anchored = [
        (450. - panned.tx) / panned.scale,
        (350. - panned.ty) / panned.scale,
    ];
    f.window.simulate_event(ScrollWheelEvent {
        position: point(px(450.), px(350.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-10.))),
        modifiers: gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        },
        touch_phase: TouchPhase::Moved,
    });
    f.window.draw();
    let zoomed = f
        .window
        .update(|_, _, cx| f.block.read(cx).viewer.as_ref().unwrap().camera);
    assert!(zoomed.scale < panned.scale);
    assert!(((450. - zoomed.tx) / zoomed.scale - anchored[0]).abs() < 1e-8);
    assert!(((350. - zoomed.ty) / zoomed.scale - anchored[1]).abs() < 1e-8);
    f.window.simulate_event(PinchEvent {
        position: point(px(450.), px(350.)),
        delta: 0.25,
        phase: TouchPhase::Moved,
        modifiers: Default::default(),
    });
    f.window.draw();
    f.window.update(|_, _, cx| {
        assert!(
            (f.block.read(cx).viewer.as_ref().unwrap().camera.scale - zoomed.scale * 1.25).abs()
                < 1e-8
        )
    });
    f.window
        .simulate_mouse_down(point(px(100.), px(100.)), MouseButton::Left);
    f.window.draw();
    f.window
        .update(|_, w, _| assert!(w.captured_hitbox().is_some()));
    f.window.simulate_event(gpui_kit::MouseMoveEvent {
        position: point(px(130.), px(120.)),
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    f.window.draw();
    f.window.simulate_event(PinchEvent {
        phase: TouchPhase::Cancelled,
        position: point(px(130.), px(120.)),
        ..Default::default()
    });
    f.window.update(|_, w, cx| {
        assert!(w.captured_hitbox().is_none());
        assert!(f.block.read(cx).viewer.as_ref().unwrap().drag.is_none());
        f.block.update(cx, |s, cx| {
            let focus = s.expand_focus.clone();
            s.set_theme(true, cx);
            assert!(s.ready());
            assert!(s.is_expanded());
            assert_eq!(s.expand_focus, focus);
        });
    });
}
