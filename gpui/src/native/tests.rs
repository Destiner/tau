//! Descriptor dispatch and real Root responders on TestPlatform, not AppKit menu validation.
use super::edit_menu;
use crate::components::{
    composer::{Composer, ComposerOptions},
    markdown::MarkdownView,
};
use gpui_kit::{
    Action, AppContext, ClipboardItem, Context, Entity, FocusHandle, Focusable, IntoElement,
    MenuItem, MouseButton, OsAction, Render, TestApp, TestAppWindow, TestSupportExt, Window,
    base::{
        Dialog, Input, Root,
        input::{Copy, Cut, InputState, Paste, Redo, SelectAll, Undo},
    },
    div, point,
    prelude::*,
    px, size,
    test::TestWindowExt,
};

fn action(label: &str) -> Box<dyn Action> {
    edit_menu()
        .items
        .into_iter()
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if name.as_ref() == label => Some(action),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}

#[test]
fn edit_menu_descriptors() {
    let menu = edit_menu();
    assert_eq!(menu.name.as_ref(), "Edit");
    assert!(!menu.disabled);
    assert_eq!(menu.items.len(), 8);
    let expected: [(usize, &str, Box<dyn Action>, OsAction); 6] = [
        (0, "Undo", Box::new(Undo), OsAction::Undo),
        (1, "Redo", Box::new(Redo), OsAction::Redo),
        (3, "Cut", Box::new(Cut), OsAction::Cut),
        (4, "Copy", Box::new(Copy), OsAction::Copy),
        (5, "Paste", Box::new(Paste), OsAction::Paste),
        (7, "Select All", Box::new(SelectAll), OsAction::SelectAll),
    ];
    for (index, label, expected_action, os) in expected {
        let MenuItem::Action {
            name,
            action,
            os_action,
            checked,
            disabled,
        } = &menu.items[index]
        else {
            panic!("expected action at {index}")
        };
        assert_eq!(name.as_ref(), label);
        assert_eq!(
            action.as_any().type_id(),
            expected_action.as_any().type_id()
        );
        assert!(*os_action == Some(os), "{label} OS metadata");
        assert!(!checked && !disabled);
    }
    for index in [2, 6] {
        assert!(matches!(menu.items[index], MenuItem::Separator));
    }
}

const DRAFT: &str = "Composer café 😀\n第二行";
const INPUT: &str = "Input naïve 🦀";
const REPLACEMENT: &str = "貼り付け 🦀 e\u{301}";

struct Host {
    composer: Entity<Composer>,
    input: Entity<InputState>,
    overlay: Entity<InputState>,
    markdown: Entity<MarkdownView>,
    show_overlay: bool,
    trap: FocusHandle,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(30.))
            .child(
                div()
                    .id("base-input")
                    .test_support()
                    .h(px(36.))
                    .child(Input::new(&self.input)),
            )
            .child(
                div()
                    .id("prose")
                    .test_support()
                    .h(px(80.))
                    .child(self.markdown.clone()),
            )
            .child(self.composer.clone())
            .when(self.show_overlay, |d| {
                d.child(
                    Dialog::new(cx)
                        .open(true)
                        .focus_handle(self.trap.clone())
                        .popup(
                            div().w(px(300.)).h(px(100.)).child(
                                div()
                                    .id("overlay-input")
                                    .test_support()
                                    .h(px(36.))
                                    .child(Input::new(&self.overlay)),
                            ),
                        ),
                )
            })
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    host: Entity<Host>,
    app: TestApp,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        // Release TestPlatform's focused input handler before TestApp checks for leaks.
        self.window.update(|_, w, cx| w.blur(cx));
        self.app.run_until_parked();
    }
}
impl Fixture {
    fn new() -> Self {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut host = None;
        let mut window = app.open_window(|w, cx| {
            let view = cx.new(|cx| Host {
                composer: cx.new(|cx| {
                    Composer::new(
                        ComposerOptions {
                            models: vec![],
                            model_heading: "Models".into(),
                            efforts: vec![],
                            model: String::new(),
                            effort: String::new(),
                            draft: DRAFT.into(),
                            dark: false,
                            disabled: false,
                            streaming: false,
                        },
                        w,
                        cx,
                    )
                }),
                input: cx.new(|cx| {
                    let mut s = InputState::new(w, cx);
                    s.set_value(INPUT, w, cx);
                    s
                }),
                overlay: cx.new(|cx| {
                    let mut s = InputState::new(w, cx);
                    s.set_value("Overlay λ", w, cx);
                    s
                }),
                markdown: cx.new(|cx| {
                    MarkdownView::new("Prose café 😀", false, |_| {}, cx).natural_height()
                }),
                show_overlay: false,
                trap: cx.focus_handle(),
            });
            host = Some(view.clone());
            Root::new(view, w, cx)
        });
        window.simulate_resize(size(px(800.), px(650.)));
        window.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        app.run_until_parked();
        window.draw();
        Self {
            app,
            window,
            host: host.unwrap(),
        }
    }
    fn click(&mut self, id: &str) {
        self.window.draw();
        let position = self
            .window
            .update(|_, w, _| w.find(id.to_owned()).bounds().center());
        self.window.simulate_click(position, MouseButton::Left);
        self.window.draw();
    }
    fn invoke(&mut self, label: &str) {
        let descriptor = action(label);
        self.app.update(|cx| {
            assert!(
                cx.active_window().is_some(),
                "must exercise App's active-window route"
            );
            cx.dispatch_action(descriptor.as_ref());
        });
        self.app.run_until_parked();
        self.window.draw();
    }
    fn clipboard(&mut self, text: &str) {
        self.app
            .update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text.into())));
    }
    fn copied(&self) -> String {
        self.window
            .read(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap())
    }
    fn value(&self, id: &str) -> String {
        self.window.read(|_, cx| {
            let h = self.host.read(cx);
            match id {
                "editor" => h.composer.read(cx).editor().read(cx).value().to_string(),
                "base-input" => h.input.read(cx).value().to_string(),
                "overlay-input" => h.overlay.read(cx).value().to_string(),
                _ => panic!("unknown editor"),
            }
        })
    }
    fn assert_focused(&mut self, id: &str) {
        self.window.update(|_, w, cx| {
            let h = self.host.read(cx);
            let focused = match id {
                "editor" => h
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w),
                "base-input" => h.input.read(cx).focus_handle(cx).is_focused(w),
                "overlay-input" => h.overlay.read(cx).focus_handle(cx).is_focused(w),
                _ => false,
            };
            assert!(focused, "{id} must own focus");
        });
    }
}

#[test]
fn focused_base_input_and_composer_unicode_editing_from_menu() {
    for (id, initial, untouched, retained) in [
        ("base-input", INPUT, "editor", DRAFT),
        ("editor", DRAFT, "base-input", INPUT),
    ] {
        let mut f = Fixture::new();
        f.click(id);
        f.assert_focused(id);
        f.invoke("Select All");
        f.invoke("Copy");
        assert_eq!(f.copied(), initial);
        f.invoke("Cut");
        assert_eq!(f.value(id), "");
        assert_eq!(f.copied(), initial);
        f.invoke("Undo");
        assert_eq!(f.value(id), initial);
        f.invoke("Redo");
        assert_eq!(f.value(id), "");
        f.clipboard(REPLACEMENT);
        f.invoke("Paste");
        assert_eq!(f.value(id), REPLACEMENT);
        f.invoke("Undo");
        assert_eq!(f.value(id), "");
        f.invoke("Redo");
        assert_eq!(f.value(id), REPLACEMENT);
        f.invoke("Select All");
        f.invoke("Copy");
        assert_eq!(f.copied(), REPLACEMENT);
        assert_eq!(f.value(untouched), retained);
    }
}

#[test]
fn switching_editors_routes_selection_and_history_to_current_focus() {
    let mut f = Fixture::new();
    for (id, value) in [
        ("editor", "Composer changed 😀"),
        ("base-input", "Input changed λ"),
    ] {
        f.click(id);
        f.assert_focused(id);
        f.invoke("Select All");
        f.clipboard(value);
        f.invoke("Paste");
    }
    f.invoke("Undo");
    assert_eq!(f.value("base-input"), INPUT);
    assert_eq!(f.value("editor"), "Composer changed 😀");
    f.click("editor");
    f.invoke("Undo");
    assert_eq!(f.value("editor"), DRAFT);
    f.invoke("Redo");
    f.invoke("Select All");
    f.invoke("Copy");
    assert_eq!(f.copied(), "Composer changed 😀");
    assert_eq!(f.value("base-input"), INPUT);
    f.click("base-input");
    f.invoke("Redo");
    f.invoke("Select All");
    f.invoke("Copy");
    assert_eq!(f.copied(), "Input changed λ");
    assert_eq!(f.value("editor"), "Composer changed 😀");
}

#[test]
fn focused_dialog_input_never_edits_background_composer() {
    let mut f = Fixture::new();
    f.click("editor");
    f.invoke("Select All");
    let selection = f.window.read(|_, cx| {
        f.host
            .read(cx)
            .composer
            .read(cx)
            .editor()
            .read(cx)
            .selected_range()
    });
    f.window.update(|_, _, cx| {
        f.host.update(cx, |h, cx| {
            h.show_overlay = true;
            cx.notify();
        })
    });
    f.click("overlay-input");
    f.assert_focused("overlay-input");
    f.window
        .update(|_, w, cx| assert!(gpui_kit::base::active_focus_trap(w, cx).is_some()));
    f.invoke("Select All");
    f.invoke("Copy");
    assert_eq!(f.copied(), "Overlay λ");
    f.invoke("Cut");
    f.invoke("Undo");
    f.invoke("Redo");
    f.clipboard(REPLACEMENT);
    f.invoke("Paste");
    assert_eq!(f.value("overlay-input"), REPLACEMENT);
    f.window.read(|_, cx| {
        assert_eq!(
            f.host
                .read(cx)
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .selected_range(),
            selection
        );
    });
    assert_eq!(f.value("editor"), DRAFT);
    assert_eq!(f.value("base-input"), INPUT);
}

#[test]
fn root_prose_copy_beats_unfocused_editor_selection() {
    let mut f = Fixture::new();
    f.click("editor");
    f.invoke("Select All");
    let handle = f.app.windows()[0];
    f.app.update(|cx| {
        cx.update_window(handle, |_, w, cx| {
            let b = f.host.read(cx).markdown.read(cx).text().read(cx).bounds();
            w.drag(
                point(b.left() + px(1.), b.center().y),
                point(b.right() - px(1.), b.center().y),
                cx,
            );
        })
        .unwrap();
    });
    f.app.run_until_parked();
    f.window.draw();
    f.window.update(|_, w, cx| {
        let h = f.host.read(cx);
        assert!(
            !h.composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        );
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(w, cx),
            "Prose café 😀\n"
        );
    });
    f.clipboard("sentinel");
    f.invoke("Copy");
    assert_eq!(f.copied(), "Prose café 😀");
    for label in ["Cut", "Paste", "Undo", "Redo", "Select All"] {
        f.invoke(label);
    }
    assert_eq!(f.value("editor"), DRAFT);
    assert_eq!(f.value("base-input"), INPUT);
}

#[test]
fn no_focus_and_no_active_window_are_inert() {
    let mut f = Fixture::new();
    f.window.update(|_, w, cx| assert!(w.focused(cx).is_none()));
    f.clipboard("sentinel");
    for label in ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"] {
        f.invoke(label);
    }
    assert_eq!(f.copied(), "sentinel");
    assert_eq!(f.value("editor"), DRAFT);
    assert_eq!(f.value("base-input"), INPUT);
    let mut app = TestApp::new();
    app.update(|cx| {
        crate::components::init(cx, false);
        assert!(cx.active_window().is_none());
        cx.write_to_clipboard(ClipboardItem::new_string("isolated".into()));
        for label in ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"] {
            cx.dispatch_action(action(label).as_ref());
        }
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "isolated"
        );
    });
    assert_eq!(f.copied(), "sentinel");
}

#[test]
fn disabled_composer_cannot_receive_background_menu_edits() {
    let mut f = Fixture::new();
    f.click("editor");
    f.invoke("Select All");
    f.window.update(|_, _, cx| {
        let composer = f.host.read(cx).composer.clone();
        composer.update(cx, |s, cx| s.set_status(true, false, cx));
    });
    f.click("base-input");
    f.assert_focused("base-input");
    f.invoke("Select All");
    f.invoke("Cut");
    f.invoke("Undo");
    f.invoke("Redo");
    f.clipboard(REPLACEMENT);
    f.invoke("Paste");
    assert_eq!(f.value("base-input"), REPLACEMENT);
    assert_eq!(f.value("editor"), DRAFT);
    f.click("editor");
    f.window.update(|_, w, cx| {
        let composer = f.host.read(cx).composer.read(cx);
        assert!(composer.is_disabled());
        assert!(!composer.editor().read(cx).focus_handle(cx).is_focused(w));
    });
    for label in ["Cut", "Paste", "Undo", "Redo", "Select All"] {
        f.invoke(label);
        assert_eq!(f.value("editor"), DRAFT);
    }
}

#[test]
fn readonly_input_keeps_copy_but_rejects_menu_mutations() {
    let mut f = Fixture::new();
    f.window.update(|_, _, cx| {
        let input = f.host.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_readonly(true, cx));
    });
    f.click("base-input");
    f.assert_focused("base-input");
    f.invoke("Select All");
    f.invoke("Copy");
    assert_eq!(f.copied(), INPUT);
    f.clipboard(REPLACEMENT);
    for label in ["Cut", "Paste", "Undo", "Redo"] {
        f.invoke(label);
    }
    assert_eq!(f.value("base-input"), INPUT);
    assert_eq!(f.value("editor"), DRAFT);
}
