//! Real Root/Input responders on TestPlatform. No AppKit or OS state claims.
use super::*;
use gpui_kit::{
    Action, AppContext, Context, Entity, Focusable, IntoElement, MouseButton, Render, TestApp,
    TestAppWindow, TestSupportExt, Window,
    base::{Input, Root, input::InputState},
    div,
    prelude::*,
    px,
    test::TestWindowExt,
};
use std::{cell::Cell, rc::Rc};

const DRAFT: &str = "Draft café 😀 第二行";

struct Host {
    input: Entity<InputState>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("editor")
                .test_support()
                .h(px(36.))
                .child(Input::new(&self.input)),
        )
    }
}

struct Fixture {
    first: TestAppWindow<Root>,
    second: TestAppWindow<Root>,
    first_input: Entity<InputState>,
    second_input: Entity<InputState>,
    allow: Rc<Cell<bool>>,
    checks: Rc<Cell<usize>>,
    closed: Rc<Cell<usize>>,
    app: TestApp,
}

fn open(app: &mut TestApp) -> (TestAppWindow<Root>, Entity<InputState>) {
    let mut input = None;
    let mut window = app.open_window(|w, cx| {
        let editor = cx.new(|cx| {
            let mut input = InputState::new(w, cx);
            input.set_value(DRAFT, w, cx);
            input
        });
        input = Some(editor.clone());
        let host = cx.new(|_| Host { input: editor });
        Root::new(host, w, cx)
    });
    let input = input.unwrap();
    window.draw();
    let position = window.update(|_, w, _| w.find("editor").bounds().center());
    window.simulate_click(position, MouseButton::Left);
    window.update(|_, w, cx| assert!(input.read(cx).focus_handle(cx).is_focused(w)));
    (window, input)
}

impl Fixture {
    fn new() -> Self {
        let mut app = TestApp::new();
        app.update(|cx| {
            crate::components::init(cx, false);
            assert!(!cx.is_action_available(&CloseWindow));
            assert!(!cx.is_action_available(&MinimizeWindow));
            install_window_commands(cx);
            let bindings = cx.key_bindings().borrow().bindings().len();
            install_window_commands(cx);
            assert_eq!(cx.key_bindings().borrow().bindings().len(), bindings);
            assert!(!cx.is_action_available(&CloseWindow));
            assert!(!cx.is_action_available(&MinimizeWindow));
        });
        let (mut first, first_input) = open(&mut app);
        let (second, second_input) = open(&mut app);
        let allow = Rc::new(Cell::new(false));
        let checks = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let guard_allow = allow.clone();
        let guard_checks = checks.clone();
        first.update(|_, w, cx| {
            w.on_window_should_close(cx, move |_, _| {
                guard_checks.set(guard_checks.get() + 1);
                guard_allow.get()
            })
        });
        let count = closed.clone();
        app.update(|cx| {
            cx.on_window_closed(move |_, _| count.set(count.get() + 1))
                .detach()
        });
        first.simulate_activation(true);
        app.update(|cx| {
            assert!(cx.is_action_available(&CloseWindow));
            assert!(cx.is_action_available(&MinimizeWindow));
        });
        Self {
            app,
            first,
            second,
            first_input,
            second_input,
            allow,
            checks,
            closed,
        }
    }

    fn menu(&mut self, label: &str) {
        let action = window_menu()
            .items
            .into_iter()
            .find_map(|item| match item {
                MenuItem::Action { name, action, .. } if name.as_ref() == label => Some(action),
                _ => None,
            })
            .unwrap();
        // Native menu delivery enters App::dispatch_action, not a component handler.
        self.app.update(|cx| cx.dispatch_action(action.as_ref()));
        self.app.run_until_parked();
    }

    fn assert_draft_and_focus(window: &mut TestAppWindow<Root>, input: &Entity<InputState>) {
        window.update(|_, w, cx| {
            assert_eq!(input.read(cx).value().as_ref(), DRAFT);
            assert!(input.read(cx).focus_handle(cx).is_focused(w));
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        self.app.update(|cx| {
            for handle in cx.windows() {
                let _ = handle.update(cx, |_, w, cx| w.blur(cx));
            }
        });
        self.app.simulate_deactivation();
        self.app.run_until_parked();
    }
}

#[test]
fn menu_fragment_has_only_window_actions() {
    let menu = window_menu();
    assert_eq!(menu.name.as_ref(), "Window");
    assert_eq!(menu.items.len(), 2);
    let actions: [(&str, Box<dyn Action>); 2] = [
        ("Minimize", Box::new(MinimizeWindow)),
        ("Close Window", Box::new(CloseWindow)),
    ];
    for (item, (label, expected)) in menu.items.iter().zip(actions) {
        let MenuItem::Action {
            name,
            action,
            os_action,
            disabled,
            ..
        } = item
        else {
            panic!()
        };
        assert_eq!(name.as_ref(), label);
        assert_eq!(action.as_any().type_id(), expected.as_any().type_id());
        assert!(os_action.is_none() && !disabled);
    }
}

#[test]
fn cancelled_keyboard_menu_and_native_close_preserve_draft_and_focus() {
    let mut f = Fixture::new();
    f.first.simulate_keystroke("cmd-w");
    assert_eq!(f.checks.get(), 1);
    f.menu("Close Window");
    assert_eq!(f.checks.get(), 2);
    assert!(!f.first.simulate_close());
    assert_eq!(f.checks.get(), 3);
    assert_eq!(f.closed.get(), 0);
    assert_eq!(f.app.read(|cx| cx.windows().len()), 2);
    Fixture::assert_draft_and_focus(&mut f.first, &f.first_input);
    Fixture::assert_draft_and_focus(&mut f.second, &f.second_input);
}

#[test]
fn allowed_close_runs_guard_and_closes_exactly_once() {
    let mut f = Fixture::new();
    f.allow.set(true);
    f.first.simulate_keystroke("cmd-w");
    assert_eq!(f.checks.get(), 1);
    assert_eq!(f.closed.get(), 1);
    assert_eq!(f.app.read(|cx| cx.windows().len()), 1);
    f.second.simulate_activation(true);
    Fixture::assert_draft_and_focus(&mut f.second, &f.second_input);
    f.menu("Close Window");
    assert_eq!(f.checks.get(), 1);
    assert_eq!(f.closed.get(), 2);
    assert!(f.app.read(|cx| cx.windows().is_empty()));
    // TestPlatform retains activation until the host explicitly deactivates it.
    f.app.simulate_deactivation();
    f.app.update(|cx| {
        assert!(!cx.is_action_available(&CloseWindow));
        assert!(!cx.is_action_available(&MinimizeWindow));
    });
    assert_eq!(f.app.quit_mode(), QuitMode::Explicit);
}

#[test]
fn native_allow_uses_same_guard_once() {
    let mut f = Fixture::new();
    f.allow.set(true);
    assert!(f.first.simulate_close());
    assert_eq!(f.checks.get(), 1);
    assert_eq!(f.closed.get(), 1);
    assert_eq!(f.app.read(|cx| cx.windows().len()), 1);
}

#[test]
fn minimize_routes_to_active_window_and_installation_is_once() {
    let mut f = Fixture::new();
    f.first.simulate_keystroke("cmd-m");
    assert_eq!(f.first.minimize_requests(), 1);
    assert_eq!(f.second.minimize_requests(), 0);
    f.second.simulate_activation(true);
    f.menu("Minimize");
    assert_eq!(f.first.minimize_requests(), 1);
    assert_eq!(f.second.minimize_requests(), 1);
    assert_eq!(f.checks.get(), 0);
    Fixture::assert_draft_and_focus(&mut f.first, &f.first_input);
    Fixture::assert_draft_and_focus(&mut f.second, &f.second_input);
}

#[test]
fn no_active_window_is_inert() {
    let mut f = Fixture::new();
    f.first.simulate_activation(false);
    assert!(f.app.read(|cx| cx.active_window().is_none()));
    f.app.update(|cx| {
        assert!(!cx.is_action_available(&CloseWindow));
        assert!(!cx.is_action_available(&MinimizeWindow));
    });
    f.menu("Close Window");
    f.menu("Minimize");
    assert_eq!(f.first.minimize_requests(), 0);
    assert_eq!(f.second.minimize_requests(), 0);
    assert_eq!(f.checks.get(), 0);
    assert_eq!(f.closed.get(), 0);
    assert_eq!(f.app.read(|cx| cx.windows().len()), 2);
}

#[test]
fn close_guard_can_reenter_or_replace_without_a_borrow_or_double_invocation() {
    let mut f = Fixture::new();
    let checks = f.checks.clone();
    f.first.update(|_, w, cx| {
        w.on_window_should_close(cx, move |w, cx| {
            checks.set(checks.get() + 1);
            assert!(!w.request_close(cx));
            w.on_window_should_close(cx, |_, _| true);
            false
        });
    });
    f.menu("Close Window");
    assert_eq!(f.checks.get(), 1);
    assert_eq!(f.closed.get(), 0);
    Fixture::assert_draft_and_focus(&mut f.first, &f.first_input);
    f.menu("Close Window");
    assert_eq!(f.checks.get(), 1);
    assert_eq!(f.closed.get(), 1);
}
