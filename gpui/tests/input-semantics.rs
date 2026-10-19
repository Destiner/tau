//! Delivered Core TestPlatform trees/actions only; no OS acceptance or text projection.
use core::prelude::v1::test;
use gpui_kit::{
    accesskit::{Action, ActionRequest, NodeId, Role, TreeId, TreeUpdate},
    base::{
        self, Button, Dialog, DialogHandle, FocusTrapElement, Input, InputBase, Root, Textarea,
        input::{InputState, TextareaState},
    },
    prelude::*,
    test::TestWindowExt,
    *,
};

fn named<'a>(tree: &'a TreeUpdate, name: &str) -> (NodeId, &'a accesskit::Node) {
    let mut nodes = tree.nodes.iter().filter(|(_, n)| n.label() == Some(name));
    let (id, node) = nodes.next().unwrap_or_else(|| panic!("missing {name}"));
    assert!(nodes.next().is_none(), "duplicate name {name}");
    (*id, node)
}
fn focus(id: NodeId) -> ActionRequest {
    ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: id,
        data: None,
    }
}
struct Host {
    trap: FocusHandle,
    button: FocusHandle,
    input: Entity<InputState>,
    multi: Entity<TextareaState>,
    generic: Entity<InputState>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("dialog")
            .role(Role::Dialog)
            .aria_label("Ordinary dialog")
            .track_focus(&self.trap)
            .flex()
            .flex_col()
            .w(px(300.))
            .child(
                Button::new("origin")
                    .accessibility_label("Origin")
                    .track_focus(&self.button)
                    .h(px(40.))
                    .child("Origin"),
            )
            .child(
                InputBase::new("single-shell")
                    .role(None)
                    .h(px(40.))
                    .child(Input::new(&self.input)),
            )
            .child(
                InputBase::new("multi-shell")
                    .role(None)
                    .h(px(80.))
                    .child(Textarea::new(&self.multi)),
            )
            .child(
                InputBase::new("generic-shell")
                    .accessibility_label("Legacy wrapper")
                    .h(px(40.))
                    .child(Input::new(&self.generic)),
            )
            .focus_trap("ordinary-dialog", &self.trap)
    }
}
#[test]
fn rendered_editor_semantics_and_real_accessibility_focus() {
    let mut app = TestApp::new();
    app.update(|cx| tau_gpui::components::init(cx, false));
    let mut window = app.open_window(|w, cx| Host {
        trap: cx.focus_handle(),
        button: cx.focus_handle(),
        input: cx.new(|cx| {
            InputState::new(w, cx)
                .accessibility_label("Field")
                .default_value("private sentinel")
        }),
        multi: cx.new(|cx| {
            TextareaState::new(w, cx)
                .accessibility_label("Draft")
                .default_value("private draft")
                .rows(2)
        }),
        generic: cx.new(|cx| InputState::new(w, cx)),
    });
    window.activate_accessibility();
    for (disabled, readonly, masked) in [
        (false, false, false),
        (false, true, false),
        (true, true, false),
        (true, false, false),
        (false, true, true),
        (false, false, false),
    ] {
        window.update(|s, _, cx| {
            s.input.update(cx, |s, cx| {
                s.set_disabled(disabled, cx);
                s.set_readonly(readonly, cx);
            })
        });
        window.update(|s, w, cx| s.input.update(cx, |s, cx| s.set_masked(masked, w, cx)));
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        let (id, n) = named(&tree, "Field");
        println!(
            "Field node={id:?} role={:?} disabled={} readonly={} Focus={} delivered_focus={:?}",
            n.role(),
            n.is_disabled(),
            n.is_read_only(),
            n.supports_action(Action::Focus),
            tree.focus
        );
        assert_eq!(
            n.role(),
            if masked {
                Role::PasswordInput
            } else {
                Role::TextInput
            }
        );
        assert_eq!(n.is_disabled(), disabled);
        assert_eq!(n.is_read_only(), readonly);
        assert_eq!(n.supports_action(Action::Focus), !disabled);
        assert_eq!(named(&tree, "Draft").1.role(), Role::MultilineTextInput);
        assert_eq!(named(&tree, "Legacy wrapper").1.role(), Role::TextInput);
        assert!(
            !named(&tree, "Legacy wrapper")
                .1
                .supports_action(Action::Focus)
        );
        assert!(!named(&tree, "Ordinary dialog").1.is_modal());
        assert_eq!(
            tree.nodes
                .iter()
                .filter(|(_, n)| matches!(
                    n.role(),
                    Role::TextInput | Role::PasswordInput | Role::MultilineTextInput
                ))
                .count(),
            3,
            "no wrapper/editor duplicates"
        );
        if masked {
            assert_eq!(n.value(), None);
        }
        for (_, n) in &tree.nodes {
            if masked {
                assert!(!n.value().unwrap_or_default().contains("private sentinel"));
            }
            assert!(!n.label().unwrap_or_default().contains("private"));
        }
        let at = window.update(|_, w, _| w.find("origin").bounds().center());
        window.simulate_click(at, MouseButton::Left);
        window.update(|s, w, cx| {
            assert!(s.button.is_focused(w));
            assert!(!s.input.read(cx).focus_handle(cx).is_focused(w));
            assert!(
                base::active_focus_trap(w, cx).is_some(),
                "pointer establishes active trap"
            );
        });
        window.dispatch_accessibility_action(focus(id));
        window.update(|s, w, cx| {
            assert_eq!(s.input.read(cx).focus_handle(cx).is_focused(w), !disabled)
        });
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        if !disabled {
            assert_eq!(tree.focus, named(&tree, "Field").0);
        }
        if !disabled {
            window.simulate_input("X");
            window.read(|s, cx| {
                let value = s.input.read(cx).value();
                if readonly {
                    assert_eq!(value.as_ref(), "private sentinel");
                } else {
                    assert_ne!(value.as_ref(), "private sentinel");
                    assert!(value.contains('X'));
                }
            });
            if readonly {
                window.simulate_keystroke("right");
                window.read(|s, cx| assert!(s.input.read(cx).selected_range().start > 0));
            }
            window.simulate_keystroke("backspace");
            window.read(|s, cx| assert_eq!(s.input.read(cx).value().as_ref(), "private sentinel"));
        }
        let draft = named(&tree, "Draft").0;
        window.dispatch_accessibility_action(focus(draft));
        window.update(|s, w, cx| assert!(s.multi.read(cx).focus_handle(cx).is_focused(w)));
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        assert_eq!(tree.focus, named(&tree, "Draft").0);
    }
    // Reuse an ID captured while enabled after disabling (TestApp auto-redraws).
    window.draw();
    let id = named(&window.accessibility_tree().unwrap(), "Field").0;
    let at = window.update(|_, w, _| w.find("origin").bounds().center());
    window.simulate_click(at, MouseButton::Left);
    window.update(|s, _, cx| s.input.update(cx, |s, cx| s.set_disabled(true, cx)));
    // TestApp drains redraw tasks after each update; the old ID still must be inert.
    window.dispatch_accessibility_action(focus(id));
    window.update(|s, w, _| {
        assert!(
            s.button.is_focused(w),
            "stale enabled action must not focus now-disabled editor"
        )
    });
    drop(window);
    app.update(|cx| cx.shutdown());
}

#[test]
fn legacy_wrapper_action_does_not_reach_editor_negative_control() {
    let mut app = TestApp::new();
    app.update(|cx| tau_gpui::components::init(cx, false));
    let mut window = app.open_window(|w, cx| Host {
        trap: cx.focus_handle(),
        button: cx.focus_handle(),
        input: cx.new(|cx| InputState::new(w, cx).accessibility_label("Field")),
        multi: cx.new(|cx| TextareaState::new(w, cx).accessibility_label("Draft")),
        generic: cx.new(|cx| InputState::new(w, cx)),
    });
    window.activate_accessibility();
    window.draw();
    let id = named(&window.accessibility_tree().unwrap(), "Legacy wrapper").0;
    let at = window.update(|_, w, _| w.find("origin").bounds().center());
    window.simulate_click(at, MouseButton::Left);
    window.dispatch_accessibility_action(focus(id));
    window.update(|s, w, cx| {
        assert!(s.button.is_focused(w));
        assert!(!s.generic.read(cx).focus_handle(cx).is_focused(w));
    });
    drop(window);
    app.update(|cx| cx.shutdown());
}

struct ModalHost {
    dialog: DialogHandle,
    trap: FocusHandle,
    background: Entity<TextareaState>,
    inner: Entity<InputState>,
}
impl Render for ModalHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                InputBase::new("background-shell")
                    .role(None)
                    .w(px(300.))
                    .h(px(80.))
                    .child(Textarea::new(&self.background)),
            )
            .child(
                Dialog::new(cx)
                    .handle(self.dialog.clone())
                    .focus_handle(self.trap.clone())
                    .popup(
                        InputBase::new("inner-shell")
                            .role(None)
                            .w(px(300.))
                            .h(px(40.))
                            .child(Input::new(&self.inner)),
                    ),
            )
    }
}

#[test]
fn editor_ax_focus_preserves_active_dialog_trap_until_escape() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut host = None;
    let mut window = app.open_window(|w, cx| {
        let view = cx.new(|cx| ModalHost {
            dialog: DialogHandle::new(true),
            trap: cx.focus_handle(),
            background: cx
                .new(|cx| TextareaState::new(w, cx).accessibility_label("Background draft")),
            inner: cx.new(|cx| InputState::new(w, cx).accessibility_label("Dialog field")),
        });
        host = Some(view.clone());
        Root::new(view, w, cx)
    });
    let host = host.unwrap();
    window.activate_accessibility();
    window.draw();
    let at = window.update(|_, w, _| w.find("inner-shell").bounds().center());
    window.simulate_click(at, MouseButton::Left);
    window.update(|_, w, cx| {
        let s = host.read(cx);
        assert!(
            s.inner.read(cx).focus_handle(cx).is_focused(w),
            "genuine click focuses dialog editor"
        );
        let trap = base::active_focus_trap(w, cx).expect("click establishes active dialog trap");
        assert_eq!(trap, s.trap);
        assert!(trap.contains(&trap, w), "contains includes handle equality");
        assert!(trap.contains(&s.inner.read(cx).focus_handle(cx), w));
        assert!(!trap.contains(&s.background.read(cx).focus_handle(cx), w));
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let background = named(&tree, "Background draft").0;
    let inner = named(&tree, "Dialog field").0;
    assert_eq!(tree.focus, inner);
    assert!(
        named(&tree, "Background draft")
            .1
            .supports_action(Action::Focus)
    );
    window.dispatch_accessibility_action(focus(background));
    window.update(|_, w, cx| {
        let s = host.read(cx);
        assert!(
            s.inner.read(cx).focus_handle(cx).is_focused(w),
            "background AXFocus must retain dialog editor focus"
        );
        assert_eq!(
            base::active_focus_trap(w, cx),
            Some(s.trap.clone()),
            "background AXFocus must retain active trap"
        );
        assert!(!s.background.read(cx).focus_handle(cx).is_focused(w));
    });
    window.draw();
    assert_eq!(window.accessibility_tree().unwrap().focus, inner);
    window.simulate_input("owned");
    window.read(|_, cx| {
        let s = host.read(cx);
        assert_eq!(s.inner.read(cx).value().as_ref(), "owned");
        assert_eq!(s.background.read(cx).value().as_ref(), "");
    });
    // Move within the trap through its delivered action, not forced focus.
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let trap = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::Dialog)
        .unwrap()
        .0;
    window.dispatch_accessibility_action(focus(trap));
    window.update(|_, w, cx| assert!(host.read(cx).trap.is_focused(w)));
    window.dispatch_accessibility_action(focus(
        named(&window.accessibility_tree().unwrap(), "Dialog field").0,
    ));
    window.update(|_, w, cx| assert!(host.read(cx).inner.read(cx).focus_handle(cx).is_focused(w)));
    window.simulate_input("!");
    window.read(|_, cx| assert_eq!(host.read(cx).inner.read(cx).value().as_ref(), "owned!"));
    window.simulate_keystroke("escape");
    window.update(|_, w, cx| {
        assert!(
            !host.read(cx).dialog.is_open(),
            "genuine Escape dismisses Base Dialog"
        );
        assert!(base::active_focus_trap(w, cx).is_none());
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert!(!tree.nodes.iter().any(|(_, n)| n.role() == Role::Dialog));
    window.dispatch_accessibility_action(focus(named(&tree, "Background draft").0));
    window.update(|_, w, cx| {
        assert!(
            host.read(cx)
                .background
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    window.draw();
    assert_eq!(
        window.accessibility_tree().unwrap().focus,
        named(&window.accessibility_tree().unwrap(), "Background draft").0
    );
    window.simulate_input("draft");
    window.read(|_, cx| {
        let s = host.read(cx);
        assert_eq!(s.background.read(cx).value().as_ref(), "draft");
        assert_eq!(s.inner.read(cx).value().as_ref(), "owned!");
    });
    println!(
        "Root/Base Dialog: background AXFocus rejected; trap/inner/delivered focus retained; typing owned; inner AXFocus works; Escape allows background AXFocus and typing"
    );
    drop(window);
    app.update(|cx| cx.shutdown());
}

#[test]
fn active_focus_trap_without_base_init_is_none() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, _| Empty);
    window.update(|_, w, cx| assert!(base::active_focus_trap(w, cx).is_none()));
    drop(window);
    app.update(|cx| cx.shutdown());
}
