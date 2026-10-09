//! Actual rendered AccessKit labels, not text-value projection or VoiceOver certification.
use crate::components::{
    composer::{Choice, Composer, ComposerOptions},
    files::{FileContent, FileViewer},
    support::{self, ButtonVariant},
};
use gpui_kit::{
    Context, Entity, IntoElement, Render, TestApp, Window, accesskit, base::Root, div, prelude::*,
};

struct Host(Entity<Composer>);
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(support::button(
                "synthetic",
                "Synthetic action",
                ButtonVariant::Ghost,
                false,
            ))
            .child(support::button(
                "synthetic-primary",
                "Synthetic primary",
                ButtonVariant::Primary,
                false,
            ))
            .child(support::button_sm(
                "synthetic-secondary",
                "Synthetic secondary",
                ButtonVariant::Secondary,
                false,
            ))
            .child(self.0.clone())
    }
}

fn labeled<'a>(tree: &'a accesskit::TreeUpdate, label: &str) -> &'a accesskit::Node {
    &tree
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .unwrap_or_else(|| panic!("missing rendered label {label}: {tree:#?}"))
        .1
}

#[test]
fn rendered_root_labels_follow_actual_composer_action_without_hover() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut composer = None;
    let mut window = app.open_window(|window, cx| {
        let entity = cx.new(|cx| {
            Composer::new(
                ComposerOptions {
                    models: vec![Choice {
                        id: "synthetic-model".into(),
                        label: "Synthetic model".into(),
                        element_id: "synthetic-model".into(),
                    }],
                    efforts: vec![Choice {
                        id: "synthetic-effort".into(),
                        label: "Synthetic effort".into(),
                        element_id: "synthetic-effort".into(),
                    }],
                    model_heading: "Models".into(),
                    model: "synthetic-model".into(),
                    effort: "synthetic-effort".into(),
                    draft: String::new(),
                    dark: false,
                    disabled: false,
                    streaming: false,
                },
                window,
                cx,
            )
        });
        composer = Some(entity.clone());
        let host = cx.new(|_| Host(entity));
        Root::new(host, window, cx)
    });
    let composer = composer.unwrap();
    assert!(window.accessibility_tree().is_none());
    window.activate_accessibility();
    for (streaming, draft, action) in [
        (false, "", "Send"),
        (true, "", "Stop"),
        (true, "synthetic draft", "Queue"),
    ] {
        window.update(|_, window, cx| {
            composer.update(cx, |composer, cx| composer.set_status(false, streaming, cx));
            let editor = composer.read(cx).editor();
            editor.update(cx, |editor, cx| editor.set_value(draft, window, cx));
            window.refresh();
        });
        window.draw();
        let tree = window
            .accessibility_tree()
            .expect("delivered rendered tree");
        for label in [
            "Synthetic action",
            "Synthetic primary",
            "Synthetic secondary",
            "Model: Synthetic model",
            "Effort: Synthetic effort",
            action,
        ] {
            assert_eq!(labeled(&tree, label).role(), accesskit::Role::Button);
        }
        assert_eq!(
            labeled(&tree, "Message").role(),
            accesskit::Role::MultilineTextInput
        );
        for other in ["Send", "Queue", "Stop"] {
            if other != action {
                assert!(
                    tree.nodes
                        .iter()
                        .all(|(_, node)| node.label() != Some(other))
                );
            }
        }
    }
}

#[test]
fn rendered_file_viewer_close_label_without_hover() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let mut window = app.open_window(|window, cx| {
        let viewer = cx.new(|cx| {
            FileViewer::new(
                "synthetic.txt",
                "synthetic",
                FileContent::Text("Synthetic content".into()),
                None,
                false,
                window,
                cx,
            )
        });
        Root::new(viewer, window, cx)
    });
    window.activate_accessibility();
    window.draw();
    let tree = window
        .accessibility_tree()
        .expect("delivered rendered tree");
    let close = labeled(&tree, "Close file viewer");
    assert_eq!(close.role(), accesskit::Role::Button);
    assert!(close.supports_action(accesskit::Action::Click));
}
