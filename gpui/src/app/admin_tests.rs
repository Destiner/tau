use super::*;
use gpui_kit::{
    FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke, Render, TestApp,
    base::{
        Input, InputBase, Root, Textarea,
        input::{InputState, TextareaState},
    },
    div,
    prelude::*,
    px,
};
use std::{cell::RefCell, rc::Rc};

fn key(value: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse(value).unwrap(),
        is_held: false,
        prefer_character_input: false,
    }
}

#[test]
fn matcher_matches_vue_interval_case_overlap_modifiers_repeat_and_utf16() {
    let mut matcher = Matcher::default();
    for k in ["i", "i", "D", "d", "q"] {
        assert!(!matcher.press(&key(k), Duration::ZERO, false));
    }
    let mut repeated = key("D");
    repeated.is_held = true;
    assert!(matcher.press(&repeated, Duration::from_secs(2), false));
    for reset in ["cmd-d", "ctrl-d", "alt-d", "escape", "enter", "😀"] {
        for k in ["i", "d"] {
            assert!(!matcher.press(&key(k), Duration::ZERO, false));
        }
        assert!(!matcher.press(&key(reset), Duration::ZERO, false));
        for k in ["d", "q", "d"] {
            assert!(!matcher.press(&key(k), Duration::ZERO, false));
        }
    }
    for k in ["i", "d", "d", "q"] {
        assert!(!matcher.press(&key(k), Duration::ZERO, false));
    }
    assert!(!matcher.press(&key("d"), Duration::from_millis(2001), false));
    for k in ["i", "d", "d", "q", "d"] {
        assert!(!matcher.press(&key(k), Duration::ZERO, true));
    }
}

struct Harness {
    admin: State,
    focus: FocusHandle,
    input: Entity<InputState>,
    textarea: Entity<TextareaState>,
    context: &'static str,
    multiline: bool,
    commands: Rc<RefCell<Vec<Command>>>,
    identity: Option<String>,
    blocked: bool,
    reject_mode: bool,
}
impl Harness {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut admin = State::default();
        let commands = Rc::new(RefCell::new(vec![]));
        let load = admin.mount(
            false,
            window,
            cx,
            |shell: &mut Self, event, window, cx| match event {
                UiEvent::Submit {
                    description,
                    include_session,
                } => {
                    if let Some(command) = shell.admin.submit(
                        description.clone(),
                        *include_session,
                        shell.identity.clone(),
                    ) {
                        shell.commands.borrow_mut().push(command);
                    }
                }
                UiEvent::Dismissed => {
                    if !shell.blocked {
                        shell.focus.focus(window, cx);
                    }
                }
            },
        );
        commands.borrow_mut().push(load);
        Self {
            admin,
            focus: cx.focus_handle(),
            input: cx.new(|cx| InputState::new(window, cx)),
            textarea: cx.new(|cx| TextareaState::new(window, cx).submit_on_enter(false)),
            context: "ChromeRename",
            multiline: false,
            commands,
            identity: Some("pi-current".into()),
            blocked: false,
            reject_mode: false,
        }
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let editable = self.input.read(cx).focus_handle(cx).is_focused(window)
            || self.textarea.read(cx).focus_handle(cx).is_focused(window);
        if let Some(command) = self.admin.key(event, Duration::ZERO, editable, window) {
            self.commands.borrow_mut().push(command.clone());
            if self.reject_mode {
                self.admin.failed(command);
            }
            self.admin
                .sync(false, self.identity.is_some(), self.blocked, window, cx);
        }
    }
}
impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.admin
            .sync(false, self.identity.is_some(), self.blocked, window, cx);
        div()
            .key_context("TauShell")
            .track_focus(&self.focus)
            .size_full()
            .capture_key_down(cx.listener(Self::key))
            .child(
                InputBase::new("typed-input")
                    .key_context(self.context)
                    .w(px(300.))
                    .child(Input::new(&self.input)),
            )
            .when(self.multiline, |d| {
                d.child(InputBase::new("typed-textarea").child(Textarea::new(&self.textarea)))
            })
            .children(self.admin.view.clone())
    }
}

fn enable(h: &mut Harness, w: &mut Window, cx: &mut Context<Harness>) {
    let command = h.admin.set_mode(true);
    let Command::SetMode { revision, .. } = command else {
        unreachable!()
    };
    h.admin.reply(
        Reply::ModeSet {
            revision,
            enabled: true,
            saved: true,
        },
        w,
        cx,
    );
    h.admin.sync(false, h.identity.is_some(), h.blocked, w, cx);
}

#[test]
fn real_root_idle_unlock_and_base_inputs_are_not_rewritten_or_matched() {
    for context in [
        "NativeRemote",
        "NativePaletteSearch",
        "ChromeRename",
        "NativeComposerSearch",
        "NativeExtension",
    ] {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|w, cx| {
            let h = cx.new(|cx| Harness::new(w, cx));
            Root::new(h, w, cx)
        });
        let h = window.update(|root, _, _| root.view().clone().downcast::<Harness>().unwrap());
        window.update(|_, w, cx| {
            h.update(cx, |h, cx| {
                h.context = context;
                h.input.read(cx).focus_handle(cx).focus(w, cx);
            })
        });
        window.draw();
        for k in ["i", "d", "d", "q", "d"] {
            window.simulate_keystroke(k);
        }
        window.update(|_, w, cx| {
            h.update(cx, |h, cx| {
                assert_eq!(h.input.read(cx).value().as_ref(), "iddqd");
                assert!(!h.admin.enabled());
                assert_eq!(h.commands.borrow().len(), 1);
                assert!(focused_input(w, false));
                h.focus.focus(w, cx);
            })
        });
        window.draw();
        for k in ["i", "d", "d", "q", "d"] {
            window.simulate_keystroke(k);
        }
        window.update(|_, w, cx| {
            h.update(cx, |h, cx| {
                assert!(!h.admin.enabled(), "no fabricated persisted enable");
                assert!(matches!(
                    h.commands.borrow().last(),
                    Some(Command::SetMode { enabled: true, .. })
                ));
                enable(h, w, cx);
                h.multiline = true;
                h.textarea.read(cx).focus_handle(cx).focus(w, cx);
            })
        });
        window.draw();
        for k in ["i", "d", "d", "q", "d"] {
            window.simulate_keystroke(k);
        }
        window.update(|_, _, cx| {
            h.update(cx, |h, cx| {
                assert_eq!(h.textarea.read(cx).value().as_ref(), "iddqd");
                assert!(h.admin.enabled());
                assert_eq!(h.commands.borrow().len(), 2);
            })
        });
    }
}

#[test]
fn worker_boundary_is_send_revisioned_private_and_load_cannot_override_toggle() {
    fn send<T: Send>() {}
    send::<Command>();
    send::<Reply>();
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        let Command::Load { revision } = h.commands.borrow()[0] else {
            unreachable!()
        };
        enable(h, w, cx);
        assert!(!h.admin.reply(
            Reply::Loaded {
                revision,
                enabled: false
            },
            w,
            cx
        ));
        assert!(h.admin.enabled());
        let command = h
            .admin
            .submit(" draft ".into(), false, Some("private-id".into()))
            .unwrap();
        let Command::Submit {
            revision,
            description,
            session_id,
        } = command
        else {
            unreachable!()
        };
        assert_eq!(description, "draft");
        assert_eq!(session_id, None);
        assert!(
            h.admin
                .submit("again".into(), true, Some("other".into()))
                .is_none()
        );
        h.admin.set_mode(false);
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert!(!h.admin.enabled());
        enable(h, w, cx);
        let command = h
            .admin
            .submit("with id".into(), true, Some("captured-now".into()))
            .unwrap();
        assert!(
            matches!(command, Command::Submit { session_id: Some(id), .. } if id == "captured-now")
        );
    });
}

#[test]
fn reporter_captures_identity_on_submit_retains_failure_retry_and_off_draft() {
    use gpui_kit::test::TestWindowExt;
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        h.admin.open_reporter(w, cx);
        let view = h.admin.view.clone().unwrap();
        view.update(cx, |view, cx| {
            view.editor()
                .update(cx, |editor, cx| editor.set_value("keep this draft", w, cx));
        });
        h.identity = Some("pi-after-popup-open".into());
    });
    window.draw();
    window.draw();
    window.simulate_keystroke("cmd-shift-s");
    window.simulate_keystroke("cmd-enter");
    let revision = window.update(|h, w, cx| {
        let Command::Submit {
            revision,
            description,
            session_id,
        } = h.commands.borrow().last().unwrap().clone()
        else {
            panic!("no real submission event")
        };
        assert_eq!(description, "keep this draft");
        assert_eq!(session_id.as_deref(), Some("pi-after-popup-open"));
        h.admin.reply(
            Reply::Submitted {
                revision: revision + 1,
                saved: true,
            },
            w,
            cx,
        );
        revision
    });
    window.simulate_keystroke("cmd-enter");
    window.simulate_keystroke("escape");
    window.update(|h, w, cx| {
        assert_eq!(h.commands.borrow().len(), 2, "busy double guard");
        assert!(h.admin.view.as_ref().unwrap().read(cx).is_open());
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: false,
            },
            w,
            cx,
        );
        let view = h.admin.view.as_ref().unwrap().read(cx);
        assert_eq!(view.editor().read(cx).value().as_ref(), "keep this draft");
    });
    window.draw();
    window.update(|_, w, _| {
        assert!(w.find("report-error").bounds().size.height > px(0.));
    });
    window.simulate_keystroke("cmd-enter");
    let retry = window.update(|h, _, _| {
        let Command::Submit { revision, .. } = h.commands.borrow().last().unwrap().clone() else {
            unreachable!()
        };
        revision
    });
    window.update(|h, w, cx| {
        h.admin.set_mode(false);
        h.admin.sync(false, true, false, w, cx);
        h.admin.reply(
            Reply::Submitted {
                revision: retry,
                saved: true,
            },
            w,
            cx,
        );
        enable(h, w, cx);
        let view = h.admin.view.as_ref().unwrap().read(cx);
        assert!(!view.is_open());
        assert_eq!(view.editor().read(cx).value().as_ref(), "");
    });
    window.update(|h, w, cx| {
        assert!(
            h.focus.is_focused(w),
            "parent restores root focus on dismissal"
        );
        h.admin.open_reporter(w, cx);
        let editor = h.admin.view.as_ref().unwrap().read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("new report", w, cx));
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    window.update(|h, w, cx| {
        let Command::Submit { revision, .. } = h.commands.borrow().last().unwrap().clone() else {
            unreachable!()
        };
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert_eq!(
            h.admin
                .view
                .as_ref()
                .unwrap()
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .as_ref(),
            ""
        );
    });
    window.draw();
    window.update(|_, w, _| {
        assert!(w.find("report-sent").bounds().size.height > px(0.));
    });
}

#[test]
fn reporter_base_editor_newlines_and_both_submit_modifiers() {
    for shortcut in ["ctrl-enter", "cmd-enter"] {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(Harness::new);
        window.update(|h, w, cx| {
            h.identity = None;
            enable(h, w, cx);
            h.admin.open_reporter(w, cx);
        });
        window.draw();
        window.simulate_input("first");
        window.simulate_keystroke("enter");
        window.simulate_input("second");
        window.update(|h, _, cx| {
            assert_eq!(
                h.admin
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref(),
                "first\nsecond"
            );
        });
        window.simulate_keystroke(shortcut);
        window.update(|h, _, _| {
            assert!(matches!(h.commands.borrow().last(), Some(Command::Submit { description, session_id: None, .. }) if description == "first\nsecond"));
        });
        window.simulate_keystroke("escape");
        window.update(|h, _, cx| assert!(h.admin.view.as_ref().unwrap().read(cx).is_open()));
    }
}

#[test]
fn reporter_ime_enter_and_escape_do_not_submit_or_dismiss() {
    use gpui_kit::EntityInputHandler;
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    for shortcut in ["cmd-enter", "escape"] {
        window.update(|h, w, cx| {
            enable(h, w, cx);
            h.admin.open_reporter(w, cx);
            let editor = h.admin.view.as_ref().unwrap().read(cx).editor();
            editor.read(cx).focus_handle(cx).focus(w, cx);
            editor.update(cx, |editor, cx| {
                editor.replace_and_mark_text_in_range(Some(0..0), "composing", Some(0..9), w, cx);
                assert!(editor.marked_text_range(w, cx).is_some());
            });
        });
        window.draw();
        window.simulate_keystroke(shortcut);
        window.update(|h, _, cx| {
            assert!(h.admin.view.as_ref().unwrap().read(cx).is_open());
            assert_eq!(h.commands.borrow().len(), 1, "no submit while composing");
        });
    }
}

#[test]
fn reporter_suspension_preserves_draft_without_stealing_modal_focus() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        h.admin.open_reporter(w, cx);
        h.blocked = true;
        h.input.read(cx).focus_handle(cx).focus(w, cx);
        h.admin.sync(false, true, true, w, cx);
        h.admin.open_reporter(w, cx);
        assert!(h.input.read(cx).focus_handle(cx).is_focused(w));
        assert!(h.admin.view.as_ref().unwrap().read(cx).is_open());
    });
    window.draw();
    window.simulate_keystroke("escape");
    window.update(|h, w, cx| {
        assert!(h.admin.view.as_ref().unwrap().read(cx).is_open());
        h.blocked = false;
        h.admin.sync(false, true, false, w, cx);
        assert!(h.input.read(cx).focus_handle(cx).is_focused(w));
    });
}

#[test]
fn superseded_live_switch_is_reconciled_after_newer_enqueue_failure() {
    let mut state = State::default();
    let Command::SetMode {
        revision: first, ..
    } = state.set_mode(true)
    else {
        unreachable!()
    };
    let rejected = state.set_mode(false);
    assert!(state.failed(rejected));
    assert!(!state.enabled());
    // The earlier queued operation remains authoritative when the newer operation
    // never reached the backend. It must not be mistaken for a rollback.
    assert!(!state.accept(Reply::ModeSet {
        revision: first,
        enabled: true,
        saved: true
    }));
    assert!(state.enabled());
    assert!(state.mode_failed());
    let Command::SetMode { revision: last, .. } = state.set_mode(false) else {
        unreachable!()
    };
    assert!(state.accept(Reply::ModeSet {
        revision: last,
        enabled: false,
        saved: true
    }));
    assert!(!state.accept(Reply::ModeSet {
        revision: first,
        enabled: true,
        saved: true
    }));
    assert!(!state.enabled());
}

#[test]
fn actual_mode_mismatch_is_accepted_by_revision_without_claiming_saved() {
    let mut state = State::default();
    let Command::SetMode { revision, .. } = state.set_mode(true) else {
        unreachable!()
    };
    assert!(state.accept(Reply::ModeSet {
        revision,
        enabled: false,
        saved: false
    }));
    assert!(!state.enabled());
    assert!(state.mode_failed());
    assert!(!state.pending(revision));
}

#[test]
fn persisted_load_applies_only_matching_callback() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        assert!(!h.admin.enabled());
        let Command::Load { revision } = h.commands.borrow()[0] else {
            unreachable!()
        };
        assert!(!h.admin.reply(
            Reply::Loaded {
                revision: revision + 1,
                enabled: true
            },
            w,
            cx
        ));
        assert!(h.admin.reply(
            Reply::Loaded {
                revision,
                enabled: true
            },
            w,
            cx
        ));
        assert!(h.admin.enabled());
        assert!(!h.admin.reply(
            Reply::Loaded {
                revision,
                enabled: false
            },
            w,
            cx
        ));
    });
}

#[test]
fn busy_component_failed_disable_enqueue_still_consumes_real_acknowledgement() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        h.admin.open_reporter(w, cx);
        let editor = h.admin.view.as_ref().unwrap().read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("accepted report", w, cx));
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    let revision = window.update(|h, w, cx| {
        let Command::Submit { revision, .. } = h.commands.borrow().last().unwrap().clone() else {
            panic!("component did not submit");
        };
        assert!(h.admin.view.as_ref().unwrap().read(cx).is_busy());
        h.reject_mode = true;
        h.focus.focus(w, cx);
        revision
    });
    window.draw();
    for k in ["i", "d", "d", "q", "d"] {
        window.simulate_keystroke(k);
    }
    window.update(|h, w, cx| {
        assert!(h.admin.enabled());
        assert!(h.admin.mode_failed());
        assert!(h.admin.pending(revision));
        assert!(h.admin.view.as_ref().unwrap().read(cx).is_busy());
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        let view = h.admin.view.as_ref().unwrap().read(cx);
        assert!(
            !view.is_busy(),
            "Saving must settle even if off was never painted"
        );
        assert_eq!(view.editor().read(cx).value().as_ref(), "");
    });
}

#[test]
fn busy_component_delayed_success_after_disable_reenable_does_not_reopen() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        h.admin.open_reporter(w, cx);
        let editor = h.admin.view.as_ref().unwrap().read(cx).editor();
        editor.update(cx, |editor, cx| {
            editor.set_value("  accepted draft  ", w, cx)
        });
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    window.update(|h, w, cx| {
        let Command::Submit { revision, .. } = h.commands.borrow().last().unwrap().clone() else {
            unreachable!();
        };
        let Command::SetMode { revision: off, .. } = h.admin.set_mode(false) else {
            unreachable!();
        };
        h.admin.accept(Reply::ModeSet {
            revision: off,
            enabled: false,
            saved: true,
        });
        h.admin.sync(false, true, false, w, cx);
        let view = h.admin.view.clone().unwrap();
        assert!(!view.read(cx).is_open());
        assert!(view.read(cx).is_busy());
        assert!(h.admin.pending(revision));
        enable(h, w, cx);
        assert!(h.admin.submit("unsafe retry".into(), false, None).is_none());
        assert!(view.read(cx).is_busy());
        assert!(!view.read(cx).is_open());
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert!(!view.read(cx).is_open());
        assert!(!view.read(cx).is_busy());
        assert_eq!(view.read(cx).editor().read(cx).value().as_ref(), "");
        let editor = view.read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("newer draft", w, cx));
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert_eq!(editor.read(cx).value().as_ref(), "newer draft");
        assert!(!view.read(cx).is_open());
    });
}

#[test]
fn accepted_write_survives_remount_without_clearing_replacement_draft() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        let Command::Submit { revision, .. } =
            h.admin.submit("old report".into(), false, None).unwrap()
        else {
            unreachable!();
        };
        h.admin.mount(false, w, cx, |_: &mut Harness, _, _, _| {});
        let replacement = h.admin.view.clone().unwrap();
        let editor = replacement.read(cx).editor();
        editor.update(cx, |editor, cx| {
            editor.set_value("replacement draft", w, cx)
        });
        assert!(h.admin.pending(revision));
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert!(!h.admin.pending(revision));
        assert_eq!(editor.read(cx).value().as_ref(), "replacement draft");
        assert!(!replacement.read(cx).is_open());
    });
}

#[test]
fn accepted_acknowledgement_does_not_clear_newer_editor_work() {
    let mut app = TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window(Harness::new);
    window.update(|h, w, cx| {
        enable(h, w, cx);
        h.admin.open_reporter(w, cx);
        let editor = h.admin.view.as_ref().unwrap().read(cx).editor();
        editor.update(cx, |editor, cx| editor.set_value("accepted draft", w, cx));
    });
    window.draw();
    window.simulate_keystroke("cmd-enter");
    window.update(|h, w, cx| {
        let Command::Submit { revision, .. } = h.commands.borrow().last().unwrap().clone() else {
            unreachable!();
        };
        let view = h.admin.view.clone().unwrap();
        let editor = view.read(cx).editor();
        // A programmatic draft replacement is still newer work, even while readonly.
        editor.update(cx, |editor, cx| editor.set_value("newer draft", w, cx));
        h.admin.reply(
            Reply::Submitted {
                revision,
                saved: true,
            },
            w,
            cx,
        );
        assert!(!view.read(cx).is_busy());
        assert_eq!(editor.read(cx).value().as_ref(), "newer draft");
    });
}
