//! Bounded delivered-TreeUpdate tests, not OS/VoiceOver acceptance or text projection.
use gpui_kit::accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId, TreeUpdate};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, MouseButton, Render, TestApp, Window,
    base::{Button, Input, InputBase, input::InputState},
    div,
    prelude::*,
    px,
    test::TestWindowExt,
};
use tau_gpui_prototype::components::{
    self,
    composer::{Composer, ComposerOptions},
    support::{self, ButtonVariant},
};

fn named<'a>(tree: &'a TreeUpdate, name: &str) -> (NodeId, &'a Node) {
    let (id, node) = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(name))
        .unwrap_or_else(|| panic!("missing semantic name {name}"));
    (*id, node)
}
fn click(id: NodeId) -> ActionRequest {
    ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: id,
        data: None,
    }
}
struct Buttons {
    disabled: bool,
    base_calls: usize,
    native_calls: usize,
}
impl Render for Buttons {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().children([
            Button::new("base")
                .accessibility_label("Base save")
                .disabled(self.disabled)
                .on_click(cx.listener(|s, _, _, cx| {
                    s.base_calls += 1;
                    cx.notify();
                }))
                .child("Save")
                .into_any_element(),
            support::button("native", "Native save", ButtonVariant::Primary, false)
                .disabled(self.disabled)
                .on_click(cx.listener(|s, _, _, cx| {
                    s.native_calls += 1;
                    cx.notify();
                }))
                .into_any_element(),
        ])
    }
}
#[test]
fn disabled_accessibility_buttons_track_actual_callbacks() {
    let mut app = TestApp::new();
    app.update(|cx| components::init(cx, false));
    let mut window = app.open_window(|_, _| Buttons {
        disabled: false,
        base_calls: 0,
        native_calls: 0,
    });
    window.activate_accessibility();
    for (disabled, expected_calls) in [(false, 1), (true, 1), (false, 2)] {
        window.update(|s, _, cx| {
            s.disabled = disabled;
            cx.notify();
        });
        window.draw();
        let tree = window.accessibility_tree().expect("delivered update");
        for name in ["Base save", "Native save"] {
            let (id, node) = named(&tree, name);
            assert_eq!(node.role(), Role::Button);
            assert_eq!(node.is_disabled(), disabled, "{name} disabled metadata");
            assert_eq!(node.supports_action(Action::Click), !disabled);
            assert_eq!(node.value(), None);
            window.dispatch_accessibility_action(click(id));
        }
        window.read(|s, _| {
            assert_eq!(s.base_calls, expected_calls);
            assert_eq!(s.native_calls, expected_calls);
        });
    }
    drop(window);
    app.update(|cx| cx.shutdown());
}

struct PrivateInput {
    input: Entity<InputState>,
    disabled: bool,
}
impl Render for PrivateInput {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        InputBase::new("private-wrapper")
            .w(px(300.))
            .h(px(36.))
            .accessibility_label("Private field")
            .disabled(self.disabled)
            .child(Input::new(&self.input))
    }
}
#[test]
fn disabled_input_wrapper_and_masked_readonly_privacy() {
    let mut app = TestApp::new();
    app.update(|cx| components::init(cx, false));
    let mut window = app.open_window(|window, cx| PrivateInput {
        input: cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value("private sentinel café 7391", window, cx);
            input.set_masked(true, window, cx);
            input.set_readonly(true, cx);
            input
        }),
        disabled: false,
    });
    window.activate_accessibility();
    for disabled in [false, true, false] {
        window.update(|s, _, cx| {
            s.disabled = disabled;
            s.input
                .update(cx, |state, cx| state.set_disabled(disabled, cx));
            cx.notify();
        });
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        let (_, node) = named(&tree, "Private field");
        assert_eq!(node.role(), Role::TextInput);
        assert_eq!(node.is_disabled(), disabled);
        // The wrapper owns no read-only state: do not claim it by inference.
        assert!(!node.is_read_only());
        assert!(!node.supports_action(Action::Click));
        assert_eq!(node.value(), None);
        for (_, node) in &tree.nodes {
            assert_eq!(node.value(), None, "no text-value projection in this slice");
            assert!(
                !node
                    .label()
                    .unwrap_or_default()
                    .contains("private sentinel")
            );
        }
        let at = window.update(|_, window, _| window.find("private-wrapper").bounds().center());
        window.simulate_click(at, MouseButton::Left);
        window.simulate_input("X");
        window.read(|s, cx| {
            assert!(!s.input.read(cx).is_editable());
            assert_eq!(
                s.input.read(cx).value().as_ref(),
                "private sentinel café 7391"
            );
        });
    }
    drop(window);
    app.update(|cx| cx.shutdown());
}

#[test]
fn composer_wrapper_disabled_matches_actual_editor_status() {
    let mut app = TestApp::new();
    app.update(|cx| components::init(cx, false));
    let mut window = app.open_window(|window, cx| {
        Composer::new(
            ComposerOptions {
                models: vec![],
                efforts: vec![],
                model_heading: "Models".into(),
                model: "".into(),
                effort: "".into(),
                draft: "private composer sentinel".into(),
                dark: false,
                disabled: false,
                streaming: false,
            },
            window,
            cx,
        )
    });
    window.activate_accessibility();
    // Streaming disables settings, not editing; never infer from appearance.
    for (disabled, streaming) in [(false, false), (true, false), (false, true), (false, false)] {
        window.update(|s, _, cx| s.set_status(disabled, streaming, cx));
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        let (_, node) = named(&tree, "Message");
        assert_eq!(node.role(), Role::TextInput);
        assert_eq!(node.is_disabled(), disabled);
        assert!(!node.is_read_only());
        assert!(!node.supports_action(Action::Click));
        window.read(|s, cx| {
            assert_eq!(s.is_disabled(), disabled);
            assert_eq!(s.editor().read(cx).is_editable(), !disabled);
            assert_eq!(
                s.editor().read(cx).value().as_ref(),
                "private composer sentinel"
            );
        });
    }
    drop(window);
    app.update(|cx| cx.shutdown());
}

struct Metadata;
impl Render for Metadata {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children([
            div()
                .id("default")
                .role(Role::Button)
                .aria_label("Default")
                .into_any_element(),
            div()
                .id("override")
                .role(Role::Button)
                .aria_label("Override")
                .aria_disabled(true)
                .aria_disabled(false)
                .into_any_element(),
            InputBase::new("styled")
                .accessibility_label("Style only")
                .styles(|s| s.disabled(|s| s.opacity(0.4)))
                .into_any_element(),
        ])
    }
}
#[test]
fn metadata_defaults_and_disabled_style_do_not_invent_state() {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, _| Metadata);
    window.activate_accessibility();
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    for name in ["Default", "Override", "Style only"] {
        assert!(!named(&tree, name).1.is_disabled());
    }
    drop(window);
    app.update(|cx| cx.shutdown());
}
