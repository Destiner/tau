//! Direct Base regressions through TestPlatform, not a migration/showcase host.
use gpui_kit::{
    Context, Entity, FocusHandle, Focusable, IntoElement, MouseButton, Render, TestApp,
    TestSupportExt, Window, accesskit,
    base::{
        Button, Dialog, DialogClose, DialogHandle, DialogPopup, FocusTrapElement, InputBase, Root,
        input::InputState,
    },
    div,
    prelude::*,
    px,
    test::TestWindowExt,
};

fn field(id: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .w(px(180.))
        .h(px(32.))
        .child(InputBase::new("field").role(None).child(input.clone()))
}

struct Nested {
    outer: FocusHandle,
    middle: FocusHandle,
    inner: FocusHandle,
    background: Entity<InputState>,
    middle_input: Entity<InputState>,
    inner_input: Entity<InputState>,
    show_inner: bool,
}
impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let middle = div()
            .flex()
            .flex_col()
            .focus_trap("middle", &self.middle)
            .child(field("middle-input", &self.middle_input))
            .when(self.show_inner, |this| {
                this.child(
                    div()
                        .focus_trap("inner", &self.inner)
                        .child(field("inner-input", &self.inner_input)),
                )
            });
        div()
            .flex()
            .flex_col()
            .child(field("background", &self.background))
            .child(div().focus_trap("outer", &self.outer).child(middle))
    }
}

fn ax_focus(window: &mut gpui_kit::TestAppWindow<Root>, label: &str) {
    let tree = window.accessibility_tree().unwrap();
    let target = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .unwrap()
        .0;
    window.dispatch_accessibility_action(accesskit::ActionRequest {
        action: accesskit::Action::Focus,
        target_tree: accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    });
    window.draw();
}

#[test]
fn innermost_trap_contains_keyboard_and_editor_ax_focus() {
    // Recreate the global manager to exercise independently seeded HashMaps.
    for _ in 0..16 {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut host = None;
        let mut window = app.open_window(|window, cx| {
            let entity = cx.new(|cx| {
                let background =
                    cx.new(|cx| InputState::new(window, cx).accessibility_label("Background"));
                let middle_input =
                    cx.new(|cx| InputState::new(window, cx).accessibility_label("Middle"));
                let inner_input =
                    cx.new(|cx| InputState::new(window, cx).accessibility_label("Inner"));
                Nested {
                    outer: cx.focus_handle(),
                    middle: cx.focus_handle(),
                    inner: cx.focus_handle(),
                    background,
                    middle_input,
                    inner_input,
                    show_inner: true,
                }
            });
            host = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let host = host.unwrap();
        window.activate_accessibility();
        window.draw();
        let position = window.update(|_, w, _| w.find("inner-input").bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
        window.update(|_, w, cx| {
            let h = host.read(cx);
            assert!(h.inner_input.read(cx).focus_handle(cx).is_focused(w));
            assert_eq!(
                gpui_kit::base::active_focus_trap(w, cx),
                Some(h.inner.clone())
            );
        });
        for key in ["tab", "shift-tab", "tab"] {
            window.simulate_keystroke(key);
            window.draw();
            window.update(|_, w, cx| assert!(host.read(cx).inner.contains_focused(w, cx)));
        }
        for label in ["Middle", "Background", "Inner"] {
            ax_focus(&mut window, label);
            window.update(|_, w, cx| assert!(host.read(cx).inner.contains_focused(w, cx)));
        }
        // Retained but unmounted inner handles must not constrain the next layer.
        window.update(|_, _, cx| {
            host.update(cx, |h, cx| {
                h.show_inner = false;
                cx.notify();
            })
        });
        window.draw();
        let position = window.update(|_, w, _| w.find("middle-input").bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
        window.update(|_, w, cx| {
            assert_eq!(
                gpui_kit::base::active_focus_trap(w, cx),
                Some(host.read(cx).middle.clone())
            )
        });
        ax_focus(&mut window, "Background");
        window.update(|_, w, cx| {
            assert!(
                host.read(cx)
                    .middle_input
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            )
        });
    }
}

struct LargeTree {
    trap: FocusHandle,
}
impl Render for LargeTree {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .focus_trap("single", &self.trap)
                    .child(Button::new("inside").child("Inside").size(px(20.))),
            )
            .children(
                (0..130usize).map(|i| Button::new(("outside", i)).child("Outside").size(px(2.))),
            )
    }
}
#[test]
fn trap_identity_is_per_window_even_with_identical_element_paths() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut handles = Vec::new();
    let mut windows = Vec::new();
    for _ in 0..2 {
        windows.push(app.open_window(|window, cx| {
            let entity = cx.new(|cx| LargeTree {
                trap: cx.focus_handle(),
            });
            handles.push(entity.read(cx).trap.clone());
            Root::new(entity, window, cx)
        }));
    }
    for window in &mut windows {
        window.draw();
        let position = window.update(|_, w, _| w.find("inside").bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
    }
    // The second draw must not overwrite the first window's trap registration.
    for (window, handle) in windows.iter_mut().zip(handles) {
        window.update(|_, w, cx| {
            assert!(handle.contains_focused(w, cx));
            assert_eq!(
                gpui_kit::base::active_focus_trap(w, cx),
                Some(handle.clone())
            );
        });
    }
}

#[test]
fn bounded_tab_search_never_leaves_the_trap() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut host = None;
    let mut window = app.open_window(|window, cx| {
        let entity = cx.new(|cx| LargeTree {
            trap: cx.focus_handle(),
        });
        host = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let host = host.unwrap();
    window.draw();
    let position = window.update(|_, w, _| w.find("inside").bounds().center());
    window.simulate_click(position, MouseButton::Left);
    window.draw();
    window.update(|_, w, cx| assert!(host.read(cx).trap.contains_focused(w, cx)));
    for key in ["tab", "shift-tab", "tab", "shift-tab"] {
        window.simulate_keystroke(key);
        window.draw();
        window.update(|_, w, cx| {
            assert!(
                host.read(cx).trap.contains_focused(w, cx),
                "escaped after {key}"
            )
        });
    }
}

struct Layers {
    outer: DialogHandle,
    inner: DialogHandle,
    keyboard: bool,
    lower_focus: FocusHandle,
}
impl Render for Layers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lower_focus = self.lower_focus.clone();
        Dialog::new(cx)
            .handle(self.outer.clone())
            .layer(0, !self.inner.is_open())
            .popup(
                DialogPopup::new()
                    .child(
                        Button::new("lower-close")
                            .accessibility_label("Lower Close")
                            .track_focus(&self.lower_focus)
                            .size(px(20.))
                            .on_click(move |_, window, cx| {
                                lower_focus.dispatch_action(
                                    &gpui_kit::base::actions::Cancel,
                                    window,
                                    cx,
                                )
                            }),
                    )
                    .child(
                        Dialog::new(cx)
                            .handle(self.inner.clone())
                            .layer(1, true)
                            .close_on_escape(self.keyboard)
                            .popup(
                                DialogPopup::new().size(px(100.)).child(
                                    DialogClose::new()
                                        .trigger(|b| b.size(px(80.)).child("Close inner")),
                                ),
                            ),
                    ),
            )
    }
}
#[test]
fn nested_escape_respects_top_layer_and_explicit_close() {
    for keyboard in [true, false] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let outer = DialogHandle::new(true);
        let inner = DialogHandle::new(true);
        let mut window = app.open_window(|window, cx| {
            let host = cx.new(|cx| Layers {
                outer: outer.clone(),
                inner: inner.clone(),
                keyboard,
                lower_focus: cx.focus_handle(),
            });
            Root::new(host, window, cx)
        });
        window.draw();
        // AX Focus uses the actual button handler, without activating Close.
        window.activate_accessibility();
        window.draw();
        ax_focus(&mut window, "Lower Close");
        window.update(|_, w, cx| w.dispatch_action(Box::new(gpui_kit::base::actions::Cancel), cx));
        window.draw();
        assert!(outer.is_open(), "a lower-layer Close action is inert");
        assert!(inner.is_open());
        ax_focus(&mut window, "Close");
        window.simulate_keystroke("escape");
        window.draw();
        assert!(outer.is_open(), "Escape cannot dismiss the ancestor");
        assert_eq!(inner.is_open(), !keyboard);
        if !keyboard {
            let tree = window.accessibility_tree().unwrap();
            let target = tree
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some("Close"))
                .unwrap()
                .0;
            window.dispatch_accessibility_action(accesskit::ActionRequest {
                action: accesskit::Action::Click,
                target_tree: accesskit::TreeId::ROOT,
                target_node: target,
                data: None,
            });
            window.draw();
            assert!(
                !inner.is_open(),
                "close_on_escape(false) does not disable explicit Close"
            );
            assert!(outer.is_open());
        }
    }
}
