//! Real Root/component fallback-delivery contracts, not AppKit candidate-window proof.
use crate::components::{
    composer::{Composer, ComposerOptions},
    dialogs::{DialogEvent, DialogKind, DialogProps, NativeDialog},
    models::{Event as ModelEvent, ModelOption, ModelSelector},
    palette::{Palette, PaletteEvent, PaletteItem, PalettePage},
};
use gpui_kit::{
    AppContext, Context, Entity, EntityInputHandler, Focusable, IntoElement, MouseButton, Render,
    Subscription, TestApp, TestAppWindow, TestSupportExt, Window,
    base::{
        Root,
        input::{InputEvent, InputState},
    },
    div, point,
    prelude::*,
    px, size,
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc};

const DRAFT: &str = "Typed composer café 😀";
#[derive(Clone, Copy, Debug)]
enum Kind {
    Model,
    Palette,
    Ssh,
    ExtensionInput,
}
#[derive(Clone)]
enum Control {
    Model(Entity<ModelSelector>),
    Palette(Entity<Palette>),
    Dialog(Entity<NativeDialog>),
}
impl Control {
    fn editor(&self, cx: &gpui_kit::App) -> Entity<InputState> {
        match self {
            Self::Model(e) => e.read(cx).search_editor(),
            Self::Palette(e) => e.read(cx).search_editor(),
            Self::Dialog(e) => e.read(cx).editor(),
        }
    }
    fn open(&self, cx: &gpui_kit::App) -> bool {
        match self {
            Self::Model(e) => e.read(cx).is_open(),
            Self::Palette(e) => e.read(cx).is_open(),
            Self::Dialog(e) => e.read(cx).is_open(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observed {
    Model(ModelEvent),
    Palette(PaletteEvent),
    Dialog(DialogEvent),
}
struct Host {
    composer: Entity<Composer>,
    control: Option<Control>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let page = div().size_full().flex().flex_col().p(px(20.)).child(
            div()
                .id("composer-context")
                .test_support()
                .w_full()
                .child(self.composer.clone()),
        );
        match &self.control {
            Some(Control::Model(e)) => page.child(e.clone()),
            Some(Control::Palette(e)) => page.child(e.clone()),
            Some(Control::Dialog(e)) => page.child(e.clone()),
            None => page,
        }
    }
}
struct Fixture {
    window: TestAppWindow<Root>,
    host: Entity<Host>,
    control: Control,
    input: Entity<InputState>,
    events: Rc<RefCell<Vec<Observed>>>,
    enters: Rc<RefCell<usize>>,
    _subscriptions: Vec<Subscription>,
    app: TestApp,
}
impl Fixture {
    fn new(kind: Kind) -> Self {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut host = None;
        let mut window = app.open_window(|w, cx| {
            let composer = cx.new(|cx| {
                Composer::new(
                    ComposerOptions {
                        models: vec![],
                        model_heading: "Models".into(),
                        efforts: vec![],
                        model: String::new(),
                        effort: String::new(),
                        draft: String::new(),
                        dark: false,
                        disabled: false,
                        streaming: false,
                    },
                    w,
                    cx,
                )
            });
            let view = cx.new(|_| Host {
                composer,
                control: None,
            });
            host = Some(view.clone());
            Root::new(view, w, cx)
        });
        let host = host.unwrap();
        window.simulate_resize(size(px(900.), px(700.)));
        window.update(|_, w, cx| w.bounds_changed(cx));
        window.draw();
        // A genuine editor click and typed draft before opening another input.
        let position = window.update(|_, w, _| {
            let bounds = w.find("composer-context").bounds();
            point(bounds.left() + px(15.), bounds.top() + px(15.))
        });
        window.simulate_click(position, MouseButton::Left);
        window.simulate_input(DRAFT);
        let events = Rc::new(RefCell::new(vec![]));
        let enters = Rc::new(RefCell::new(0));
        let (control, input, subscriptions) = window.update(|_, w, cx| {
            let mut subscriptions = vec![];
            let control = match kind {
                Kind::Model => {
                    let e = cx.new(|cx| {
                        ModelSelector::new(
                            ["alpha", "beta"]
                                .into_iter()
                                .map(|id| ModelOption {
                                    id: id.into(),
                                    label: id.into(),
                                    group: "Models".into(),
                                    element_id: format!("row-{id}"),
                                })
                                .collect(),
                            "beta",
                            false,
                            w,
                            cx,
                        )
                    });
                    let received = events.clone();
                    subscriptions.push(cx.subscribe(&e, move |_, _, event: &ModelEvent, _| {
                        received.borrow_mut().push(Observed::Model(event.clone()));
                    }));
                    Control::Model(e)
                }
                Kind::Palette => {
                    let e = cx.new(|cx| {
                        Palette::new(
                            false,
                            vec![PalettePage {
                                id: "root".into(),
                                placeholder: "Search".into(),
                                items: vec![PaletteItem {
                                    id: "alpha".into(),
                                    label: "Alpha".into(),
                                    detail: String::new(),
                                    shortcut: String::new(),
                                    group: None,
                                    destination: None,
                                }],
                            }],
                            w,
                            cx,
                        )
                    });
                    let received = events.clone();
                    subscriptions.push(cx.subscribe(&e, move |_, _, event: &PaletteEvent, _| {
                        received.borrow_mut().push(Observed::Palette(event.clone()));
                    }));
                    e.update(cx, |s, cx| s.open(w, cx));
                    Control::Palette(e)
                }
                Kind::Ssh | Kind::ExtensionInput => {
                    let e = cx.new(|cx| {
                        NativeDialog::new(
                            DialogProps {
                                kind: if matches!(kind, Kind::Ssh) {
                                    DialogKind::Ssh
                                } else {
                                    DialogKind::ExtensionInput
                                },
                                title: "Input".into(),
                                description: "Description".into(),
                                placeholder: "Type here".into(),
                                options: vec![],
                            },
                            false,
                            w,
                            cx,
                        )
                        .embedded()
                    });
                    let received = events.clone();
                    subscriptions.push(cx.subscribe(&e, move |_, _, event: &DialogEvent, _| {
                        received.borrow_mut().push(Observed::Dialog(event.clone()));
                    }));
                    Control::Dialog(e)
                }
            };
            let input = control.editor(cx);
            let received = enters.clone();
            subscriptions.push(cx.subscribe(&input, move |_, _, event: &InputEvent, _| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    *received.borrow_mut() += 1;
                }
            }));
            host.update(cx, |s, cx| {
                s.control = Some(control.clone());
                cx.notify();
            });
            (control, input, subscriptions)
        });
        let mut f = Self {
            app,
            window,
            host,
            control,
            input,
            events,
            enters,
            _subscriptions: subscriptions,
        };
        f.window.draw();
        match kind {
            Kind::Model => f.click("trigger"),
            Kind::Palette => f.click("search"),
            Kind::Ssh | Kind::ExtensionInput => f.click("input"),
        }
        f.window.update(|_, w, cx| {
            assert!(
                f.input.read(cx).focus_handle(cx).is_focused(w),
                "{kind:?} click/open focus"
            );
            assert_eq!(
                f.host
                    .read(cx)
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref(),
                DRAFT
            );
        });
        f
    }
    fn click(&mut self, id: &str) {
        self.window.draw();
        let position = self
            .window
            .update(|_, w, _| w.find(id.to_owned()).bounds().center());
        self.window.simulate_click(position, MouseButton::Left);
        self.window.draw();
    }
    fn mark(&mut self) {
        self.window.update(|_, w, cx| {
            self.input.update(cx, |s, cx| {
                s.replace_and_mark_text_in_range(Some(0..0), "al", Some(2..2), w, cx);
                assert_eq!(s.marked_text_range(w, cx), Some(0..2));
            })
        });
        self.window.draw();
    }
    fn commit(&mut self, unmark: bool) {
        self.window.update(|_, w, cx| {
            self.input.update(cx, |s, cx| {
                if unmark {
                    s.unmark_text(w, cx);
                } else {
                    s.replace_text_in_range(None, "alpha", w, cx);
                }
                assert_eq!(s.marked_text_range(w, cx), None);
            })
        });
        self.window.draw();
    }
    fn key(&mut self, key: &str) {
        self.window.simulate_keystroke(key);
        self.window.draw();
    }
    fn assert_open_and_quiet(&mut self) {
        assert!(
            self.window.read(|_, cx| self.control.open(cx)),
            "unexpected close: {:?}",
            self.events.borrow()
        );
        assert!(
            self.events.borrow().is_empty(),
            "{:?}",
            self.events.borrow()
        );
        assert_eq!(*self.enters.borrow(), 0);
    }
    fn assert_draft(&mut self) {
        self.window.read(|_, cx| {
            assert_eq!(
                self.host
                    .read(cx)
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .value()
                    .as_ref(),
                DRAFT
            );
        });
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

fn enter_waits_for_resolution(kind: Kind) {
    for unmark in [false, true] {
        let mut f = Fixture::new(kind);
        f.mark();
        f.key("enter");
        f.assert_open_and_quiet();
        f.window.update(|_, w, cx| {
            f.input.update(cx, |s, cx| {
                assert_eq!(s.marked_text_range(w, cx), Some(0..2));
                assert_eq!(s.value().as_ref(), "al");
            })
        });
        f.commit(unmark);
        f.key("enter");
        let expected = match kind {
            // Base unmarkText retains preedit without emitting Change; the
            // selector's existing cursor stays beta until a committed edit.
            Kind::Model => Observed::Model(ModelEvent::Selected(
                if unmark { "beta" } else { "alpha" }.into(),
            )),
            Kind::Palette => Observed::Palette(PaletteEvent::Selected("alpha".into())),
            Kind::Ssh => Observed::Dialog(DialogEvent::Submitted(
                if unmark { "al" } else { "alpha" }.into(),
            )),
            Kind::ExtensionInput => unreachable!(),
        };
        assert_eq!(*f.events.borrow(), vec![expected]);
        assert_eq!(*f.enters.borrow(), 1);
        f.assert_draft();
    }
}

fn escape_resolves_before_dismissal(kind: Kind) {
    for unmark in [false, true] {
        let mut f = Fixture::new(kind);
        f.mark();
        f.commit(unmark);
        f.key("escape");
        assert!(!f.window.read(|_, cx| f.control.open(cx)), "{kind:?}");
        f.assert_draft();
    }
    let mut f = Fixture::new(kind);
    f.mark();
    f.key("escape");
    f.assert_open_and_quiet();
    f.window.update(|_, w, cx| {
        f.input.update(cx, |s, cx| {
            assert_eq!(s.marked_text_range(w, cx), None);
            assert_eq!(s.value().as_ref(), "al");
        })
    });
    f.key("escape");
    assert!(!f.window.read(|_, cx| f.control.open(cx)), "{kind:?}");
    if matches!(kind, Kind::Palette) {
        assert_eq!(
            *f.events.borrow(),
            vec![Observed::Palette(PaletteEvent::Dismissed)]
        );
    } else if matches!(kind, Kind::Ssh) {
        assert_eq!(
            *f.events.borrow(),
            vec![Observed::Dialog(DialogEvent::Cancelled)]
        );
    }
    f.assert_draft();
}

#[test]
fn ime_model_enter_waits_for_resolution() {
    enter_waits_for_resolution(Kind::Model);
}
#[test]
fn ime_palette_enter_waits_for_resolution() {
    enter_waits_for_resolution(Kind::Palette);
}
#[test]
fn ime_ssh_enter_waits_for_resolution() {
    enter_waits_for_resolution(Kind::Ssh);
}
#[test]
fn ime_model_escape_resolves_before_dismissal() {
    escape_resolves_before_dismissal(Kind::Model);
}
#[test]
fn ime_palette_escape_resolves_before_dismissal() {
    escape_resolves_before_dismissal(Kind::Palette);
}
#[test]
fn ime_ssh_escape_resolves_before_dismissal() {
    escape_resolves_before_dismissal(Kind::Ssh);
}

#[test]
fn ime_extension_input_keeps_its_button_submission_contract() {
    // ExtensionInput has no keyboard submit/dismiss handler. Do not invent one:
    // test its real Base PressEnter boundary and the existing Submit button.
    for unmark in [false, true] {
        let mut f = Fixture::new(Kind::ExtensionInput);
        f.mark();
        f.key("enter");
        f.assert_open_and_quiet();
        f.commit(unmark);
        f.key("enter");
        assert_eq!(*f.enters.borrow(), 1);
        assert!(f.events.borrow().is_empty());
        f.click("submit");
        assert_eq!(
            *f.events.borrow(),
            vec![Observed::Dialog(DialogEvent::Submitted(
                if unmark { "al" } else { "alpha" }.into()
            ))]
        );
        f.assert_draft();
    }
    let mut f = Fixture::new(Kind::ExtensionInput);
    f.mark();
    f.key("escape");
    f.assert_open_and_quiet();
    f.window.update(|_, w, cx| {
        f.input
            .update(cx, |s, cx| assert_eq!(s.marked_text_range(w, cx), None))
    });
    f.key("escape");
    f.assert_open_and_quiet();
    f.click("cancel");
    assert_eq!(
        *f.events.borrow(),
        vec![Observed::Dialog(DialogEvent::Cancelled)]
    );
    f.assert_draft();
}

#[test]
fn ime_ssh_busy_and_readonly_guards_survive_commit() {
    for busy in [false, true] {
        let mut f = Fixture::new(Kind::Ssh);
        f.mark();
        f.commit(false);
        let Control::Dialog(dialog) = f.control.clone() else {
            unreachable!()
        };
        f.window.update(|_, _, cx| {
            dialog.update(cx, |s, cx| {
                if busy {
                    s.set_busy(true, cx);
                } else {
                    s.set_readonly(true, cx);
                }
            })
        });
        f.window.draw();
        f.key("enter");
        assert!(f.events.borrow().is_empty());
        assert!(f.window.read(|_, cx| dialog.read(cx).is_open()));
        if busy {
            f.key("escape");
            assert!(f.events.borrow().is_empty());
            assert!(f.window.read(|_, cx| dialog.read(cx).is_open()));
        }
        f.window.update(|_, _, cx| {
            dialog.update(cx, |s, cx| {
                s.set_busy(false, cx);
                s.set_readonly(false, cx);
            })
        });
        f.window.draw();
        f.key("enter");
        assert_eq!(
            *f.events.borrow(),
            vec![Observed::Dialog(DialogEvent::Submitted("alpha".into()))]
        );
        f.key("enter");
        assert_eq!(
            f.events.borrow().len(),
            1,
            "busy blocks duplicate submission"
        );
        f.assert_draft();
    }
}
