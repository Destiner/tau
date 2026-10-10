use super::*;
use crate::native::lifecycle::{Intent, LifecycleCallbacks, Outcome, Request};
use gpui_kit::{
    AppContext, Context, Entity, Focusable, IntoElement, Render, TestApp, TestAppWindow, Window,
    base::{Input, Root, input::InputState},
    div,
    prelude::*,
};
use std::cell::{Cell, RefCell};

#[path = "host_policy_tests.rs"]
mod policy;

const DRAFT: &str = "Draft café 😀 第二行";
struct View {
    input: Entity<InputState>,
}
impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(Input::new(&self.input))
    }
}
fn root(
    window: &mut Window,
    cx: &mut Context<Root>,
    editors: &Rc<RefCell<Vec<Entity<InputState>>>>,
) -> Root {
    let input = cx.new(|cx| {
        let mut input = InputState::new(window, cx);
        input.set_value(DRAFT, window, cx);
        input
    });
    editors.borrow_mut().push(input.clone());
    let view = cx.new(|_| View { input });
    Root::new(view, window, cx)
}
struct Fixture {
    app: TestApp,
    host: NativeHost,
    first: TestAppWindow<Root>,
    second: TestAppWindow<Root>,
    requests: Rc<RefCell<Vec<Request>>>,
    settled: Rc<RefCell<Vec<(Request, Outcome)>>>,
    editors: Rc<RefCell<Vec<Entity<InputState>>>>,
    created: Rc<Cell<usize>>,
    sessions: Rc<Cell<usize>>,
    reveals: Rc<RefCell<Vec<AnyWindowHandle>>>,
    fail_factory: Rc<Cell<bool>>,
    failures: Rc<Cell<usize>>,
    cleanup: Rc<Cell<usize>>,
}
impl Fixture {
    fn new() -> Self {
        let mut app = TestApp::new();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let settled = Rc::new(RefCell::new(Vec::new()));
        let editors = Rc::new(RefCell::new(Vec::new()));
        let created = Rc::new(Cell::new(0));
        let sessions = Rc::new(Cell::new(0));
        let reveals = Rc::new(RefCell::new(Vec::new()));
        let fail_factory = Rc::new(Cell::new(false));
        let failures = Rc::new(Cell::new(0));
        let cleanup = Rc::new(Cell::new(0));
        app.update(|cx| crate::components::init(cx, false));
        let host = app.update(|cx| {
            let requests = requests.clone();
            let settled = settled.clone();
            let editors = editors.clone();
            let created = created.clone();
            let sessions = sessions.clone();
            let reveals = reveals.clone();
            let fail_factory = fail_factory.clone();
            let failures = failures.clone();
            let count = cleanup.clone();
            cx.on_app_quit(move |_| {
                count.set(count.get() + 1);
                async {}
            })
            .detach();
            NativeHost::install(
                cx,
                "Tau",
                Duration::from_secs(60),
                HostCallbacks {
                    lifecycle: LifecycleCallbacks {
                        requested: Rc::new(move |request, _| requests.borrow_mut().push(request)),
                        settled: Rc::new(move |request, outcome, _| {
                            settled.borrow_mut().push((request, outcome))
                        }),
                    },
                    new_session: Rc::new(move |_| sessions.set(sessions.get() + 1)),
                    create_window: Rc::new(move |cx| {
                        created.set(created.get() + 1);
                        if fail_factory.get() {
                            return Err(anyhow::anyhow!("synthetic factory failure"));
                        }
                        cx.open_window(Default::default(), |w, cx| {
                            cx.new(|cx| root(w, cx, &editors))
                        })
                        .map(Into::into)
                    }),
                    reveal_window: Rc::new(move |target, cx| {
                        reveals.borrow_mut().push(target);
                        reveal_window(target, cx).unwrap();
                    }),
                    window_failed: Rc::new(move |_| failures.set(failures.get() + 1)),
                },
            )
        });
        let mut first = app.open_window(|w, cx| root(w, cx, &editors));
        let mut second = app.open_window(|w, cx| root(w, cx, &editors));
        first.update(|_, w, cx| host.lifecycle.attach(w, cx));
        second.update(|_, w, cx| host.lifecycle.attach(w, cx));
        first.draw();
        second.draw();
        first.update(|_, w, cx| editors.borrow()[0].read(cx).focus_handle(cx).focus(w, cx));
        first.simulate_activation(true);
        Self {
            app,
            host,
            first,
            second,
            requests,
            settled,
            editors,
            created,
            sessions,
            reveals,
            fail_factory,
            failures,
            cleanup,
        }
    }
    fn dispatch(&mut self, action: &dyn gpui_kit::Action) {
        self.app.update(|cx| cx.dispatch_action(action));
    }
    fn latest(&self) -> Request {
        *self.requests.borrow().last().unwrap()
    }
    fn resolve(&mut self, request: Request, approved: bool) -> bool {
        self.app
            .update(|cx| self.host.lifecycle.resolve(request, approved, cx))
    }
    fn drafts(&self) {
        self.app.read(|cx| {
            for input in self.editors.borrow().iter() {
                assert_eq!(input.read(cx).value().as_ref(), DRAFT);
            }
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        self.app.update(|cx| {
            for target in cx.windows() {
                let _ = target.update(cx, |_, w, cx| w.blur(cx));
            }
        });
        self.app.simulate_deactivation();
        self.editors.borrow_mut().clear();
    }
}

#[test]
fn standard_reveal_keeps_editors_and_reports_removed_targets() {
    let mut f = Fixture::new();
    let target: AnyWindowHandle = f.first.handle().into();
    f.app.update(|cx| reveal_window(target, cx).unwrap());
    f.drafts();
    f.app.update(|cx| {
        target.update(cx, |_, w, _| w.remove_window()).unwrap();
        assert!(reveal_window(target, cx).is_err());
    });
    assert_eq!(f.created.get(), 0);
    assert_eq!(f.cleanup.get(), 0);
    f.drafts();
}

#[test]
fn host_installation_is_idempotent_and_keeps_later_host_policy() {
    let mut f = Fixture::new();
    f.app.update(|cx| {
        let bindings = cx.key_bindings().borrow().bindings().len();
        cx.set_quit_mode(gpui_kit::QuitMode::LastWindowClosed);
        NativeHost::install(
            cx,
            "Replacement",
            Duration::from_secs(1),
            HostCallbacks {
                lifecycle: LifecycleCallbacks {
                    requested: Rc::new(|_, _| panic!("original callback must remain")),
                    settled: Rc::new(|_, _, _| panic!("original callback must remain")),
                },
                new_session: Rc::new(|_| panic!("original callback must remain")),
                create_window: Rc::new(|_| panic!("original callback must remain")),
                reveal_window: Rc::new(|_, _| panic!("original callback must remain")),
                window_failed: Rc::new(|_| panic!("original callback must remain")),
            },
        );
        assert_eq!(cx.key_bindings().borrow().bindings().len(), bindings);
    });
    assert_eq!(f.app.quit_mode(), gpui_kit::QuitMode::LastWindowClosed);
    f.dispatch(&NewSession);
    assert_eq!(f.sessions.get(), 1);
}

#[test]
fn composed_menus_exclude_settings_and_keep_no_active_window_dispatch() {
    let mut f = Fixture::new();
    let menus = host_menus("Tau");
    assert_eq!(
        menus.iter().map(|m| m.name.as_ref()).collect::<Vec<_>>(),
        ["Tau", "File", "Edit", "Window"]
    );
    assert_eq!(menus[0].items.len(), 1);
    assert!(
        matches!(&menus[0].items[0], MenuItem::Action { name, .. } if name.as_ref() == "Quit Tau")
    );
    let bindings = f.app.update(|cx| {
        cx.key_bindings()
            .borrow()
            .bindings()
            .cloned()
            .collect::<Vec<_>>()
    });
    let unbound = KeyBinding::new("cmd-,", Quit, None);
    assert!(
        bindings
            .iter()
            .all(|binding| binding.keystrokes() != unbound.keystrokes())
    );
    f.first.simulate_keystroke("cmd-,");
    assert_eq!(f.sessions.get(), 0);
    assert_eq!(f.created.get(), 0);
    assert!(f.requests.borrow().is_empty());
    f.drafts();
    let new_session = menus[1]
        .items
        .iter()
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if name.as_ref() == "New Session" => Some(action),
            _ => None,
        })
        .unwrap();
    f.app.update(|cx| cx.dispatch_action(new_session.as_ref()));
    f.first.simulate_keystroke("cmd-n");
    assert_eq!(f.sessions.get(), 2);
    assert_eq!(
        f.created.get(),
        0,
        "Cmd-N must not replace Tau's New Session with New Window"
    );
    f.app.simulate_deactivation();
    f.dispatch(&CloseWindow);
    assert!(f.requests.borrow().is_empty());
    f.dispatch(&Quit);
    assert_eq!(f.latest().intent(), Intent::Quit);
    assert_eq!(f.cleanup.get(), 0);
    f.dispatch(&NewWindow);
    f.dispatch(&NewSession);
    assert_eq!(f.created.get(), 0);
    assert_eq!(f.sessions.get(), 2);
    assert!(!f.app.simulate_quit());
    assert_eq!(f.requests.borrow().len(), 1);
    assert!(f.resolve(f.latest(), false));
    f.drafts();
}

#[test]
fn independent_close_cancel_repeat_and_native_callbacks_keep_same_entities() {
    let mut f = Fixture::new();
    let original = f.editors.borrow().clone();
    f.first.simulate_keystroke("cmd-w");
    let first = f.latest();
    f.dispatch(&CloseWindow);
    assert_eq!(f.requests.borrow().len(), 1);
    f.second.simulate_activation(true);
    f.dispatch(&CloseWindow);
    let second = f.latest();
    assert_ne!(first, second);
    assert!(matches!(second.intent(), Intent::Close(_)));
    assert!(f.resolve(first, false));
    assert!(!f.resolve(first, true));
    f.drafts();
    assert_eq!(*f.editors.borrow(), original);
    f.first
        .update(|_, w, cx| assert!(original[0].read(cx).focus_handle(cx).is_focused(w)));
    f.first.simulate_activation(true);
    assert!(!f.first.simulate_close());
    let next = f.latest();
    assert_ne!(next, first);
    assert!(f.resolve(next, false));
    assert!(f.resolve(second, true));
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
    f.drafts();
    assert_eq!(f.cleanup.get(), 0);
}

#[test]
fn quit_supersedes_close_rejects_stale_decisions_and_shutdown_is_after_approval() {
    let mut f = Fixture::new();
    f.dispatch(&CloseWindow);
    let close = f.latest();
    f.first.simulate_keystroke("cmd-q");
    let quit = f.latest();
    assert_eq!(quit.intent(), Intent::Quit);
    assert!(f.settled.borrow().contains(&(close, Outcome::Superseded)));
    assert!(!f.resolve(close, true));
    assert!(!f.app.simulate_quit());
    f.dispatch(&Quit);
    assert_eq!(f.requests.borrow().len(), 2);
    assert!(f.resolve(quit, false));
    f.drafts();
    assert_eq!(f.cleanup.get(), 0);
    assert!(!f.app.simulate_quit());
    let retry = f.latest();
    assert_ne!(quit, retry);
    assert!(!f.resolve(quit, true));
    assert!(f.resolve(retry, true));
    assert_eq!(f.cleanup.get(), 1);
    assert_eq!(f.app.accepted_quits(), 1);
    assert!(!f.resolve(retry, true));
    assert!(!f.app.simulate_quit());
    assert_eq!(f.cleanup.get(), 1);
    f.app.read(|cx| assert!(cx.windows().is_empty()));
}

#[test]
fn reopen_reuses_owned_windows_then_factory_and_reports_failure() {
    let mut f = Fixture::new();
    f.app.simulate_reopen();
    assert_eq!(f.created.get(), 0);
    assert_eq!(f.reveals.borrow().len(), 1);
    f.dispatch(&CloseWindow);
    let first = f.latest();
    assert!(f.resolve(first, true));
    f.second.simulate_activation(true);
    f.dispatch(&CloseWindow);
    let second = f.latest();
    assert!(f.resolve(second, true));
    f.app.read(|cx| assert!(cx.windows().is_empty()));
    assert_eq!(f.cleanup.get(), 0);
    f.fail_factory.set(true);
    f.app.simulate_reopen();
    assert_eq!(f.failures.get(), 1);
    f.fail_factory.set(false);
    f.app.simulate_reopen();
    assert_eq!(f.created.get(), 2);
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
    f.app.simulate_reopen();
    assert_eq!(f.created.get(), 2);
    f.dispatch(&NewWindow);
    assert_eq!(f.created.get(), 3);
    f.app.read(|cx| assert_eq!(cx.windows().len(), 2));
}

#[test]
fn expired_async_approval_cannot_close_newer_request() {
    let mut f = Fixture::new();
    f.dispatch(&CloseWindow);
    let old = f.latest();
    let delivered = Rc::new(Cell::new(false));
    let delivery = delivered.clone();
    f.app.update(|cx| {
        let host = f.host.lifecycle.clone();
        let timer = cx.background_executor().timer(Duration::from_secs(65));
        cx.spawn(async move |cx| {
            timer.await;
            cx.update(|cx| {
                assert!(!host.resolve(old, true, cx));
                delivery.set(true);
            });
        })
        .detach();
    });
    f.app.advance_clock(Duration::from_secs(61));
    assert!(f.settled.borrow().contains(&(old, Outcome::Expired)));
    f.dispatch(&CloseWindow);
    let current = f.latest();
    assert_ne!(old, current);
    f.app.advance_clock(Duration::from_secs(5));
    assert!(delivered.get());
    f.app.read(|cx| assert_eq!(cx.windows().len(), 2));
    f.drafts();
    assert!(f.resolve(current, false));
}

#[test]
fn elapsed_deadline_rejects_approval_before_timer_delivery_and_preserves_owner_context() {
    for (quit, elapsed) in [(false, 60), (true, 60), (false, 61), (true, 61)] {
        let mut f = Fixture::new();
        let editors = f.editors.borrow().clone();
        let windows = f.app.read(|cx| cx.windows());
        if quit {
            f.dispatch(&Quit);
        } else {
            f.dispatch(&CloseWindow);
        }
        let old = f.latest();
        let host = f.host.lifecycle.clone();
        f.app.update(|cx| {
            // Move only the executor clock: no foreground timer callback can run
            // while this App update owns the foreground context.
            cx.background_executor()
                .dispatcher()
                .as_test()
                .unwrap()
                .scheduler()
                .clock()
                .advance(Duration::from_secs(elapsed));
            assert!(f.settled.borrow().is_empty());
            assert!(!host.is_pending(old));
            assert!(!host.resolve(old, true, cx));
            assert!(!host.resolve(old, false, cx));
            assert!(!host.is_exiting());
            assert_eq!(cx.windows(), windows);
            assert_eq!(f.cleanup.get(), 0);
            if quit {
                cx.dispatch_action(&Quit);
            } else {
                cx.dispatch_action(&CloseWindow);
            }
        });
        // update drained the stale old timer after the replacement was installed.
        let current = f.latest();
        assert_ne!(current, old);
        assert!(host.is_pending(current));
        assert_eq!(*f.settled.borrow(), [(old, Outcome::Expired)]);
        assert_eq!(f.app.accepted_quits(), 0);
        assert_eq!(f.cleanup.get(), 0);
        assert_eq!(*f.editors.borrow(), editors);
        f.app.read(|cx| assert_eq!(cx.windows(), windows));
        f.first.update(|_, window, cx| {
            assert!(editors[0].read(cx).focus_handle(cx).is_focused(window));
        });
        f.drafts();
        assert!(!f.resolve(old, true));
        assert!(host.is_pending(current));
        assert!(f.resolve(current, false));
        assert_eq!(
            *f.settled.borrow(),
            [(old, Outcome::Expired), (current, Outcome::Cancelled)]
        );
    }
}

#[test]
fn foreign_host_decision_is_not_authorization() {
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    first.dispatch(&Quit);
    second.dispatch(&Quit);
    let foreign = first.latest();
    assert!(!second.resolve(foreign, true));
    assert_eq!(second.cleanup.get(), 0);
    assert!(first.resolve(foreign, false));
    assert!(second.resolve(second.latest(), false));
}

#[test]
fn target_removal_retires_request_and_rejects_late_approval() {
    let mut f = Fixture::new();
    f.dispatch(&CloseWindow);
    let request = f.latest();
    f.first.update(|_, window, _| window.remove_window());
    assert!(
        f.settled
            .borrow()
            .contains(&(request, Outcome::TargetClosed))
    );
    assert!(!f.resolve(request, true));
    f.app.read(|cx| assert_eq!(cx.windows().len(), 1));
}
