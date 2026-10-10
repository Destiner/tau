//! TestPlatform contracts using the real OS-facing EntityInputHandler on focused Base entities.
//! Not AppKit candidate-window proof: macOS offers keys to NSTextInputContext before GPUI.
//!
//! Manual release gate (real macOS window; no TestPlatform substitute): Japanese
//! and Chinese candidate updates, arrows, Return confirmation without submission,
//! then a second Return submitting; Escape cancellation with a model popup open;
//! dead-key accent commit; replacing selected non-BMP text; undo/redo after each.
//! Verify candidate positioning after scrolling and focus transfer between inputs.
//! `unmarkText` retains text; an OS cancellation that removes text must deliver a
//! replacement (such as empty marked text), not merely `unmarkText`.
use crate::components::composer::{Composer, ComposerOptions, Event};
use gpui_kit::{
    AppContext, Context, Entity, EntityInputHandler, Focusable, IntoElement, KeyDownEvent,
    MouseButton, Render, Subscription, TestApp, TestAppWindow, TestSupportExt, Window,
    base::{
        Input, Root,
        input::{InputState, TextareaState},
    },
    div,
    prelude::*,
    px,
    test::TestWindowExt,
};
use std::{cell::RefCell, ops::Range, rc::Rc};

const SEED: &str = "A😀Z";
struct Host {
    composer: Entity<Composer>,
    input: Entity<InputState>,
    escaped: Rc<RefCell<usize>>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(20.))
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, _, _| {
                if e.keystroke.key == "escape" {
                    *s.escaped.borrow_mut() += 1;
                }
            }))
            .child(
                div()
                    .id("ime-input")
                    .test_support()
                    .h(px(36.))
                    .child(Input::new(&self.input)),
            )
            .child(self.composer.clone())
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    composer: Entity<Composer>,
    editor: Entity<TextareaState>,
    input: Entity<InputState>,
    escaped: Rc<RefCell<usize>>,
    sent: Rc<RefCell<Vec<String>>>,
    _subscription: Subscription,
    single: bool,
    app: TestApp,
}
macro_rules! edit {
    ($f:expr, |$s:ident, $w:ident, $cx:ident| $body:expr) => {{
        let f = &mut *$f;
        f.window.update(|_, $w, $cx| {
            if f.single {
                f.input.update($cx, |$s, $cx| $body)
            } else {
                f.editor.update($cx, |$s, $cx| $body)
            }
        })
    }};
}
impl Fixture {
    fn new(single: bool) -> Self {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let escaped = Rc::new(RefCell::new(0));
        let sent = Rc::new(RefCell::new(vec![]));
        let mut entities = None;
        let mut window = app.open_window(|w, cx| {
            let composer = cx.new(|cx| {
                Composer::new(
                    ComposerOptions {
                        models: vec![],
                        model_heading: "Models".into(),
                        efforts: vec![],
                        model: String::new(),
                        effort: String::new(),
                        draft: SEED.into(),
                        dark: false,
                        disabled: false,
                        streaming: false,
                    },
                    w,
                    cx,
                )
            });
            let editor = composer.read(cx).editor();
            let input = cx.new(|cx| {
                let mut s = InputState::new(w, cx);
                s.set_value(SEED, w, cx);
                s.set_protect_ime_keys(true);
                s
            });
            let target = sent.clone();
            let subscription = cx.subscribe(&composer, move |_, _, event, _| {
                if let Event::Send(text) | Event::FollowUp(text) = event {
                    target.borrow_mut().push(text.clone());
                }
            });
            let view = cx.new(|_| Host {
                composer: composer.clone(),
                input: input.clone(),
                escaped: escaped.clone(),
            });
            entities = Some((composer, editor, input, subscription));
            Root::new(view, w, cx)
        });
        window.draw();
        let (composer, editor, input, subscription) = entities.unwrap();
        let mut f = Self {
            app,
            window,
            composer,
            editor,
            input,
            escaped,
            sent,
            _subscription: subscription,
            single,
        };
        let at = f.window.update(|_, w, _| {
            w.find(if single { "ime-input" } else { "editor" })
                .bounds()
                .center()
        });
        f.window.simulate_click(at, MouseButton::Left);
        f.window.draw();
        edit!(&mut f, |s, w, cx| assert!(s.focus_handle(cx).is_focused(w)));
        f
    }
    fn marked(
        &mut self,
        replacement: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
    ) {
        edit!(self, |s, w, cx| s.replace_and_mark_text_in_range(
            replacement,
            text,
            selection,
            w,
            cx
        ));
        self.window.draw();
    }
    fn commit(&mut self, replacement: Option<Range<usize>>, text: &str) {
        edit!(self, |s, w, cx| s.replace_text_in_range(
            replacement,
            text,
            w,
            cx
        ));
        self.window.draw();
    }
    fn check(&mut self, text: &str, marked: Option<Range<usize>>, selected: Range<usize>) {
        edit!(self, |s, w, cx| {
            assert_eq!(s.value().as_ref(), text);
            assert_eq!(s.marked_text_range(w, cx), marked);
            let selection = s.selected_text_range(false, w, cx).unwrap();
            assert_eq!(selection.range, selected);
            assert!(!selection.reversed);
            let mut adjusted = None;
            assert_eq!(
                s.text_for_range(0..text.encode_utf16().count(), &mut adjusted, w, cx)
                    .as_deref(),
                Some(text)
            );
            assert_eq!(adjusted, Some(0..text.encode_utf16().count()));
        });
    }
    fn key(&mut self, key: &str) {
        self.window.simulate_keystroke(key);
        self.window.draw();
    }
    fn value(&mut self) -> String {
        edit!(self, |s, _w, _cx| s.value().to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.window.update(|_, w, cx| w.blur(cx));
            self.app.run_until_parked();
        }
    }
}

#[test]
fn ime_secondary_enter_submits_only_after_composition_commits() {
    let mut f = Fixture::new(false);
    let key = if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    };
    f.marked(Some(0..4), "稍后", Some(2..2));
    f.key(key);
    assert!(f.sent.borrow().is_empty());
    f.check("稍后", Some(0..2), 2..2);
    f.commit(None, "稍后");
    f.key(key);
    assert_eq!(*f.sent.borrow(), vec!["稍后"]);
    assert_eq!(f.value(), "");
}

#[test]
fn ime_utf16_multi_update_commit_and_atomic_undo_through_root() {
    for single in [false, true] {
        let mut f = Fixture::new(single);
        f.marked(Some(1..3), "n", Some(1..1));
        f.check("AnZ", Some(1..2), 2..2);
        f.marked(None, "ni😀", Some(2..4));
        f.check("Ani😀Z", Some(1..5), 3..5);
        f.marked(None, "你", Some(1..1));
        f.check("A你Z", Some(1..2), 2..2);
        f.commit(None, "你好");
        f.check("A你好Z", None, 3..3);
        f.commit(None, "!");
        f.key("cmd-z");
        assert_eq!(f.value(), "A你好Z", "post-commit typing is separate");
        f.key("cmd-z");
        assert_eq!(f.value(), SEED, "all marked updates are one transaction");
        f.key("cmd-shift-z");
        assert_eq!(f.value(), "A你好Z");
    }
}

#[test]
fn ime_unmark_retains_text_cancel_removes_only_marked_text() {
    for single in [false, true] {
        let mut f = Fixture::new(single);
        f.marked(Some(3..3), "é", Some(1..1));
        edit!(&mut f, |s, w, cx| s.unmark_text(w, cx));
        f.check("A😀éZ", None, 4..4);
        f.commit(None, "!");
        f.key("cmd-z");
        assert_eq!(f.value(), "A😀éZ");
        f.key("cmd-z");
        assert_eq!(f.value(), SEED);
        // Legitimate OS cancellation delivery: setMarkedText(empty), then unmarkText.
        f.marked(Some(3..3), "x", Some(1..1));
        f.marked(None, "", Some(0..0));
        edit!(&mut f, |s, w, cx| s.unmark_text(w, cx));
        f.check(SEED, None, 3..3);
    }
}

#[test]
fn ime_dead_key_combining_grapheme_and_explicit_utf16_replacement() {
    for single in [false, true] {
        let mut f = Fixture::new(single);
        f.marked(Some(3..3), "´", Some(1..1));
        f.commit(None, "e\u{301}");
        f.check("A😀e\u{301}Z", None, 5..5);
        f.key("left");
        f.check("A😀e\u{301}Z", None, 3..3);
        f.key("right");
        f.check("A😀e\u{301}Z", None, 5..5);
        // Base deliberately uses scalar deletion (like macOS decomposed accents),
        // while horizontal navigation treats the combining sequence as a grapheme.
        f.key("backspace");
        f.check("A😀eZ", None, 4..4);
        f.key("cmd-z");
        assert_eq!(f.value(), "A😀e\u{301}Z");
        f.commit(Some(1..3), "🦀");
        f.check("A🦀e\u{301}Z", None, 3..3);
    }
}

#[test]
fn ime_enter_os_delivery_contract_never_submits_marked_draft() {
    let mut f = Fixture::new(false);
    f.marked(Some(3..3), "に", Some(1..1));
    // TestPlatform delivers a GPUI key, not an NSTextInputContext candidate confirmation.
    // A key that reaches the application while still marked must not submit the draft.
    f.key("enter");
    assert!(f.sent.borrow().is_empty());
    assert_eq!(f.value(), "A😀にZ");
    f.commit(None, "日");
    f.key("enter");
    assert_eq!(
        *f.sent.borrow(),
        vec!["A😀日Z".to_string()],
        "normal Enter immediately after commit must work"
    );
    assert_eq!(f.value(), "");
    assert!(!f.window.read(|_, cx| f.composer.read(cx).is_disabled()));
    f.commit(None, "line one");
    f.key("shift-enter");
    assert_eq!(f.value(), "line one\n");
    assert_eq!(f.sent.borrow().len(), 1, "Shift+Enter must not send");
    f.commit(None, "line two");
    f.key("enter");
    assert_eq!(f.sent.borrow()[1], "line one\nline two");
}

#[test]
fn ime_single_line_enter_is_local_only_until_commit() {
    let mut f = Fixture::new(true);
    let entered = Rc::new(RefCell::new(0));
    let count = entered.clone();
    let _subscription = f.window.update(|_, _, cx| {
        cx.subscribe(&f.input, move |_, _, event, _| {
            if matches!(event, gpui_kit::base::input::InputEvent::PressEnter { .. }) {
                *count.borrow_mut() += 1;
            }
        })
    });
    f.marked(Some(3..3), "n", Some(1..1));
    f.key("enter");
    f.check("A😀nZ", Some(3..4), 4..4);
    assert_eq!(*entered.borrow(), 0);
    f.commit(None, "你");
    f.key("enter");
    assert_eq!(*entered.borrow(), 1);
    assert_eq!(
        f.window.read(|_, cx| f.editor.read(cx).value().to_string()),
        SEED
    );
}

#[test]
fn ime_model_search_escape_preserves_popup_and_other_draft() {
    let mut f = Fixture::new(false);
    let position = f.window.update(|_, w, _| w.find("model").bounds().center());
    f.window.simulate_click(position, MouseButton::Left);
    f.window.draw();
    let search = f.window.read(|_, cx| f.composer.read(cx).search_editor());
    f.window.update(|_, w, cx| {
        search.update(cx, |s, cx| {
            assert!(s.focus_handle(cx).is_focused(w));
            s.replace_and_mark_text_in_range(None, "n", Some(1..1), w, cx);
            s.replace_and_mark_text_in_range(None, "ni", Some(2..2), w, cx);
        });
    });
    f.window.draw();
    f.key("escape");
    f.window.read(|_, cx| {
        assert_eq!(f.composer.read(cx).open_selector(), Some("model"));
        assert_eq!(search.read(cx).value().as_ref(), "ni");
        assert_eq!(f.editor.read(cx).value().as_ref(), SEED);
    });
    assert_eq!(*f.escaped.borrow(), 0);
    f.key("escape");
    assert_eq!(
        f.window.read(|_, cx| f.composer.read(cx).open_selector()),
        None
    );
    assert!(f.sent.borrow().is_empty());
}

#[test]
fn ime_escape_os_delivery_contract_does_not_reach_unrelated_layer() {
    for single in [false, true] {
        let mut f = Fixture::new(single);
        f.marked(Some(3..3), "n", Some(1..1));
        f.key("escape");
        assert_eq!(
            *f.escaped.borrow(),
            0,
            "composition Escape must not bubble into a layer dismissal"
        );
        f.check("A😀nZ", None, 4..4);
        f.key("escape");
        assert_eq!(*f.escaped.borrow(), 1, "ordinary Escape still propagates");
    }
}
