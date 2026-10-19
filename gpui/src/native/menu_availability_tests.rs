//! Real responder validation and keyboard routing on TestPlatform, not AppKit NSMenu.
//! Parent/global handlers below are adversarial test counters, never host edit handlers.
use super::edit_menu;
use gpui_kit::{
    Action, AppContext, ClipboardItem, Context, Entity, Focusable, HitboxBehavior, IntoElement,
    MenuItem, MouseButton, Render, TestApp, TestAppWindow, TestSupportExt, Window,
    base::{
        self, Input, Root, TextSelection, TextSelectionContentKey, TextSelectionDomain,
        TextSelectionHandle, TextSelectionRegistration, TextView,
        input::{Copy, Editor, EditorState, InputState, Paste, Undo},
    },
    canvas, div, point,
    prelude::*,
    px, size,
    test::TestWindowExt,
};
use std::{cell::Cell, rc::Rc};

#[test]
fn action_lists_exclude_denied_globals_without_hiding_local_responders() {
    let mut f = Fixture::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    f.app.update(|cx| {
        cx.on_action(move |_: &Undo, _| count.set(count.get() + 1));
        cx.on_action_available::<Undo>(|_, _| false);
    });
    assert!(!f.available("Undo"));
    assert!(!f.window.update(|_, w, cx| {
        w.available_actions(cx)
            .iter()
            .any(|a| a.as_any().is::<Undo>())
    }));
    f.click("input");
    f.window.simulate_input("x");
    f.window.draw();
    assert!(f.available("Undo"));
    assert!(f.window.update(|_, w, cx| {
        w.available_actions(cx)
            .iter()
            .any(|a| a.as_any().is::<Undo>())
    }));
    f.invoke("Undo");
    assert_eq!(f.input_value(), "Input café");
    assert_eq!(calls.get(), 0);
}

const DOMAIN_TEXT: &str = "Cold source café";

fn descriptor(label: &str) -> Box<dyn Action> {
    edit_menu()
        .items
        .into_iter()
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if name.as_ref() == label => Some(action),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}

struct Host {
    input: Entity<InputState>,
    editor: Entity<EditorState>,
    domain_handle: Option<TextSelectionHandle>,
    show_domain: bool,
    fallbacks: Rc<Cell<usize>>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let fallbacks = self.fallbacks.clone();
        let paste_fallbacks = self.fallbacks.clone();
        let handle = self.domain_handle.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(20.))
            .child(
                div()
                    .id("input")
                    .test_support()
                    .h(px(36.))
                    .on_action(move |_: &Copy, _, _| fallbacks.set(fallbacks.get() + 1))
                    .on_action(move |_: &Paste, _, _| {
                        paste_fallbacks.set(paste_fallbacks.get() + 1)
                    })
                    .child(Input::new(&self.input)),
            )
            .child(
                div()
                    .id("code")
                    .test_support()
                    .h(px(100.))
                    .child(Editor::new(&self.editor)),
            )
            .child(
                div().id("prose").test_support().h(px(30.)).child(
                    TextView::markdown("plain", "Prose café")
                        .selectable(true)
                        .scrollable(false),
                ),
            )
            .when(self.show_domain, |d| {
                d.child(
                    div().id("domain").test_support().h(px(30.)).child(
                        canvas(
                            move |bounds, window, cx| {
                                let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
                                if let Some(handle) = handle {
                                    handle.register(
                                        TextSelectionRegistration::new(hitbox, bounds)
                                            .with_text_bounds(vec![bounds]),
                                        window,
                                        cx,
                                    );
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    ),
                )
            })
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    host: Entity<Host>,
    projections: Rc<Cell<usize>>,
    global_fallbacks: Rc<Cell<usize>>,
    app: TestApp,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.window.update(|_, w, cx| w.blur(cx));
            self.app.run_until_parked();
        }
    }
}
impl Fixture {
    fn new() -> Self {
        let mut app = TestApp::new();
        app.update(base::init);
        let projections = Rc::new(Cell::new(0));
        let counter = projections.clone();
        let global_fallbacks = Rc::new(Cell::new(0));

        let mut host = None;
        let mut window = app.open_window(|w, cx| {
            let h = cx.new(|cx| {
                let domain = TextSelectionDomain::new(
                    |item, _| Some(item),
                    move |range, _| {
                        counter.set(counter.get() + 1);
                        let start = range.anchor.offset.min(range.cursor.offset);
                        let end = range.anchor.offset.max(range.cursor.offset);
                        DOMAIN_TEXT[start..end].into()
                    },
                );
                let domain_handle = TextSelectionHandle::new("", cx);
                domain_handle.set_domain(Some((domain, 1)), cx);
                domain_handle.resolve_content_key_with(
                    |p, _| {
                        let column = (f32::from(p.x).max(0.) / 4.) as usize;
                        let offset = DOMAIN_TEXT
                            .char_indices()
                            .nth(column)
                            .map_or(DOMAIN_TEXT.len(), |(offset, _)| offset);
                        Some(TextSelectionContentKey::new(offset as u64))
                    },
                    cx,
                );
                Host {
                    input: cx.new(|cx| InputState::new(w, cx).default_value("Input café")),
                    editor: cx.new(|cx| EditorState::new(w, cx).default_value("let café = 1;\n")),
                    domain_handle: Some(domain_handle),
                    show_domain: true,
                    fallbacks: Rc::new(Cell::new(0)),
                }
            });
            host = Some(h.clone());
            Root::new(h, w, cx)
        });
        window.simulate_resize(size(px(600.), px(320.)));
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
            projections,
            global_fallbacks,
        }
    }
    fn install_global_copy_fallback(&mut self) {
        let global = self.global_fallbacks.clone();
        self.app.update(|cx| {
            cx.on_action(move |_: &Copy, _| global.set(global.get() + 1));
        });
    }
    fn click(&mut self, id: &str) {
        self.window.draw();
        let p = self
            .window
            .update(|_, w, _| w.find(id.to_owned()).bounds().center());
        self.window.simulate_click(p, MouseButton::Left);
        self.window.draw();
    }
    fn drag(&mut self, id: &str) {
        self.drag_range(id, 2., 80.);
    }
    fn drag_range(&mut self, id: &str, start: f32, end: f32) {
        let window = self.app.windows()[0];
        self.app.update(|cx| {
            cx.update_window(window, |_, w, cx| {
                let b = w.find(id.to_owned()).bounds();
                w.drag(
                    point(b.left() + px(start), b.center().y),
                    point(b.left() + px(end), b.center().y),
                    cx,
                );
            })
            .unwrap();
        });
        self.app.run_until_parked();
        self.window.draw();
    }
    fn available(&mut self, label: &str) -> bool {
        let action = descriptor(label);
        let app_available = self
            .app
            .update(|cx| cx.is_action_available(action.as_ref()));
        let window_available = self
            .window
            .update(|_, w, cx| w.is_action_available(action.as_ref(), cx));
        assert_eq!(
            app_available, window_available,
            "App/Window disagree for {label}"
        );
        app_available
    }
    fn invoke(&mut self, label: &str) {
        let action = descriptor(label);
        self.app.update(|cx| cx.dispatch_action(action.as_ref()));
        self.app.run_until_parked();
        self.window.draw();
    }
    fn key(&mut self, key: &str) {
        self.window.simulate_keystroke(key);
        self.app.run_until_parked();
        self.window.draw();
    }
    fn clipboard(&mut self, value: &str) {
        self.app
            .update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(value.into())));
    }
    fn copied(&self) -> String {
        self.window
            .read(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap())
    }
    fn input_value(&self) -> String {
        self.window
            .read(|_, cx| self.host.read(cx).input.read(cx).value().to_string())
    }
    fn fallback_count(&self) -> usize {
        self.window.read(|_, cx| self.host.read(cx).fallbacks.get()) + self.global_fallbacks.get()
    }
}

#[test]
fn menu_availability_empty_history_selection_and_keyboard_branching() {
    let mut f = Fixture::new();
    f.click("input");
    f.install_global_copy_fallback();
    f.window.update(|_, w, cx| {
        assert!(
            f.host
                .read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    for label in ["Undo", "Redo", "Cut", "Copy"] {
        assert!(!f.available(label), "empty {label}");
    }
    assert!(f.available("Paste"));
    assert!(f.available("Select All"));
    f.clipboard("sentinel");
    f.key("cmd-c");
    f.invoke("Copy");
    assert_eq!(f.copied(), "sentinel");
    assert_eq!(
        f.fallback_count(),
        0,
        "denial must not reach parent or global Copy"
    );
    f.key("cmd-a");
    assert!(f.available("Copy"));
    assert!(f.available("Cut"));
    f.key("cmd-c");
    assert_eq!(f.copied(), "Input café");
    f.key("cmd-x");
    assert_eq!(f.input_value(), "");
    assert!(f.available("Undo"));
    assert!(!f.available("Redo"));
    assert!(!f.available("Select All"));
    f.key("cmd-z");
    assert_eq!(f.input_value(), "Input café");
    assert!(!f.available("Undo"));
    assert!(f.available("Redo"));
    f.key("cmd-shift-z");
    assert_eq!(f.input_value(), "");
    f.invoke("Undo");
    f.clipboard("Replacement λ");
    f.key("cmd-v");
    assert_eq!(f.input_value(), "Replacement λ");
    assert!(f.available("Undo"));
    assert!(!f.available("Redo"), "new edit invalidates redo branch");
    assert_eq!(f.fallback_count(), 0);
}

#[test]
fn menu_availability_readonly_masked_and_disabled_controls() {
    let mut f = Fixture::new();
    f.click("input");
    f.key("cmd-a");
    f.window.update(|_, _, cx| {
        let input = f.host.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_readonly(true, cx));
    });
    assert!(
        !f.available("Paste"),
        "live policy must not wait for a redraw"
    );
    f.window.draw();
    assert!(f.available("Copy"));
    for label in ["Cut", "Paste", "Undo", "Redo"] {
        assert!(!f.available(label), "readonly {label}");
    }
    f.clipboard("sentinel");
    f.key("cmd-v");
    f.key("cmd-x");
    assert_eq!(f.input_value(), "Input café");
    assert_eq!(f.copied(), "sentinel");
    f.key("cmd-c");
    assert_eq!(f.copied(), "Input café");
    f.window.update(|_, w, cx| {
        let input = f.host.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_masked(true, w, cx));
    });
    f.window.draw();
    assert!(!f.available("Copy"));
    f.key("cmd-c");
    assert_eq!(f.fallback_count(), 0);
    f.window.update(|_, _, cx| {
        let input = f.host.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_disabled(true, cx));
    });
    f.window.draw();
    f.click("input");
    f.window.update(|_, w, cx| {
        assert!(
            !f.host
                .read(cx)
                .input
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    for label in ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"] {
        assert!(!f.available(label), "disabled {label}");
    }
}

#[test]
fn menu_availability_code_editor_preserves_stock_copy_policy_and_focus_history() {
    let mut f = Fixture::new();
    f.click("code");
    // This pinned Base version does not copy whole lines without selection.
    // Derive from its actual stock capability, not an input-only assumption.
    let copyable = f
        .window
        .read(|_, cx| f.host.read(cx).editor.read(cx).is_copyable());
    assert_eq!(f.available("Copy"), copyable);
    assert!(!f.available("Undo"));
    assert!(!f.available("Redo"));
    f.key("cmd-a");
    assert!(f.available("Copy"));
    f.key("cmd-c");
    assert_eq!(f.copied(), "let café = 1;\n");
    f.key("cmd-x");
    assert!(f.available("Undo"));
    f.click("input");
    assert!(
        !f.available("Undo"),
        "histories belong to focused responder"
    );
    f.click("code");
    f.key("cmd-z");
    assert!(f.available("Redo"));
    assert_eq!(
        f.window
            .read(|_, cx| f.host.read(cx).editor.read(cx).value().to_string()),
        "let café = 1;\n"
    );
}

#[test]
fn menu_availability_root_selection_and_cold_domain_do_not_project_during_validation() {
    let mut f = Fixture::new();
    assert!(
        !f.available("Copy"),
        "Root with no selection has no Copy work"
    );
    f.drag("prose");
    f.install_global_copy_fallback();
    assert!(f.available("Copy"));
    f.clipboard("sentinel");
    f.key("cmd-c");
    assert_ne!(f.copied(), "sentinel");
    f.window.update(|_, w, cx| TextSelection::clear(w, cx));
    f.window.draw();
    assert!(!f.available("Copy"));
    f.drag("domain");
    assert!(f.available("Copy"));
    f.window.update(|_, _, cx| {
        f.host.update(cx, |h, cx| {
            h.show_domain = false;
            h.domain_handle.take();
            cx.notify();
        });
    });
    f.window.draw();
    f.app.run_until_parked();
    f.window.draw();
    let before = f.projections.get();
    for _ in 0..100 {
        assert!(
            f.available("Copy"),
            "unmounted logical selection still owns Copy"
        );
    }
    assert_eq!(
        f.projections.get(),
        before,
        "validation must not call source provider"
    );
    f.clipboard("sentinel");
    f.key("cmd-c");
    assert_eq!(f.copied(), "Cold source café");
    assert_eq!(
        f.projections.get(),
        before + 1,
        "only actual Copy projects source"
    );
    f.click("input");
    assert!(
        !f.available("Copy"),
        "focused empty input selection shadows Root's retained domain"
    );
    f.key("cmd-c");
    assert_eq!(f.copied(), "Cold source café");
    assert_eq!(f.projections.get(), before + 1);
    assert_eq!(f.fallback_count(), 0);
}

#[test]
fn menu_availability_empty_source_projection_fails_closed() {
    let mut f = Fixture::new();
    // A real logical interval selecting the space after "Cold". Validation
    // must not read source to discover Root's existing trim policy will drop it.
    f.drag_range("domain", 16., 20.);
    f.install_global_copy_fallback();
    let before = f.projections.get();
    assert!(f.available("Copy"));
    assert_eq!(f.projections.get(), before);
    f.clipboard("sentinel");
    f.key("cmd-c");
    assert_eq!(f.copied(), "sentinel");
    assert_eq!(f.projections.get(), before + 1);
    assert_eq!(
        f.fallback_count(),
        0,
        "empty projection must not reach unrelated Copy"
    );
}

#[test]
fn menu_availability_keyboard_denial_never_reaches_parent_paste() {
    let mut f = Fixture::new();
    f.click("input");
    f.window.update(|_, _, cx| {
        let input = f.host.read(cx).input.clone();
        input.update(cx, |s, cx| s.set_readonly(true, cx));
    });
    f.window.draw();
    f.clipboard("sentinel");
    f.key("cmd-v");
    assert_eq!(
        f.fallback_count(),
        0,
        "readonly keyboard Paste must not reach a parent handler"
    );
    assert_eq!(f.input_value(), "Input café");
    assert_eq!(f.copied(), "sentinel");
    assert!(!f.available("Paste"));
}

#[test]
fn menu_availability_paste_safe_failure_and_no_window() {
    let mut f = Fixture::new();
    f.click("input");
    assert!(
        f.available("Paste"),
        "validation must not inspect or request clipboard permissions"
    );
    f.clipboard("");
    f.key("cmd-v");
    assert_eq!(f.input_value(), "Input café");
    assert!(!f.available("Undo"));
    let mut app = TestApp::new();
    app.update(base::init);
    app.update(|cx| {
        assert!(cx.active_window().is_none());
        for label in ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"] {
            assert!(
                !cx.is_action_available(descriptor(label).as_ref()),
                "no window {label}"
            );
        }
    });
}
