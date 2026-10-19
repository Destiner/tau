//! Native key dispatch through an actual Base Root, including unfocused startup.
use super::markdown::MarkdownView;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, KeyUpEvent,
    Keystroke, Render, TestApp, TestAppWindow, TestSupportExt, Window,
    base::{
        Dialog, Root,
        input::{Input, InputState},
    },
    div,
    prelude::*,
    px, size,
    test::TestWindowExt,
};

struct Host {
    markdown: Entity<MarkdownView>,
    editor: Entity<InputState>,
    show_editor: bool,
    dialog: bool,
    trap: FocusHandle,
    last: FocusHandle,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .p(px(40.))
            .when(self.show_editor, |d| {
                d.child(
                    div()
                        .id("editor")
                        .test_support()
                        .h(px(32.))
                        .child(Input::new(&self.editor)),
                )
            })
            .child(self.markdown.clone())
            .when(self.dialog, |d| {
                d.child(
                    Dialog::new(cx)
                        .open(true)
                        .focus_handle(self.trap.clone())
                        .popup(
                            div()
                                .flex_col()
                                .w(px(200.))
                                .h(px(100.))
                                .child(
                                    div()
                                        .id("first")
                                        .test_support()
                                        .h(px(40.))
                                        .child(Input::new(&self.editor)),
                                )
                                .child(
                                    gpui_kit::base::Button::new("last")
                                        .track_focus(&self.last)
                                        .h(px(40.))
                                        .child("Last"),
                                ),
                        ),
                )
            })
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    host: Entity<Host>,
    markdown: Entity<MarkdownView>,
    app: TestApp,
}
impl Fixture {
    fn new(editor: bool) -> Self {
        let mut app = TestApp::new();
        app.update(|cx| {
            super::init(cx, false);
            // Both production and test entry points may request navigation.
            super::bind_native_navigation(cx);
            super::bind_native_navigation(cx);
        });
        let mut entities = None;
        let mut window = app.open_window(|w, cx| {
            let markdown = cx.new(|cx| {
                MarkdownView::new(
                    "```rs\nlet a = 1;\n```\n\n```rs\nlet b = 2;\n```",
                    false,
                    |_| {},
                    cx,
                )
            });
            let host = cx.new(|cx| Host {
                markdown: markdown.clone(),
                editor: cx.new(|cx| {
                    let mut input = InputState::new(w, cx);
                    input.set_value("retained draft", w, cx);
                    input
                }),
                show_editor: editor,
                dialog: false,
                trap: cx.focus_handle(),
                last: cx.focus_handle(),
            });
            entities = Some((host.clone(), markdown));
            Root::new(host, w, cx)
        });
        let (host, markdown) = entities.unwrap();
        window.simulate_resize(size(px(900.), px(700.)));
        window.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert!(w.is_window_active());
            assert!(
                w.focused(cx).is_none(),
                "fixture must start with no owned focus"
            );
            assert!(w.find("copy-1").visible());
            assert!(!w.find("copy-paint-1").visible());
        });
        Self {
            app,
            window,
            host,
            markdown,
        }
    }
    fn press(&mut self, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        self.window.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        self.window.simulate_event(KeyUpEvent { keystroke });
        self.app.run_until_parked();
        self.window.draw();
    }
    fn copy_focused(&mut self, id: &'static str) -> bool {
        self.window
            .update(|_, w, _| w.find(id).focused() == Some(true))
    }
}

#[test]
fn cold_root_tab_reveals_and_activates_markdown_copy() {
    let tabs = if cfg!(target_os = "macos") {
        vec!["tab", "alt-tab"]
    } else {
        vec!["tab"]
    };
    for tab in tabs {
        for activation in ["enter", "space"] {
            let mut f = Fixture::new(false);
            f.press(tab);
            assert!(f.copy_focused("copy-1"), "first {tab}");
            f.window
                .update(|_, w, _| assert!(w.find("copy-paint-1").visible()));
            f.press(activation);
            f.window.update(|_, _, cx| {
                assert_eq!(
                    f.markdown.read(cx).copied_block(),
                    Some(0),
                    "{tab} -> {activation}"
                );
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    "let a = 1;\n"
                );
            });
            assert!(f.copy_focused("copy-1"));
        }
    }
}

#[test]
fn cold_shift_tab_and_focused_navigation_move_exactly_once() {
    let pairs = if cfg!(target_os = "macos") {
        vec![("tab", "shift-tab"), ("alt-tab", "alt-shift-tab")]
    } else {
        vec![("tab", "shift-tab")]
    };
    for (next, prev) in pairs {
        let mut f = Fixture::new(false);
        f.press(prev);
        assert!(f.copy_focused("copy-2"), "cold {prev}");
        f.press(prev);
        assert!(f.copy_focused("copy-1"));
        f.press(next);
        assert!(f.copy_focused("copy-2"));
        f.press(next);
        assert!(f.copy_focused("copy-1"));
    }
}

#[test]
fn unrelated_modifiers_do_not_bootstrap_focus() {
    let mut f = Fixture::new(false);
    for key in [
        "ctrl-tab",
        "ctrl-shift-tab",
        "cmd-tab",
        "cmd-shift-tab",
        "ctrl-alt-tab",
        "alt-c",
    ] {
        f.press(key);
        f.window
            .update(|_, w, cx| assert!(w.focused(cx).is_none(), "{key}"));
    }
    #[cfg(not(target_os = "macos"))]
    {
        f.press("alt-tab");
        f.window.update(|_, w, cx| assert!(w.focused(cx).is_none()));
    }
}

#[test]
fn owned_input_keeps_normal_base_navigation_and_unrelated_keys() {
    let mut f = Fixture::new(true);
    f.press("tab");
    f.window.update(|_, w, cx| {
        assert!(
            f.host
                .read(cx)
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    for key in ["ctrl-tab", "cmd-tab", "alt-c"] {
        f.press(key);
        f.window.update(|_, w, cx| {
            assert!(
                f.host
                    .read(cx)
                    .editor
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w),
                "{key}"
            )
        });
    }
    f.press("tab");
    assert!(f.copy_focused("copy-1"));
    f.press("shift-tab");
    f.window.update(|_, w, cx| {
        let editor = f.host.read(cx).editor.read(cx);
        assert!(editor.focus_handle(cx).is_focused(w));
        assert_eq!(editor.value().as_str(), "retained draft");
    });
    f.press("tab");
    assert!(f.copy_focused("copy-1"));
}

struct EditorHost(Entity<InputState>);
impl Render for EditorHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("other-editor")
                .test_support()
                .w(px(300.))
                .h(px(32.))
                .child(Input::new(&self.0)),
        )
    }
}

#[test]
fn inactive_window_does_not_bootstrap_and_other_window_owns_its_navigation() {
    let mut f = Fixture::new(false);
    let mut other = f.app.open_window(|w, cx| {
        EditorHost(cx.new(|cx| {
            let mut input = InputState::new(w, cx);
            input.set_value("other draft", w, cx);
            input
        }))
    });
    other.update(|_, w, _| w.activate_window());
    f.app.run_until_parked();
    other.draw();
    f.window.update(|_, w, _| assert!(!w.is_window_active()));
    f.press("tab");
    f.window.update(|_, w, cx| assert!(w.focused(cx).is_none()));
    // A non-Base window is outside this application's navigation fallback.
    other.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("tab").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    other.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("tab").unwrap(),
    });
    other.update(|_, w, cx| assert!(w.focused(cx).is_none()));
    let bounds = other.update(|_, w, _| w.find("other-editor").bounds());
    other.simulate_click(bounds.center(), gpui_kit::MouseButton::Left);
    other.draw();
    for key in ["tab", "shift-tab", "alt-tab", "ctrl-tab", "cmd-tab"] {
        other.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        other.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        other.update(|s, w, cx| {
            assert!(s.0.read(cx).focus_handle(cx).is_focused(w), "{key}");
            assert_eq!(s.0.read(cx).value().as_str(), "other draft");
        });
        f.window.update(|_, w, cx| assert!(w.focused(cx).is_none()));
    }
    other.update(|_, w, cx| w.blur(cx));
    f.app.run_until_parked();
}

#[test]
fn real_dialog_focus_trap_retains_forward_and_backward_navigation() {
    let mut f = Fixture::new(false);
    f.window.update(|_, _, cx| {
        f.host.update(cx, |s, cx| {
            s.dialog = true;
            cx.notify();
        })
    });
    f.window.draw();
    // A real pointer press enters the dialog; no test-only focus seeding.
    let bounds = f.window.update(|_, w, _| w.find("first").bounds());
    f.window
        .simulate_click(bounds.center(), gpui_kit::MouseButton::Left);
    f.window.draw();
    f.window
        .update(|_, w, cx| assert!(gpui_kit::base::active_focus_trap(w, cx).is_some()));
    let pairs = if cfg!(target_os = "macos") {
        vec![("tab", "shift-tab"), ("alt-tab", "alt-shift-tab")]
    } else {
        vec![("tab", "shift-tab")]
    };
    for (next, prev) in pairs {
        for (key, last) in [(next, true), (next, false), (prev, true), (prev, false)] {
            f.press(key);
            f.window.update(|_, w, cx| {
                let host = f.host.read(cx);
                assert!(host.trap.contains_focused(w, cx), "{key} escaped dialog");
                assert_eq!(host.last.is_focused(w), last, "{key} must move once");
                assert_eq!(host.editor.read(cx).focus_handle(cx).is_focused(w), !last);
                assert!(host.markdown.read(cx).copied_block().is_none());
            });
        }
    }
    f.press("tab");
    f.window
        .update(|_, w, cx| assert!(f.host.read(cx).last.is_focused(w)));
}
