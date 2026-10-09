//! Delivered TestPlatform trees/actions and real EntityInputHandler/history; no OS/clipboard.
use core::prelude::v1::test;
use gpui_kit::{
    accesskit::{Action, ActionData, ActionRequest, NodeId, Role, TreeId, TreeUpdate},
    base::{
        self, Button, Editor, FocusTrapElement, Input, InputBase, Textarea,
        input::{
            AutoClosingPair, EditorState, InputState, TextareaState,
            language_config::LanguageConfig,
        },
    },
    prelude::*,
    test::TestWindowExt,
    *,
};
const LIMIT: usize = 64 * 1024;
fn named<'a>(tree: &'a TreeUpdate, name: &str) -> (NodeId, &'a accesskit::Node) {
    let mut nodes = tree.nodes.iter().filter(|(_, n)| n.label() == Some(name));
    let (id, node) = nodes.next().unwrap_or_else(|| panic!("missing {name}"));
    assert!(nodes.next().is_none());
    (*id, node)
}
fn request(id: NodeId, action: Action, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: id,
        data,
    }
}
fn replace(id: NodeId, text: &str) -> ActionRequest {
    request(id, Action::SetValue, Some(ActionData::Value(text.into())))
}
struct Host {
    field: Entity<InputState>,
    draft: Entity<TextareaState>,
    generic: Entity<InputState>,
    code: Entity<EditorState>,
    trap: FocusHandle,
    origin: FocusHandle,
    trapped: bool,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let inside = div()
            .id("inside")
            .track_focus(&self.trap)
            .child(
                Button::new("origin")
                    .track_focus(&self.origin)
                    .child("Origin"),
            )
            .child(
                InputBase::new("field-shell")
                    .role(None)
                    .w(px(300.))
                    .h(px(40.))
                    .child(Input::new(&self.field)),
            );
        let inside = if self.trapped {
            inside
                .focus_trap("test-trap", &self.trap)
                .into_any_element()
        } else {
            inside.into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(inside)
            .child(
                InputBase::new("code-shell")
                    .role(None)
                    .w(px(300.))
                    .h(px(80.))
                    .child(Editor::new(&self.code)),
            )
            .child(
                InputBase::new("draft-shell")
                    .role(None)
                    .w(px(300.))
                    .h(px(80.))
                    .child(Textarea::new(&self.draft)),
            )
            .child(
                InputBase::new("generic-shell")
                    .accessibility_label("Legacy")
                    .w(px(300.))
                    .h(px(40.))
                    .child(Input::new(&self.generic)),
            )
    }
}
fn host(w: &mut Window, cx: &mut Context<Host>) -> Host {
    Host {
        field: cx.new(|cx| {
            InputState::new(w, cx)
                .accessibility_label("Field")
                .default_value("old😀e\u{301}")
        }),
        draft: cx.new(|cx| {
            TextareaState::new(w, cx)
                .accessibility_label("Draft")
                .default_value("first\n第二行😀")
        }),
        generic: cx.new(|cx| InputState::new(w, cx).default_value("generic sentinel")),
        code: cx.new(|cx| {
            base::input::set_language_config(
                "ax-pairing",
                LanguageConfig::default().auto_closing_pairs([AutoClosingPair::new("(", ")")]),
                cx,
            );
            EditorState::new(w, cx)
                .language("ax-pairing")
                .accessibility_label("Code")
        }),
        trap: cx.focus_handle(),
        origin: cx.focus_handle(),
        trapped: false,
    }
}
fn no_secret(tree: &TreeUpdate, secret: &str) {
    for (_, n) in &tree.nodes {
        assert!(!n.value().unwrap_or_default().contains(secret));
        assert!(!n.label().unwrap_or_default().contains(secret));
        assert!(!n.description().unwrap_or_default().contains(secret));
    }
}
#[test]
fn named_empty_code_editor_withholds_set_value_without_disabling_pairing() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut window = app.open_window(host);
    window.activate_accessibility();
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let (id, node) = named(&tree, "Code");
    assert_eq!(node.value(), Some(""));
    assert_eq!(node.role(), Role::MultilineTextInput);
    assert!(named(&tree, "Field").1.supports_action(Action::SetValue));
    assert!(named(&tree, "Draft").1.supports_action(Action::SetValue));
    // Deliver anyway: an unsupported whole-value action must leave the empty editor intact.
    window.dispatch_accessibility_action(replace(id, "("));
    window.read(|s, cx| {
        assert_eq!(s.code.read(cx).value().as_ref(), "");
        assert_eq!(s.code.read(cx).selected_range(), 0..0);
    });
    assert!(!node.supports_action(Action::SetValue));
    window.dispatch_accessibility_action(request(id, Action::Focus, None));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.code.read(cx).value().as_ref(), ""));
    // Positive control: the actual language still pairs interactive input.
    window.simulate_input("(");
    window.read(|s, cx| assert_eq!(s.code.read(cx).value().as_ref(), "()"));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.code.read(cx).value().as_ref(), ""));
    drop(window);
    app.update(|cx| cx.shutdown());
}
#[test]
fn delivered_complete_unicode_values_set_value_real_handler_atomic_history() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut window = app.open_window(host);
    window.activate_accessibility();
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let id = named(&tree, "Field").0;
    assert_eq!(named(&tree, "Field").1.value(), Some("old😀e\u{301}"));
    assert_eq!(named(&tree, "Draft").1.value(), Some("first\n第二行😀"));
    assert!(named(&tree, "Field").1.supports_action(Action::SetValue));
    assert_eq!(named(&tree, "Draft").1.role(), Role::MultilineTextInput);
    window.dispatch_accessibility_action(request(id, Action::Focus, None));
    // Wrong payload cannot mutate or pollute history.
    window.dispatch_accessibility_action(request(id, Action::SetValue, None));
    window.dispatch_accessibility_action(request(
        id,
        Action::SetValue,
        Some(ActionData::NumericValue(5.)),
    ));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "old😀e\u{301}"));
    window.update(|s, _, cx| s.field.update(cx, |s, cx| s.set_selected_range(3..7, cx)));
    window.dispatch_accessibility_action(replace(id, "new🦀e\u{301}漢"));
    window.read(|s, cx| {
        assert_eq!(s.field.read(cx).value().as_ref(), "new🦀e\u{301}漢");
        assert_eq!(
            s.field.read(cx).selected_range(),
            "new🦀e\u{301}漢".len().."new🦀e\u{301}漢".len()
        );
        assert_eq!(s.draft.read(cx).value().as_ref(), "first\n第二行😀");
    });
    window.draw();
    assert_eq!(
        named(&window.accessibility_tree().unwrap(), "Field")
            .1
            .value(),
        Some("new🦀e\u{301}漢")
    );
    window.simulate_input("!");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "new🦀e\u{301}漢!"));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "new🦀e\u{301}漢"));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| {
        assert_eq!(s.field.read(cx).value().as_ref(), "old😀e\u{301}");
        assert_eq!(
            s.field.read(cx).selected_range(),
            3..7,
            "undo restores the editor's original Unicode selection"
        );
    });
    window.simulate_keystroke("cmd-shift-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "new🦀e\u{301}漢"));
    // A second editor has independent history; replacing all text must not use cursor offsets.
    window.draw();
    let draft = named(&window.accessibility_tree().unwrap(), "Draft").0;
    window.dispatch_accessibility_action(request(draft, Action::Focus, None));
    window.dispatch_accessibility_action(replace(draft, "α😀\n三\ne\u{301}"));
    window.read(|s, cx| assert_eq!(s.draft.read(cx).value().as_ref(), "α😀\n三\ne\u{301}"));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| {
        assert_eq!(s.draft.read(cx).value().as_ref(), "first\n第二行😀");
        assert_eq!(s.field.read(cx).value().as_ref(), "new🦀e\u{301}漢");
    });
    window.simulate_keystroke("cmd-shift-z");
    window.draw();
    assert_eq!(
        named(&window.accessibility_tree().unwrap(), "Draft")
            .1
            .value(),
        Some("α😀\n三\ne\u{301}")
    );
    drop(window);
    app.update(|cx| cx.shutdown());
}
#[test]
fn delivered_status_password_generic_and_size_guards_are_honest() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut window = app.open_window(host);
    window.activate_accessibility();
    window.draw();
    let id = named(&window.accessibility_tree().unwrap(), "Field").0;
    for (disabled, readonly, masked) in [
        (false, true, false),
        (true, false, false),
        (false, false, true),
    ] {
        window.update(|s, w, cx| {
            s.field.update(cx, |s, cx| {
                s.set_disabled(disabled, cx);
                s.set_readonly(readonly, cx);
                s.set_masked(masked, w, cx);
            })
        });
        window.draw();
        let tree = window.accessibility_tree().unwrap();
        let n = named(&tree, "Field").1;
        assert_eq!(n.is_disabled(), disabled);
        assert_eq!(n.is_read_only(), readonly);
        assert!(!n.supports_action(Action::SetValue));
        assert_eq!(
            n.value(),
            if masked {
                None
            } else {
                Some("old😀e\u{301}")
            }
        );
        if masked {
            no_secret(&tree, "old😀e\u{301}");
        }
        window.dispatch_accessibility_action(replace(id, "rejected"));
        window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "old😀e\u{301}"));
    }
    window.update(|s, w, cx| {
        s.field.update(cx, |s, cx| {
            s.set_masked(false, w, cx);
            s.set_value("x".repeat(LIMIT), w, cx);
        })
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert_eq!(named(&tree, "Field").1.value().unwrap().len(), LIMIT);
    assert!(named(&tree, "Field").1.supports_action(Action::SetValue));
    // Incoming oversized values also fail closed, preserving the previous full value.
    window.dispatch_accessibility_action(replace(id, &"y".repeat(LIMIT + 1)));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().len(), LIMIT));
    window.update(|s, w, cx| {
        s.field
            .update(cx, |s, cx| s.set_value("😀".repeat(LIMIT / 4 + 1), w, cx))
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert_eq!(named(&tree, "Field").1.value(), None);
    assert!(!named(&tree, "Field").1.supports_action(Action::SetValue));
    window.dispatch_accessibility_action(replace(id, "not a truncated edit"));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().len(), LIMIT + 4));
    let legacy = named(&tree, "Legacy").0;
    assert_eq!(named(&tree, "Legacy").1.value(), None);
    assert!(!named(&tree, "Legacy").1.supports_action(Action::SetValue));
    no_secret(&tree, "generic sentinel");
    window.dispatch_accessibility_action(replace(legacy, "must not reach generic"));
    window.read(|s, cx| assert_eq!(s.generic.read(cx).value().as_ref(), "generic sentinel"));
    // Returning below the limit restores complete value and action, including empty value.
    window.update(|s, w, cx| s.field.update(cx, |s, cx| s.set_value("", w, cx)));
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert_eq!(named(&tree, "Field").1.value(), Some(""));
    assert!(named(&tree, "Field").1.supports_action(Action::SetValue));
    drop(window);
    app.update(|cx| cx.shutdown());
}
#[test]
fn marked_text_and_active_trap_reject_value_replacement_without_history_damage() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut window = app.open_window(host);
    window.activate_accessibility();
    window.draw();
    let id = named(&window.accessibility_tree().unwrap(), "Field").0;
    window.dispatch_accessibility_action(request(id, Action::Focus, None));
    window.update(|s, w, cx| {
        s.field.update(cx, |s, cx| {
            s.replace_and_mark_text_in_range(
                Some(0.."old😀e\u{301}".encode_utf16().count()),
                "に😀",
                Some(3..3),
                w,
                cx,
            );
        })
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    assert_eq!(named(&tree, "Field").1.value(), Some("に😀"));
    assert!(!named(&tree, "Field").1.supports_action(Action::SetValue));
    window.dispatch_accessibility_action(replace(id, "must not end composition"));
    window.update(|s, w, cx| {
        s.field.update(cx, |s, cx| {
            assert_eq!(s.value().as_ref(), "に😀");
            assert!(s.marked_text_range(w, cx).is_some());
            s.replace_text_in_range(None, "日本😀", w, cx);
            assert!(s.marked_text_range(w, cx).is_none());
        })
    });
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "old😀e\u{301}"));
    window.simulate_keystroke("cmd-shift-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "日本😀"));
    window.update(|s, _, _| s.trapped = true);
    window.draw();
    let at = window.update(|_, w, _| w.find("origin").bounds().center());
    window.simulate_click(at, MouseButton::Left);
    window.update(|s, w, cx| {
        assert!(s.origin.is_focused(w));
        let trap = base::active_focus_trap(w, cx).unwrap();
        assert!(trap.contains(&s.field.read(cx).focus_handle(cx), w));
        assert!(!trap.contains(&s.draft.read(cx).focus_handle(cx), w));
    });
    window.draw();
    let tree = window.accessibility_tree().unwrap();
    let id = named(&tree, "Field").0;
    let draft = named(&tree, "Draft").0;
    assert!(named(&tree, "Draft").1.supports_action(Action::SetValue));
    window.dispatch_accessibility_action(replace(draft, "background overwrite"));
    window.read(|s, cx| assert_eq!(s.draft.read(cx).value().as_ref(), "first\n第二行😀"));
    window.dispatch_accessibility_action(replace(id, "allowed inside trap"));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "allowed inside trap"));
    window.update(|s, w, _| assert!(s.origin.is_focused(w), "SetValue doesn't steal focus"));
    window.dispatch_accessibility_action(request(id, Action::Focus, None));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "日本😀"));
    drop(window);
    app.update(|cx| cx.shutdown());
}

#[test]
fn set_value_keeps_single_line_normalization_and_existing_validation() {
    let mut app = TestApp::new();
    app.update(base::init);
    let mut window = app.open_window(|w, cx| {
        let mut s = host(w, cx);
        s.field = cx.new(|cx| {
            InputState::new(w, cx)
                .accessibility_label("Field")
                .default_value("valid😀")
                .validate(|text, _| !text.contains("blocked"))
        });
        s
    });
    window.activate_accessibility();
    window.draw();
    let id = named(&window.accessibility_tree().unwrap(), "Field").0;
    window.dispatch_accessibility_action(request(id, Action::Focus, None));
    window.dispatch_accessibility_action(replace(id, "blocked"));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "valid😀"));
    window.dispatch_accessibility_action(replace(id, "a\r\n😀\nb"));
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "a😀b"));
    window.draw();
    assert_eq!(
        named(&window.accessibility_tree().unwrap(), "Field")
            .1
            .value(),
        Some("a😀b")
    );
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "valid😀"));
    window.simulate_keystroke("cmd-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "valid😀"));
    window.simulate_keystroke("cmd-shift-z");
    window.read(|s, cx| assert_eq!(s.field.read(cx).value().as_ref(), "a😀b"));
    drop(window);
    app.update(|cx| cx.shutdown());
}
