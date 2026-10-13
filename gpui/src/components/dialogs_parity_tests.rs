use super::*;
use gpui_kit::{
    Bounds, EntityInputHandler, MouseButton, Pixels, TestApp, TestAppWindow, base::Root, point,
    size, test::TestWindowExt,
};
use std::cell::RefCell;

fn stripe_matches(bounds: Bounds<gpui_kit::ScaledPixels>, row: Bounds<Pixels>, scale: f32) -> bool {
    (bounds.size.width.0 - px(2.).scale(scale).0).abs() < px(0.5).scale(scale).0
        && (bounds.left().0 - row.left().scale(scale).0).abs() < px(1.).scale(scale).0
}

#[test]
fn stripe_pixel_conversion_preserves_logical_tolerances() {
    let row = Bounds::new(point(px(20.), px(30.)), size(px(200.), px(40.)));
    for scale in [1., 1.5, 2.] {
        for (width, offset, expected) in [
            (2., 0., true),
            (2.49, 0.99, true),
            (2.5, 0., false),
            (2., 1., false),
        ] {
            let stripe = Bounds::new(
                point(row.left() + px(offset), row.top()),
                size(px(width), row.size.height),
            );
            assert_eq!(stripe_matches(stripe.scale(scale), row, scale), expected);
        }
        assert!(row.scale(scale).contains(&row.center().scale(scale)));
    }
}

struct Host {
    dialog: Entity<NativeDialog>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(false).flex().flex_col().child(
            div()
                .w_full()
                .max_w(px(880.))
                .px(px(22.))
                .pt(px(20.))
                .child(self.dialog.clone()),
        )
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    dialog: Entity<NativeDialog>,
    events: Rc<RefCell<Vec<DialogEvent>>>,
    _subscription: Subscription,
    _app: TestApp,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.window.update(|_, w, cx| w.blur(cx));
            self._app.run_until_parked();
        }
    }
}
impl Fixture {
    fn new(kind: DialogKind, count: usize, dark: bool, confirm: bool) -> Self {
        Self::with_sources(
            kind,
            count,
            dark,
            confirm,
            "Question **with emphasis**",
            "Optional `message`",
        )
    }
    fn new_editor(dark: bool) -> Self {
        Self::with_mode(
            DialogKind::ExtensionInput,
            0,
            dark,
            false,
            "Question **with emphasis**",
            "Optional `message`",
            true,
        )
    }
    fn with_sources(
        kind: DialogKind,
        count: usize,
        dark: bool,
        confirm: bool,
        title: &str,
        message: &str,
    ) -> Self {
        Self::with_mode(kind, count, dark, confirm, title, message, false)
    }
    fn with_mode(
        kind: DialogKind,
        count: usize,
        dark: bool,
        confirm: bool,
        title: &str,
        message: &str,
        editor_mode: bool,
    ) -> Self {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let mut dialog = None;
        let mut window = app.open_window(|w, cx| {
            let entity = cx.new(|cx| {
                let p = DialogProps {
                    kind,
                    title: title.into(),
                    description: message.into(),
                    placeholder: "Reply".into(),
                    options: (0..count)
                        .map(|i| DialogOption {
                            id: format!("option-{i}"),
                            label: format!("Choice {i}"),
                        })
                        .collect(),
                };
                let d = if editor_mode {
                    NativeDialog::new_editor(p, dark, w, cx)
                } else {
                    NativeDialog::new(p, dark, w, cx)
                }
                .embedded()
                .keyboard_submission();
                if confirm { d.confirm_shortcuts() } else { d }
            });
            dialog = Some(entity.clone());
            let host = cx.new(|_| Host { dialog: entity });
            Root::new(host, w, cx)
        });
        let dialog = dialog.unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let subscription = window.update(|_, _, cx| {
            dialog.update(cx, |_, cx| {
                cx.subscribe(&cx.entity(), move |_, _, e: &DialogEvent, _| {
                    sink.borrow_mut().push(e.clone())
                })
            })
        });
        window.simulate_resize(size(px(900.), px(700.)));
        window.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        for _ in 0..3 {
            window.draw();
        }
        Self {
            window,
            dialog,
            events,
            _subscription: subscription,
            _app: app,
        }
    }
    fn bounds(&mut self, id: &str) -> Bounds<Pixels> {
        self.window.update(|_, w, _| w.find(id.to_owned()).bounds())
    }
    fn click(&mut self, id: &str) {
        let at = self.bounds(id).center();
        self.window.simulate_click(at, MouseButton::Left);
        self.window.draw();
    }
    fn busy(&mut self, busy: bool) {
        self.window
            .update(|_, _, cx| self.dialog.update(cx, |s, cx| s.set_busy(busy, cx)));
        self.window.draw();
    }
    fn value(&self) -> String {
        self.window.read(|_, cx| self.dialog.read(cx).value(cx))
    }
}
fn mod_key(suffix: &str) -> String {
    format!(
        "{}-{suffix}",
        if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        }
    )
}

#[test]
fn root_zero_and_one_option_paint_and_activation() {
    for dark in [false, true] {
        let mut empty = Fixture::new(DialogKind::ExtensionSelect, 0, dark, false);
        assert!(empty.bounds("empty-options").size.height >= px(20.));
        empty.window.simulate_keystroke("enter");
        assert!(empty.events.borrow().is_empty());
        empty.click("cancel");
        assert_eq!(*empty.events.borrow(), vec![DialogEvent::Cancelled]);

        let mut one = Fixture::new(DialogKind::ExtensionSelect, 1, dark, false);
        let row = one.bounds("option-0");
        assert!((row.size.height - px(26.)).abs() < px(2.));
        assert_eq!(one.bounds("cancel").size.height, px(22.));
        one.click("option-0");
        one.click("option-0");
        assert_eq!(
            *one.events.borrow(),
            vec![DialogEvent::Submitted("Choice 0".into())]
        );
        assert!(one.window.read(|_, cx| one.dialog.read(cx).is_busy()));
    }
}

#[test]
fn root_hover_cursor_paints_only_stripe_and_busy_freezes_admission() {
    for dark in [false, true] {
        let mut f = Fixture::new(DialogKind::ExtensionSelect, 3, dark, false);
        let at = f.bounds("option-1").center();
        f.window.simulate_mouse_move(at);
        f.window.draw();
        f.window
            .read(|_, cx| assert_eq!(f.dialog.read(cx).cursor, 1));
        let row = f.bounds("option-1");
        f.window.update(|_, w, _| {
            let t = tokens(dark);
            let scale = w.scale_factor();
            let scaled_row = row.scale(scale);
            assert!(!w.painted_quads().iter().any(|q| q.background
                == rgba(if dark { 0x47526633 } else { 0x6b7d8f1a }).into()
                && q.bounds.contains(&scaled_row.center())));
            assert!(
                w.painted_quads()
                    .iter()
                    .any(|q| q.background == rgb(t.accent).into()
                        && stripe_matches(q.bounds, row, scale))
            );
        });
        f.busy(true);
        let other = f.bounds("option-2").center();
        f.window.simulate_mouse_move(other);
        f.window.simulate_click(other, MouseButton::Left);
        f.click("cancel");
        f.window.simulate_keystroke("down");
        f.window.simulate_keystroke("escape");
        f.window
            .read(|_, cx| assert_eq!(f.dialog.read(cx).cursor, 1));
        assert!(f.events.borrow().is_empty());
        f.window.activate_accessibility();
        f.window.draw();
        let tree = f.window.accessibility_tree().unwrap();
        for label in ["Choice 0", "Choice 1", "Choice 2", "Cancel"] {
            let node = tree
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label))
                .unwrap();
            assert!(
                node.1.is_disabled(),
                "{label} must expose actual disabled semantics"
            );
        }
    }
}

#[test]
fn standalone_many_options_wrap_navigation_and_overflow_leave_controls_reachable() {
    let mut f = Fixture::new(DialogKind::ExtensionSelect, 100, false, false);
    f.window
        .update(|_, _, cx| f.dialog.update(cx, |d, _| d.embedded = false));
    f.window.simulate_resize(size(px(540.), px(480.)));
    f.window.update(|_, w, cx| w.bounds_changed(cx));
    f.window.draw();
    let content = f.bounds("extension-content");
    assert!(content.size.height <= px(240.));
    let cancel = f.bounds("cancel");
    assert!(cancel.top() >= content.bottom());
    assert!(cancel.bottom() < px(480.));
    f.window.simulate_keystroke("up");
    for _ in 0..4 {
        f.window.draw();
    }
    f.window
        .read(|_, cx| assert_eq!(f.dialog.read(cx).cursor, 99));
    let last = f.bounds("option-99");
    let viewport = f.bounds("extension-content");
    assert!(last.top() >= viewport.top() - px(1.));
    assert!(last.bottom() <= viewport.bottom() + px(1.));
    f.window.simulate_keystroke("down");
    for _ in 0..4 {
        f.window.draw();
    }
    f.window
        .read(|_, cx| assert_eq!(f.dialog.read(cx).cursor, 0));
    f.window.simulate_keystroke("enter");
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("Choice 0".into())]
    );
}

#[test]
fn embedded_options_are_natural_height_and_only_keyboard_navigation_requests_parent_reveal() {
    for dark in [false, true] {
        let mut f = Fixture::new(DialogKind::ExtensionSelect, 30, dark, false);
        f.window.simulate_resize(size(px(720.), px(480.)));
        f.window.update(|_, w, cx| w.bounds_changed(cx));
        f.window.draw();
        let title = f.bounds("extension-title");
        let message = f.bounds("extension-message");
        let line = f.window.update(|_, w, _| w.pixel_snap(px(13. * 1.62)));
        assert_eq!(title.size.height, line);
        assert_eq!(message.size.height, line);
        let options = f.bounds("extension-select-options");
        let mut total = px(0.);
        for i in 0..30 {
            total += f.bounds(&format!("option-{i}")).size.height;
        }
        assert_eq!(options.size.height, total);
        assert_eq!(
            f.bounds("extension-content").size.height,
            line * 2. + px(16.) + total
        );
        assert!(
            f.bounds("cancel").top() > px(480.),
            "do not fake fitting Cancel inside the viewport"
        );
        f.window
            .read(|_, cx| assert_eq!(f.dialog.read(cx).content_scroll.max_offset().y, px(0.)));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let sink = requests.clone();
        f.window.update(|_, _, cx| {
            f.dialog.update(cx, |d, _| {
                d.set_option_reveal_handler(move |bounds, _, _| sink.borrow_mut().push(bounds));
            })
        });
        let at = f.bounds("option-1").center();
        f.window.simulate_mouse_move(at);
        f.window.draw();
        assert!(
            requests.borrow().is_empty(),
            "hover must not move the transcript"
        );
        f.window.simulate_keystroke("up");
        f.window.draw();
        assert_eq!(&*requests.borrow(), &[f.bounds("option-0")]);
        f.window.simulate_keystroke("escape");
        assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
    }
}

#[test]
fn persistent_transcript_large_prompt_routes_real_wheel_and_keeps_cancel_reachable() {
    use crate::components::transcript::Transcript;
    for (dark, keyboard_cancel) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let transcript = app.new_entity(|cx| Transcript::new("natural-prompt", dark, cx));
        let mut prompt = None;
        let mut window = app.open_window(|w, cx| {
            let dialog = cx.new(|cx| {
                NativeDialog::new(
                    DialogProps {
                        kind: DialogKind::ExtensionSelect,
                        title: "Choose a target".into(),
                        description: "Select carefully".into(),
                        placeholder: String::new(),
                        options: (0..30)
                            .map(|i| DialogOption {
                                id: format!("natural-option-{i}"),
                                label: format!("Target {i}"),
                            })
                            .collect(),
                    },
                    dark,
                    w,
                    cx,
                )
                .embedded()
                .keyboard_submission()
            });
            prompt = Some(dialog.clone());
            transcript.update(cx, |t, cx| t.set_extension_prompt(Some(dialog), cx));
            Root::new(transcript.clone(), w, cx).flex().flex_col()
        });
        let prompt = prompt.unwrap();
        window.simulate_resize(size(px(720.), px(480.)));
        window.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        for _ in 0..8 {
            app.run_until_parked();
            window.draw();
        }
        let viewport = window.update(|_, w, _| w.find("natural-prompt").bounds());
        window.update(|_, w, cx| {
            prompt.update(cx, |d, cx| d.focus(w, cx));
        });
        window.simulate_keystroke("down");
        for _ in 0..5 {
            app.run_until_parked();
            window.draw();
        }
        let second = window.update(|_, w, _| w.find("natural-option-1").bounds());
        assert!(viewport.contains(&second.origin));
        assert!(second.bottom() <= viewport.bottom());
        window.simulate_keystroke("up");
        window.simulate_keystroke("up");
        for _ in 0..5 {
            app.run_until_parked();
            window.draw();
        }
        let last = window.update(|_, w, _| w.find("natural-option-29").bounds());
        assert!(viewport.contains(&last.origin));
        assert!(last.bottom() <= viewport.bottom());
        window.simulate_keystroke("tab");
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            assert!(prompt.read(cx).cancel_focus.is_focused(w));
        });
        if keyboard_cancel {
            window.simulate_keystroke("enter");
            window.simulate_event(gpui_kit::KeyUpEvent {
                keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
            });
            window.read(|_, cx| {
                assert_eq!(
                    prompt.read(cx).accepted_event(),
                    Some(&DialogEvent::Cancelled)
                );
            });
            window.update(|_, w, cx| w.blur(cx));
            app.run_until_parked();
            continue;
        }
        // Wheel back to the tail also makes the pointer action reachable.
        let last = window.update(|_, w, _| w.find("natural-option-29").bounds());
        window.simulate_scroll(last.center(), point(px(0.), px(-10000.)));
        for _ in 0..5 {
            app.run_until_parked();
            window.draw();
        }
        let cancel = window.update(|_, w, _| w.find("cancel").bounds());
        assert!(
            viewport.contains(&cancel.center()),
            "tail-following must expose Cancel: viewport={viewport:?}, cancel={cancel:?}, last={last:?}"
        );
        window.simulate_scroll(cancel.center(), point(px(0.), px(10000.)));
        for _ in 0..5 {
            app.run_until_parked();
            window.draw();
        }
        let cancel_above = window.update(|_, w, _| w.find("cancel").bounds());
        assert!(cancel_above.top() > viewport.bottom());
        let title = window.update(|_, w, _| w.find("extension-title").bounds());
        assert!(viewport.contains(&title.center()));
        window.simulate_scroll(title.center(), point(px(0.), px(-10000.)));
        for _ in 0..5 {
            app.run_until_parked();
            window.draw();
        }
        let cancel = window.update(|_, w, _| w.find("cancel").bounds());
        assert!(viewport.contains(&cancel.center()));
        window.simulate_click(cancel.center(), MouseButton::Left);
        window.read(|_, cx| {
            assert_eq!(
                prompt.read(cx).accepted_event(),
                Some(&DialogEvent::Cancelled)
            )
        });
        window.update(|_, w, cx| {
            transcript.update(cx, |t, cx| t.set_extension_prompt(None, cx));
            assert!(prompt.read(cx).reveal_option.is_none());
            w.blur(cx);
        });
        app.run_until_parked();
    }
}

#[test]
fn root_confirm_has_accurate_actions_not_synthetic_yes_no_options() {
    for (id, expected) in [("confirm", "Yes"), ("reject", "No")] {
        let mut f = Fixture::new(DialogKind::ExtensionSelect, 2, false, true);
        f.window
            .update(|_, w, _| assert!(w.try_find("option-0").is_none()));
        assert_eq!(f.bounds(id).size.height, px(22.));
        f.click(id);
        f.click("cancel");
        assert_eq!(
            *f.events.borrow(),
            vec![DialogEvent::Submitted(expected.into())]
        );
    }
    let mut f = Fixture::new(DialogKind::ExtensionSelect, 2, false, true);
    f.window.simulate_keystroke(&mod_key("shift-enter"));
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("No".into())]
    );
}

#[test]
fn root_markdown_is_painted_by_shared_source_owned_presentation() {
    let mut f = Fixture::with_sources(
        DialogKind::ExtensionInput,
        0,
        false,
        false,
        "**Question**\n\n```txt\nformatted\n```",
        "Optional `message`",
    );
    for _ in 0..3 {
        f.window.draw();
    }
    f.window
        .read(|_, cx| assert_eq!(f.dialog.read(cx).title.read(cx).rendered_code_blocks(), 1));
    assert!(f.bounds("extension-title").size.height > px(40.));
    let message = f.bounds("extension-message");
    let snapped_line = f.window.update(|_, w, _| w.pixel_snap(px(13. * 1.62)));
    assert_eq!(message.size.height, snapped_line);
    f.window.update(|_, _, cx| {
        f.dialog.update(cx, |s, cx| {
            s.props.description.clear();
            cx.notify();
        })
    });
    f.window.draw();
    f.window
        .update(|_, w, _| assert!(w.try_find("extension-message").is_none()));
}

#[test]
fn standalone_long_option_and_prose_do_not_escape_column_or_hide_actions() {
    let mut f = Fixture::with_sources(
        DialogKind::ExtensionSelect,
        1,
        false,
        false,
        &"Long explanatory question.\n\n".repeat(150),
        "Optional message",
    );
    f.window.update(|_, _, cx| {
        f.dialog.update(cx, |s, cx| {
            s.embedded = false;
            s.props.options[0].label = format!(
                "First line\n{}\nLast line",
                "long-word-without-spaces".repeat(25)
            );
            cx.notify();
        })
    });
    f.window.simulate_resize(size(px(540.), px(480.)));
    f.window.update(|_, w, cx| w.bounds_changed(cx));
    for _ in 0..3 {
        f.window.draw();
    }
    let row = f.bounds("option-0");
    let prompt = f.bounds("extension");
    assert!(row.size.height > px(26.));
    assert!(row.right() <= prompt.right());
    assert!(f.bounds("cancel").bottom() < px(480.));
    let at = f.bounds("extension-content").center();
    f.window.simulate_scroll(at, point(px(0.), px(-300.)));
    f.window.draw();
    f.window
        .read(|_, cx| assert!(f.dialog.read(cx).content_scroll.offset().y < px(0.)));
}

#[test]
fn root_multiline_enter_secondary_enter_and_retry_retain_original_draft_and_entity() {
    let mut f = Fixture::new_editor(false);
    let editor = f.window.read(|_, cx| f.dialog.read(cx).multiline_editor());
    assert_eq!(f.bounds("multiline").size.height, px(120.));
    f.click("multiline");
    f.window.simulate_input("first");
    f.window.simulate_keystroke("enter");
    f.window.simulate_input("second");
    assert_eq!(f.value(), "first\nsecond");
    assert!(f.events.borrow().is_empty());
    let selection = f.window.read(|_, cx| editor.read(cx).selected_range());
    f.window.simulate_keystroke(&mod_key("enter"));
    f.window.read(|_, cx| {
        let s = f.dialog.read(cx);
        assert!(s.is_open() && s.is_busy());
        assert_eq!(s.multiline_editor(), editor);
        assert_eq!(editor.read(cx).selected_range(), selection);
        assert_eq!(
            s.accepted_event(),
            Some(&DialogEvent::Submitted("first\nsecond".into()))
        );
    });
    f.window.simulate_input("blocked");
    f.window.simulate_keystroke(&mod_key("enter"));
    f.click("submit");
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("first\nsecond".into())]
    );
    assert_eq!(
        f.value(),
        "first\nsecond",
        "secondary Enter must not insert a newline"
    );
    f.window.update(|_, w, cx| {
        f.dialog.update(cx, |s, cx| {
            s.set_error("Could not send the response. Try again.", cx);
            s.set_busy(false, cx);
            s.set_theme(true, w, cx);
            s.focus(w, cx);
        })
    });
    f.window.draw();
    let error = f.bounds("dialog-error");
    assert!(error.top() >= f.bounds("multiline").bottom());
    assert!(error.bottom() <= f.bounds("submit").top());
    f.window.read(|_, cx| {
        let s = f.dialog.read(cx);
        assert_eq!(s.multiline_editor(), editor);
        assert_eq!(s.accepted_response(), Some("first\nsecond"));
    });
    f.window.simulate_input("!");
    assert_eq!(f.value(), "first\nsecond!");
    f.window.read(|_, cx| {
        assert_eq!(f.dialog.read(cx).accepted_response(), Some("first\nsecond"));
    });
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("first\nsecond".into())],
        "retry editing must not mutate the submitted copy"
    );
    f.window.simulate_keystroke(&mod_key("z"));
    assert_eq!(
        f.value(),
        "first\nsecond",
        "retry and retheme preserve native undo"
    );
}

#[test]
fn root_local_editor_callback_can_update_retained_draft_without_transport_busy() {
    let mut f = Fixture::new_editor(false);
    f.window.update(|_, _, cx| {
        f.dialog.update(cx, |s, _| s.keyboard_submission = false);
    });
    f.click("multiline");
    f.window.simulate_input("local\ndraft");
    let editor = f.window.read(|_, cx| f.dialog.read(cx).multiline_editor());
    f.click("submit");
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("local\ndraft".into())]
    );
    f.window.update(|_, w, cx| {
        f.dialog.update(cx, |s, cx| {
            assert!(!s.is_busy());
            s.set_error("Try again.", cx);
            s.focus(w, cx);
        });
    });
    f.window.draw();
    f.window.simulate_input("!");
    assert_eq!(f.value(), "local\ndraft!");
    f.window.simulate_keystroke(&mod_key("z"));
    assert_eq!(f.value(), "local\ndraft");
    f.window.read(|_, cx| {
        assert_eq!(f.dialog.read(cx).multiline_editor(), editor);
        assert_eq!(f.dialog.read(cx).accepted_response(), Some("local\ndraft"));
    });
}

#[test]
fn root_multiline_ime_escape_and_secondary_enter_are_not_prompt_responses() {
    let mut f = Fixture::new_editor(false);
    f.click("multiline");
    f.window.update(|_, w, cx| {
        f.dialog.read(cx).multiline_editor().update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(None, "稍后", Some(2..2), w, cx);
        })
    });
    f.window.draw();
    f.window.simulate_keystroke(&mod_key("enter"));
    assert!(f.events.borrow().is_empty());
    f.window.simulate_keystroke("escape");
    assert!(f.events.borrow().is_empty());
    assert!(f.window.read(|_, cx| f.dialog.read(cx).is_open()));
    f.window.update(|_, w, cx| {
        f.dialog.read(cx).multiline_editor().update(cx, |e, cx| {
            e.replace_text_in_range(Some(0..2), "稍后", w, cx);
        })
    });
    f.window.draw();
    f.window.simulate_keystroke(&mod_key("enter"));
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("稍后".into())]
    );
}

#[test]
fn root_multiline_context_editing_tab_and_busy_disabled_semantics() {
    let mut f = Fixture::new_editor(false);
    f.click("multiline");
    f.window.simulate_input("context\ndraft");
    f.window.simulate_keystroke(&mod_key("a"));
    let selection = f
        .window
        .read(|_, cx| f.dialog.read(cx).multiline.read(cx).selected_range());
    // Native right-click preserves a selection only when the press is inside it.
    let at = f.window.read(|_, cx| {
        f.dialog
            .read(cx)
            .multiline
            .read(cx)
            .range_to_bounds(&(selection.start..selection.start + 1))
            .expect("selected text must be laid out")
            .center()
    });
    f.window.simulate_click(at, MouseButton::Right);
    f.window.draw();
    f.window.read(|_, cx| {
        assert!(f.dialog.read(cx).context_menu.read(cx).is_open());
        assert_eq!(
            f.dialog.read(cx).multiline.read(cx).selected_range(),
            selection
        );
    });
    f.click("dialog-cut");
    assert_eq!(f.value(), "");
    f.click("multiline");
    f.window.simulate_keystroke(&mod_key("z"));
    assert_eq!(f.value(), "context\ndraft");
    f.window.simulate_keystroke("tab");
    f.window.update(|_, w, cx| {
        assert!(
            !f.dialog
                .read(cx)
                .multiline
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        )
    });
    assert_eq!(f.value(), "context\ndraft", "Tab traverses, not indents");
    f.busy(true);
    f.window.activate_accessibility();
    f.window.draw();
    let tree = f.window.accessibility_tree().unwrap();
    for (_, n) in &tree.nodes {
        if n.label() == Some("Submit")
            || n.label() == Some("Cancel")
            || n.role() == gpui_kit::accesskit::Role::MultilineTextInput
        {
            assert!(n.is_disabled());
        }
    }
    f.click("submit");
    f.click("cancel");
    assert!(f.events.borrow().is_empty());
    assert_eq!(f.value(), "context\ndraft");
}

#[test]
fn root_input_tab_secondary_enter_and_failed_send_keep_retained_editing_state() {
    let mut f = Fixture::new(DialogKind::ExtensionInput, 0, false, false);
    let editor = f.window.read(|_, cx| f.dialog.read(cx).editor());
    f.click("input");
    f.window.simulate_input("draft café");
    f.window.simulate_keystroke("tab");
    assert_eq!(f.value(), "draft café");
    f.window
        .update(|_, w, cx| assert!(!editor.read(cx).focus_handle(cx).is_focused(w)));
    f.window.simulate_keystroke("shift-tab");
    f.window
        .update(|_, w, cx| assert!(editor.read(cx).focus_handle(cx).is_focused(w)));
    let selection = f.window.read(|_, cx| editor.read(cx).selected_range());
    f.window.simulate_keystroke(&mod_key("enter"));
    f.window.read(|_, cx| {
        let s = f.dialog.read(cx);
        assert!(s.is_open() && s.is_busy());
        assert_eq!(s.editor(), editor);
        assert_eq!(editor.read(cx).selected_range(), selection);
        assert_eq!(s.accepted_response(), Some("draft café"));
    });
    f.window.simulate_input("blocked");
    f.window.simulate_keystroke(&mod_key("enter"));
    f.click("submit");
    assert_eq!(f.value(), "draft café");
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("draft café".into())]
    );
    f.window.update(|_, w, cx| {
        f.dialog.update(cx, |s, cx| {
            s.set_error("The extension response could not be sent. Try again.", cx);
            s.set_busy(false, cx);
            s.set_theme(true, w, cx);
            s.focus(w, cx);
        })
    });
    f.window.draw();
    assert_eq!(f.window.read(|_, cx| f.dialog.read(cx).editor()), editor);
    assert_eq!(f.value(), "draft café");
    f.window.simulate_input("!");
    assert_eq!(f.value(), "draft café!");
    f.window.read(|_, cx| {
        assert_eq!(f.dialog.read(cx).accepted_response(), Some("draft café"));
    });
    assert_eq!(
        *f.events.borrow(),
        vec![DialogEvent::Submitted("draft café".into())],
        "retry editing must not mutate the submitted copy"
    );
    f.window.simulate_keystroke(&mod_key("z"));
    assert_eq!(f.value(), "draft café");
    let error = f.bounds("dialog-error");
    assert!(error.top() >= f.bounds("input").bottom());
    assert!(error.bottom() <= f.bounds("submit").top());
}

#[test]
fn root_cancel_is_a_retained_pending_response_until_transport_acknowledges() {
    let mut f = Fixture::new_editor(false);
    f.click("multiline");
    f.window.simulate_input("unsent\nanswer");
    f.window.simulate_keystroke("escape");
    f.window.simulate_keystroke("escape");
    assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
    f.window.read(|_, cx| {
        let s = f.dialog.read(cx);
        assert!(s.is_open() && s.is_busy());
        assert_eq!(s.accepted_event(), Some(&DialogEvent::Cancelled));
    });
    assert_eq!(f.value(), "unsent\nanswer");
    f.window.update(|_, w, cx| {
        f.dialog.update(cx, |s, cx| {
            s.set_error("The extension response could not be sent. Try again.", cx);
            s.set_busy(false, cx);
            s.focus(w, cx);
        })
    });
    f.window.draw();
    assert!(f.bounds("dialog-error").size.height > px(0.));
    assert_eq!(f.value(), "unsent\nanswer");
    f.click("submit");
    assert_eq!(
        f.events.borrow().last(),
        Some(&DialogEvent::Submitted("unsent\nanswer".into()))
    );
}

#[test]
fn root_feedback_action_and_dismissal_admission_are_incident_owned() {
    let mut f = Fixture::new(DialogKind::Feedback, 0, false, false);
    f.window.update(|_, _, cx| {
        f.dialog.update(cx, |s, cx| {
            s.set_action_label("Reload Tau", cx);
            s.set_dismissible(false, cx);
        })
    });
    f.window.draw();
    f.window
        .update(|_, w, _| assert!(w.try_find("close").is_none()));
    f.window.simulate_keystroke("escape");
    assert!(f.events.borrow().is_empty());
    f.click("reconnect");
    f.click("reconnect");
    assert_eq!(f.events.borrow().len(), 1);
    let mut passive = Fixture::new(DialogKind::Feedback, 0, false, false);
    passive.window.update(|_, _, cx| {
        passive
            .dialog
            .update(cx, |s, cx| s.set_action_label("", cx))
    });
    passive.window.draw();
    passive
        .window
        .update(|_, w, _| assert!(w.try_find("reconnect").is_none()));
    passive.click("close");
    assert_eq!(*passive.events.borrow(), vec![DialogEvent::Cancelled]);
}

#[test]
fn root_quit_busy_controls_local_error_style_and_guarded_retry() {
    for dark in [false, true] {
        let mut f = Fixture::new(DialogKind::Quit, 0, dark, false);
        assert_eq!(f.bounds("dialog").top(), px(112.));
        assert_eq!(f.bounds("dialog").size.width, px(348.));
        assert_eq!(f.bounds("confirm").size.height, px(28.));
        f.busy(true);
        f.window.activate_accessibility();
        f.window.draw();
        let tree = f.window.accessibility_tree().unwrap();
        for label in ["Quit", "Cancel"] {
            assert!(
                tree.nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(label))
                    .unwrap()
                    .1
                    .is_disabled()
            );
        }
        f.click("confirm");
        f.click("cancel");
        f.window.simulate_keystroke("escape");
        assert!(f.events.borrow().is_empty());
        f.window.update(|_, _, cx| {
            f.dialog.update(cx, |s, cx| {
                s.set_error("Could not save drafts. Try quitting again.", cx);
                s.set_busy(false, cx);
            })
        });
        f.window.draw();
        let error = f.bounds("dialog-error");
        let panel = f.bounds("dialog");
        assert!(panel.contains(&error.center()));
        assert!(error.bottom() < f.bounds("confirm").top());
        f.window.update(|_, w, _| {
            assert!(w.painted_quads().iter().any(|q| {
                q.background == rgba((tokens(dark).panel_raised << 8) | 0xf5).into()
                    && q.bounds.contains(&error.center().scale(w.scale_factor()))
            }))
        });
        f.click("confirm");
        f.click("confirm");
        assert_eq!(
            f.events.borrow().len(),
            1,
            "component emits intent once; parent still owns authorization"
        );
    }
}
