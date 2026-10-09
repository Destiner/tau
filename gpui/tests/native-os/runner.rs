//! Opt-in, main-thread AppKit bridge contract. This opens only a synthetic owned window.
//! No clipboard operations, AX clients, global events, Pi, SSH, or user files.
//! Physical keyboard/IME and assistive-technology behavior remain manual gates.
#![cfg(feature = "native-os-tests")]
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native-os requires macOS and an unlocked WindowServer session");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2.7 macros test the legacy cargo-clippy feature.
mod macos {
    use gpui_kit::{
        AnyWindowHandle, AppContext, AsyncApp, Context, Entity, Focusable, IntoElement,
        KeyDownEvent, Keystroke, Menu, MenuItem, PlatformInput, Render, Window, WindowOptions,
        actions,
        base::{Input, Root, input::InputState},
        div,
        prelude::*,
    };
    use objc::{
        class, msg_send,
        runtime::{BOOL, Object, Sel, YES},
        sel, sel_impl,
    };
    use std::{
        cell::Cell,
        ffi::CStr,
        rc::Rc,
        time::{Duration, Instant},
    };
    use tau_gpui_prototype::{
        components::{Assets, init},
        native,
    };

    type Id = *mut Object;
    const NIL: Id = std::ptr::null_mut();
    const TEXT: &str = "Owned café 😀";
    actions!(native_os_test, [SafeQuit]);

    struct Host {
        input: Entity<InputState>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(Input::new(&self.input))
        }
    }

    // All Objective-C objects below belong to NSApplication in this process.
    // Calls happen on the main thread, outside App borrows: native delegates reenter GPUI.
    unsafe fn app() -> Id {
        unsafe { msg_send![class!(NSApplication), sharedApplication] }
    }
    unsafe fn string(s: Id) -> String {
        unsafe {
            let p: *const std::ffi::c_char = msg_send![s, UTF8String];
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
    unsafe fn menu(name: &str) -> Id {
        unsafe {
            let main: Id = msg_send![app(), mainMenu];
            assert!(!main.is_null(), "missing AppKit menu bar");
            let n: isize = msg_send![main, numberOfItems];
            for i in 0..n {
                let item: Id = msg_send![main, itemAtIndex: i];
                let sub: Id = msg_send![item, submenu];
                if !sub.is_null() {
                    let title: Id = msg_send![sub, title];
                    if string(title) == name {
                        return sub;
                    }
                }
            }
            panic!("missing native {name} menu");
        }
    }
    unsafe fn index(menu: Id, name: &str) -> isize {
        unsafe {
            let n: isize = msg_send![menu, numberOfItems];
            for i in 0..n {
                let item: Id = msg_send![menu, itemAtIndex: i];
                let title: Id = msg_send![item, title];
                if string(title) == name {
                    return i;
                }
            }
            panic!("missing native item {name}");
        }
    }
    fn native_enabled(menu_name: &str, name: &str) -> bool {
        unsafe {
            let m = menu(menu_name);
            let _: () = msg_send![m, update];
            let item: Id = msg_send![m, itemAtIndex: index(m, name)];
            let enabled: BOOL = msg_send![item, isEnabled];
            enabled == YES
        }
    }
    fn enabled(menu_name: &str, expectations: &[(&str, bool)]) {
        unsafe {
            let m = menu(menu_name);
            let _: () = msg_send![m, update];
            for &(name, expected) in expectations {
                let item: Id = msg_send![m, itemAtIndex: index(m, name)];
                let actual: BOOL = msg_send![item, isEnabled];
                assert_eq!(actual == YES, expected, "native {menu_name}/{name}");
            }
        }
    }
    fn native_commands() {
        const COMMAND: usize = 1 << 20;
        const SHIFT: usize = 1 << 17;
        for (menu_name, name, key, modifiers, selector) in [
            ("Edit", "Undo", "z", COMMAND, "handleGPUIMenuItem:"),
            ("Edit", "Redo", "z", COMMAND | SHIFT, "handleGPUIMenuItem:"),
            ("Edit", "Cut", "x", COMMAND, "cut:"),
            ("Edit", "Copy", "c", COMMAND, "copy:"),
            ("Edit", "Paste", "v", COMMAND, "paste:"),
            ("Edit", "Select All", "a", COMMAND, "selectAll:"),
            ("Window", "Minimize", "m", COMMAND, "handleGPUIMenuItem:"),
            (
                "Window",
                "Close Window",
                "w",
                COMMAND,
                "handleGPUIMenuItem:",
            ),
        ] {
            unsafe {
                let m = menu(menu_name);
                let item: Id = msg_send![m, itemAtIndex: index(m, name)];
                let equivalent: Id = msg_send![item, keyEquivalent];
                let mask: usize = msg_send![item, keyEquivalentModifierMask];
                let action: Sel = msg_send![item, action];
                assert_eq!(string(equivalent), key, "{menu_name}/{name} key equivalent");
                assert_eq!(mask, modifiers, "{menu_name}/{name} modifier mask");
                assert_eq!(action.name(), selector, "{menu_name}/{name} selector");
            }
        }
        eprintln!("PASS native NSMenuItem selectors and key equivalents");
    }
    fn invoke(menu_name: &str, name: &str) {
        enabled(menu_name, &[(name, true)]);
        unsafe {
            let m = menu(menu_name);
            let _: () = msg_send![m, performActionForItemAtIndex: index(m, name)];
        }
    }
    unsafe fn owned_window(title: &str) -> Id {
        unsafe {
            let windows: Id = msg_send![app(), windows];
            let n: usize = msg_send![windows, count];
            for i in 0..n {
                let w: Id = msg_send![windows, objectAtIndex: i];
                let t: Id = msg_send![w, title];
                if string(t) == title {
                    return w;
                }
            }
            NIL
        }
    }
    fn active(w: Id) -> bool {
        unsafe {
            let a: BOOL = msg_send![app(), isActive];
            let key: BOOL = msg_send![w, isKeyWindow];
            let visible: BOOL = msg_send![w, isVisible];
            a == YES && key == YES && visible == YES
        }
    }
    async fn poll(cx: &AsyncApp, label: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(
                Instant::now() < deadline,
                "blocked or failed: {label} (5s deadline)"
            );
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
        }
    }
    fn assert_input(
        cx: &AsyncApp,
        window: AnyWindowHandle,
        input: &Entity<InputState>,
        value: &str,
    ) {
        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    assert_eq!(input.read(cx).value().as_ref(), value);
                    assert!(
                        input.read(cx).focus_handle(cx).is_focused(w),
                        "focused responder was lost"
                    );
                })
                .unwrap()
        });
    }
    async fn exercise(
        cx: AsyncApp,
        window: AnyWindowHandle,
        input: Entity<InputState>,
        title: String,
        checks: Rc<Cell<usize>>,
        allow: Rc<Cell<bool>>,
        closed: Rc<Cell<usize>>,
    ) {
        eprintln!("START owned-window readiness polling");
        poll(&cx, "own native window creation", || unsafe {
            !owned_window(&title).is_null()
        })
        .await;
        eprintln!("READY own native window exists");
        let w = unsafe { owned_window(&title) };
        // Keep the native object alive for post-close inspection only.
        unsafe {
            let _: Id = msg_send![w, retain];
        }
        let activation_deadline = Instant::now() + Duration::from_secs(5);
        while !active(w) {
            if Instant::now() >= activation_deadline {
                unsafe {
                    let a: BOOL = msg_send![app(), isActive];
                    let key: BOOL = msg_send![w, isKeyWindow];
                    let visible: BOOL = msg_send![w, isVisible];
                    eprintln!(
                        "BLOCKED startup: NSApplication.isActive={} NSWindow.isKeyWindow={} isVisible={}; no edit/window actions attempted",
                        a == YES,
                        key == YES,
                        visible == YES
                    );
                }
                std::process::exit(2);
            }
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
        }
        eprintln!("READY native activation/key window");
        poll(&cx, "initial focused BaseInput responder", || {
            cx.update(|cx| {
                window
                    .update(cx, |_, w, cx| {
                        input.read(cx).focus_handle(cx).is_focused(w) && w.is_window_active()
                    })
                    .unwrap()
            })
        })
        .await;
        poll(&cx, "rendered native input action registration", || {
            native_enabled("Edit", "Paste")
        })
        .await;
        eprintln!("READY initial focused responder; validating NSMenu");
        native_commands();
        enabled(
            "Edit",
            &[
                ("Undo", false),
                ("Redo", false),
                ("Cut", false),
                ("Copy", false),
                ("Paste", true),
                ("Select All", false),
            ],
        );
        eprintln!("PASS native empty-input validation (Paste availability only; never invoked)");

        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    input.update(cx, |s, cx| s.insert(TEXT, w, cx))
                })
                .unwrap()
        });
        poll(&cx, "native history availability", || {
            native_enabled("Edit", "Undo")
        })
        .await;
        enabled(
            "Edit",
            &[
                ("Undo", true),
                ("Redo", false),
                ("Cut", false),
                ("Copy", false),
                ("Paste", true),
                ("Select All", true),
            ],
        );
        assert_input(&cx, window, &input, TEXT);
        invoke("Edit", "Select All");
        poll(&cx, "native Select All delivery", || {
            cx.update(|cx| input.read(cx).selected_value().as_ref() == TEXT)
        })
        .await;
        enabled("Edit", &[("Copy", true), ("Cut", true), ("Paste", true)]);
        assert_input(&cx, window, &input, TEXT);
        invoke("Edit", "Undo");
        poll(&cx, "native Undo delivery", || {
            cx.update(|cx| input.read(cx).value().is_empty())
        })
        .await;
        enabled(
            "Edit",
            &[
                ("Undo", false),
                ("Redo", true),
                ("Copy", false),
                ("Cut", false),
                ("Select All", false),
            ],
        );
        invoke("Edit", "Redo");
        poll(&cx, "native Redo delivery", || {
            cx.update(|cx| input.read(cx).value().as_ref() == TEXT)
        })
        .await;
        assert_input(&cx, window, &input, TEXT);
        enabled(
            "Edit",
            &[("Undo", true), ("Redo", false), ("Select All", true)],
        );
        eprintln!(
            "PASS actual NSMenu Select All/Undo/Redo routed to focused BaseInput; native selection/history availability"
        );

        // Use a window-only handle: event dispatch invokes Root listeners itself.
        // Local delivery exercises Cmd-W, not global OS keyboard injection.
        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    w.dispatch_event(
                        PlatformInput::KeyDown(KeyDownEvent {
                            keystroke: Keystroke::parse("cmd-w").unwrap(),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                })
                .unwrap()
        });
        poll(&cx, "Cmd-W close guard", || checks.get() >= 1).await;
        assert_eq!(checks.get(), 1);
        assert_eq!(closed.get(), 0);
        assert!(active(w));
        assert_input(&cx, window, &input, TEXT);
        invoke("Window", "Close Window");
        poll(&cx, "native menu close guard", || checks.get() >= 2).await;
        assert_eq!(checks.get(), 2);
        assert_eq!(closed.get(), 0);
        assert_input(&cx, window, &input, TEXT);
        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| assert!(!w.request_close(cx)))
                .unwrap()
        });
        assert_eq!(checks.get(), 3);
        unsafe {
            let _: () = msg_send![w, performClose: NIL];
        }
        assert_eq!(
            checks.get(),
            4,
            "native performClose must use the same guard exactly once"
        );
        assert_eq!(closed.get(), 0);
        assert!(active(w));
        assert_input(&cx, window, &input, TEXT);
        eprintln!(
            "PASS vetoed Cmd-W, native menu, request_close, NSWindow.performClose: same guard exactly once each, text/focus retained"
        );

        invoke("Window", "Minimize");
        poll(&cx, "actual NSWindow miniaturized flag", || unsafe {
            let b: BOOL = msg_send![w, isMiniaturized];
            b == YES
        })
        .await;
        assert_eq!(checks.get(), 4);
        assert_eq!(closed.get(), 0);
        eprintln!("PASS production Minimize -> native isMiniaturized");
        // Do not reactivate after minimizing. Native close needs no forced focus.
        allow.set(true);
        unsafe {
            let _: () = msg_send![w, performClose: NIL];
        }
        poll(&cx, "owned window removal", || closed.get() == 1).await;
        assert_eq!(checks.get(), 5);
        assert!(cx.update(|cx| cx.windows().is_empty()));
        unsafe {
            let _: () = msg_send![w, release];
        }
        eprintln!(
            "PASS allowed native performClose removes only owned window, same guard once; explicit quit policy remains host-owned"
        );
        cx.update(|cx| cx.quit());
    }
    pub fn run() {
        std::panic::set_hook(Box::new(|p| {
            eprintln!(
                "FAIL native-os: {p}\n{}",
                std::backtrace::Backtrace::force_capture()
            );
            std::process::exit(1);
        }));
        // Independent watchdog also bounds hangs in AppKit startup or delegate reentry.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(30));
            eprintln!("FAIL native-os: process watchdog (30s)");
            std::process::exit(2);
        });
        eprintln!("START native-os real Application");
        gpui_kit::application().with_assets(Assets).run(|cx| {
            eprintln!("START AppKit launch callback");
            init(cx, false);
            native::install_window_commands(cx);
            cx.on_action(|_: &SafeQuit, _| {
                eprintln!("BLOCKED native-os: test interrupted by its own Quit Test action");
                std::process::exit(2);
            });
            cx.set_menus(vec![
                Menu::new("Native OS Test").items([MenuItem::action("Quit Test", SafeQuit)]),
                native::edit_menu(),
                native::window_menu(),
            ]);
            let title = format!("Tau native-os owned {}", std::process::id());
            let checks = Rc::new(Cell::new(0));
            let allow = Rc::new(Cell::new(false));
            let closed = Rc::new(Cell::new(0));
            let (c, a) = (checks.clone(), allow.clone());
            let mut input = None;
            let window = cx
                .open_window(WindowOptions::default(), |w, cx| {
                    w.set_window_title(&title);
                    w.on_window_should_close(cx, move |_, _| {
                        c.set(c.get() + 1);
                        a.get()
                    });
                    let state = cx.new(|cx| InputState::new(w, cx));
                    state.update(cx, |s, cx| s.focus(w, cx));
                    input = Some(state.clone());
                    let host = cx.new(|_| Host { input: state });
                    cx.new(|cx| Root::new(host, w, cx))
                })
                .expect("open owned synthetic window");
            // Same one-time startup activation as the production prototype; never refocus after actions.
            cx.activate(true);
            let count = closed.clone();
            cx.on_window_closed(move |_, _| count.set(count.get() + 1))
                .detach();
            cx.spawn(async move |cx| {
                exercise(
                    cx.clone(),
                    window.into(),
                    input.unwrap(),
                    title,
                    checks,
                    allow,
                    closed,
                )
                .await
            })
            .detach();
        });
    }
}
#[cfg(target_os = "macos")]
fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--owned-child") {
        macos::run();
        return Ok(());
    }
    // HIToolbox scans the executable's directory for bundle resources. Cargo's
    // large deps directory can stall startup before any AppKit assertions run.
    let directory = tempfile::Builder::new()
        .prefix("tau-native-os-")
        .tempdir()?;
    let executable = directory.path().join("native-os");
    std::fs::copy(std::env::current_exe()?, &executable)?;
    let status = std::process::Command::new(&executable)
        .arg("--owned-child")
        .status()?;
    let code = status.code().unwrap_or(1);
    drop(directory);
    std::process::exit(code);
}
