//! Opt-in AppKit host acceptance, with owned synthetic editors only.
//! No sessions, runtime, clipboard, AX, global input, Dock manipulation, or user files.
#![cfg(feature = "native-os-tests")]
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native-host requires macOS and an unlocked WindowServer session");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod macos {
    use gpui_kit::{
        AnyWindowHandle, AppContext, AsyncApp, Context, Entity, Focusable, IntoElement,
        KeyDownEvent, Keystroke, PlatformInput, Render, Window, WindowOptions,
        base::{Input, Root, input::InputState},
        div,
        prelude::*,
    };
    use objc::{
        class, msg_send,
        runtime::{BOOL, Object, YES},
        sel, sel_impl,
    };
    use std::{
        cell::{Cell, RefCell},
        ffi::CStr,
        rc::Rc,
        time::{Duration, Instant},
    };
    use tau_gpui::{
        components::{Assets, init},
        native::{self, HostCallbacks, Intent, LifecycleCallbacks, NativeHost, Outcome, Request},
    };

    type Id = *mut Object;
    const NIL: Id = std::ptr::null_mut();
    const DRAFT: &str = "Owned host café 😀 第二行";
    const COMPLETE: &str = "PASS native-host completion-gated cleanup once";
    struct Editor {
        input: Entity<InputState>,
    }
    impl Render for Editor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(Input::new(&self.input))
        }
    }
    #[derive(Default)]
    struct Observed {
        requests: RefCell<Vec<Request>>,
        settled: RefCell<Vec<(Request, Outcome)>>,
        editors: RefCell<Vec<Entity<InputState>>>,
        created: Cell<usize>,
        sessions: Cell<usize>,
        cleanup: Cell<usize>,
        complete: Cell<bool>,
    }
    unsafe fn app() -> Id {
        unsafe { msg_send![class!(NSApplication), sharedApplication] }
    }
    unsafe fn string(s: Id) -> String {
        unsafe {
            let p: *const std::ffi::c_char = msg_send![s, UTF8String];
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
    fn title(index: usize) -> String {
        format!("Tau native-host owned {} {index}", std::process::id())
    }
    fn owned_window(index: usize) -> Id {
        unsafe {
            let windows: Id = msg_send![app(), windows];
            let n: usize = msg_send![windows, count];
            for i in 0..n {
                let w: Id = msg_send![windows, objectAtIndex: i];
                let t: Id = msg_send![w, title];
                if string(t) == title(index) {
                    return w;
                }
            }
            NIL
        }
    }
    fn active(w: Id) -> bool {
        assert!(!w.is_null());
        unsafe {
            let a: BOOL = msg_send![app(), isActive];
            let k: BOOL = msg_send![w, isKeyWindow];
            let v: BOOL = msg_send![w, isVisible];
            a == YES && k == YES && v == YES
        }
    }
    static MINIATURIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    extern "C" fn did_miniaturize(_: &Object, _: objc::runtime::Sel, _: Id) {
        MINIATURIZED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn observe_miniaturization(w: Id) -> Id {
        unsafe {
            let mut class =
                objc::declare::ClassDecl::new("TauHostMiniaturizationObserver", class!(NSObject))
                    .unwrap();
            class.add_method(
                sel!(didMiniaturize:),
                did_miniaturize as extern "C" fn(&Object, objc::runtime::Sel, Id),
            );
            let observer: Id = msg_send![class.register(), new];
            let center: Id = msg_send![class!(NSNotificationCenter), defaultCenter];
            let name: Id = msg_send![class!(NSString), stringWithUTF8String: c"NSWindowDidMiniaturizeNotification".as_ptr()];
            let _: () = msg_send![center, addObserver: observer selector: sel!(didMiniaturize:) name: name object: w];
            observer
        }
    }
    fn stop_observing(observer: Id) {
        unsafe {
            let center: Id = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: observer];
            let _: () = msg_send![observer, release];
        }
    }
    fn minimized(w: Id) -> bool {
        unsafe {
            let b: BOOL = msg_send![w, isMiniaturized];
            b == YES
        }
    }
    fn invoke(menu_name: &str, name: &str) {
        unsafe {
            let main: Id = msg_send![app(), mainMenu];
            let n: isize = msg_send![main, numberOfItems];
            for i in 0..n {
                let item: Id = msg_send![main, itemAtIndex: i];
                let menu: Id = msg_send![item, submenu];
                if menu.is_null() {
                    continue;
                }
                let t: Id = msg_send![menu, title];
                if string(t) != menu_name {
                    continue;
                }
                let _: () = msg_send![menu, update];
                let count: isize = msg_send![menu, numberOfItems];
                for j in 0..count {
                    let item: Id = msg_send![menu, itemAtIndex: j];
                    let t: Id = msg_send![item, title];
                    if string(t) != name {
                        continue;
                    }
                    let enabled: BOOL = msg_send![item, isEnabled];
                    assert_eq!(enabled, YES, "native {menu_name}/{name} disabled");
                    if name == "Quit Host Test" {
                        let key: Id = msg_send![item, keyEquivalent];
                        let mask: usize = msg_send![item, keyEquivalentModifierMask];
                        assert_eq!(string(key), "q");
                        assert_eq!(mask, 1 << 20);
                    }
                    let _: () = msg_send![menu, performActionForItemAtIndex: j];
                    return;
                }
            }
            panic!("missing native menu {menu_name}/{name}");
        }
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NativePoint {
        x: f64,
        y: f64,
    }
    // NSPoint/CGPoint is two CGFloat doubles on supported 64-bit macOS hosts.
    unsafe impl objc::Encode for NativePoint {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
        }
    }
    fn native_cmd_q() {
        unsafe {
            let q: Id = msg_send![class!(NSString), stringWithUTF8String: c"q".as_ptr()];
            let event: Id = msg_send![class!(NSEvent), keyEventWithType: 10usize
                location: NativePoint { x: 0.0, y: 0.0 }
                modifierFlags: 1usize << 20 timestamp: 0.0f64 windowNumber: 0isize
                context: NIL characters: q charactersIgnoringModifiers: q
                isARepeat: 0i8 keyCode: 12u16];
            assert!(!event.is_null());
            let menu: Id = msg_send![app(), mainMenu];
            let handled: BOOL = msg_send![menu, performKeyEquivalent: event];
            assert_eq!(handled, YES, "own native Cmd-Q key equivalent not handled");
        }
    }
    fn terminate() {
        unsafe {
            let _: () = msg_send![app(), terminate: NIL];
        }
    }
    fn delegate_gate() -> usize {
        unsafe {
            let application = app();
            let delegate: Id = msg_send![application, delegate];
            assert!(!delegate.is_null());
            msg_send![delegate, applicationShouldTerminate: application]
        }
    }
    fn reopen() {
        unsafe {
            let application = app();
            let delegate: Id = msg_send![application, delegate];
            let result: BOOL = msg_send![delegate, applicationShouldHandleReopen: application hasVisibleWindows: 0i8];
            assert_eq!(result, YES);
        }
    }
    fn key(cx: &AsyncApp, target: AnyWindowHandle, binding: &str) {
        cx.update(|cx| {
            target
                .update(cx, |_, w, cx| {
                    w.dispatch_event(
                        PlatformInput::KeyDown(KeyDownEvent {
                            keystroke: Keystroke::parse(binding).unwrap(),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                })
                .unwrap()
        });
    }
    async fn poll(cx: &AsyncApp, label: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            if Instant::now() >= deadline {
                for index in 1..=2 {
                    let w = owned_window(index);
                    if !w.is_null() {
                        eprintln!(
                            "OWNED {index}: active/key/visible={} minimized={}",
                            active(w),
                            minimized(w)
                        );
                    }
                }
                panic!("blocked or failed: {label} (5s deadline)");
            }
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
        }
    }
    async fn request(cx: &AsyncApp, state: &Observed, count: usize) -> Request {
        poll(cx, "observed request callback", || {
            state.requests.borrow().len() >= count
        })
        .await;
        assert_eq!(state.requests.borrow().len(), count);
        state.requests.borrow()[count - 1]
    }
    fn retained(cx: &AsyncApp, target: AnyWindowHandle, state: &Observed, index: usize) {
        assert_eq!(state.cleanup.get(), 0, "veto called cleanup");
        assert!(
            active(owned_window(index)),
            "veto lost native key/visible/active state"
        );
        cx.update(|cx| {
            target
                .update(cx, |_, w, cx| {
                    let input = state.editors.borrow()[index - 1].clone();
                    assert_eq!(input.read(cx).value().as_ref(), DRAFT);
                    assert!(
                        input.read(cx).focus_handle(cx).is_focused(w),
                        "veto lost editor focus"
                    );
                })
                .unwrap()
        });
    }
    async fn exercise(
        cx: AsyncApp,
        host: NativeHost,
        state: Rc<Observed>,
        first: AnyWindowHandle,
        reveal: bool,
    ) {
        poll(&cx, "startup activation and own key window", || {
            let w = owned_window(1);
            !w.is_null() && active(w)
        })
        .await;
        poll(&cx, "initial focused responder", || {
            cx.update(|cx| {
                first
                    .update(cx, |_, w, cx| {
                        state.editors.borrow()[0]
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(w)
                    })
                    .unwrap()
            })
        })
        .await;
        retained(&cx, first, &state, 1);
        eprintln!("PASS owned startup (one application activation only)");

        if reveal {
            let observer = observe_miniaturization(owned_window(1));
            invoke("Window", "Minimize");
            poll(&cx, "owned native miniaturization", || {
                minimized(owned_window(1))
            })
            .await;
            poll(&cx, "owned NSWindowDidMiniaturize notification", || {
                MINIATURIZED.load(std::sync::atomic::Ordering::SeqCst)
            })
            .await;
            stop_observing(observer);
            reopen();
            poll(
                &cx,
                "default Window activation reveals minimized owned root",
                || !minimized(owned_window(1)) && active(owned_window(1)),
            )
            .await;
            assert_eq!(state.created.get(), 1);
            retained(&cx, first, &state, 1);
            eprintln!("PASS standard reveal retains minimized editor identity/draft/focus");
        } else {
            invoke("File", "New Session");
            poll(&cx, "native New Session callback", || {
                state.sessions.get() == 1
            })
            .await;
            key(&cx, first, "cmd-n");
            poll(&cx, "local Cmd-N callback", || state.sessions.get() == 2).await;
            retained(&cx, first, &state, 1);
            unsafe {
                let _: () = msg_send![owned_window(1), performClose: NIL];
            }
            let close = request(&cx, &state, 1).await;
            assert_eq!(close.intent(), Intent::Close(first));
            retained(&cx, first, &state, 1);
            cx.update(|cx| assert!(host.lifecycle().resolve(close, false, cx)));
            retained(&cx, first, &state, 1);
            // The following close is a new observed request, not a sleep-driven decision.
            invoke("File", "Close Window");
            let close = request(&cx, &state, 2).await;
            cx.update(|cx| assert!(host.lifecycle().resolve(close, true, cx)));
            poll(&cx, "last owned window removed without app exit", || {
                cx.update(|cx| cx.windows().is_empty())
            })
            .await;
            assert_eq!(state.cleanup.get(), 0);
            reopen();
            poll(&cx, "delegate reopen factory", || state.created.get() == 2).await;
            poll(&cx, "reopened owned window activation", || {
                let w = owned_window(2);
                !w.is_null() && active(w)
            })
            .await;
            eprintln!(
                "PASS native close cancel/approve, explicit last-window lifetime, delegate reopen factory"
            );
        }
        let target = cx.update(|_| host.lifecycle().windows()[0]);
        let index = state.created.get();
        let base = state.requests.borrow().len();
        // This is the actual NSApplication terminate:, not TestPlatform or App::quit.
        terminate();
        let vetoed = request(&cx, &state, base + 1).await;
        assert_eq!(vetoed.intent(), Intent::Quit);
        retained(&cx, target, &state, index);
        terminate();
        assert_eq!(delegate_gate(), 0, "own-process Dock-style gate must veto");
        invoke("Host Test", "Quit Host Test");
        native_cmd_q();
        key(&cx, target, "cmd-q");
        // Native and GPUI dispatch flush their callbacks; the pending identity must persist.
        assert_eq!(state.requests.borrow().len(), base + 1);
        assert!(host.lifecycle().is_pending(vetoed));
        retained(&cx, target, &state, index);
        eprintln!(
            "PASS actual terminate veto, repeated native menu/native key equivalent/local Cmd-Q/direct delegate coalescing"
        );
        cx.update(|cx| assert!(host.lifecycle().resolve(vetoed, false, cx)));
        poll(&cx, "observed quit cancellation", || {
            state
                .settled
                .borrow()
                .contains(&(vetoed, Outcome::Cancelled))
        })
        .await;
        retained(&cx, target, &state, index);
        assert_eq!(delegate_gate(), 0);
        let approved = request(&cx, &state, base + 2).await;
        assert_ne!(approved, vetoed);
        terminate();
        assert_eq!(state.requests.borrow().len(), base + 2);
        retained(&cx, target, &state, index);
        cx.update(|cx| assert!(!host.lifecycle().resolve(vetoed, true, cx)));
        eprintln!("PASS cancel then distinct own-delegate request; stale approval rejected");
        // Completion is gated in on_app_quit, including the approval observer.
        state.complete.set(true);
        cx.update(|cx| assert!(host.lifecycle().resolve(approved, true, cx)));
        // AppKit must terminate the child; returning from this task is not success.
    }
    pub fn run(reveal: bool) {
        std::panic::set_hook(Box::new(|p| {
            eprintln!(
                "FAIL native-host: {p}\n{}",
                std::backtrace::Backtrace::force_capture()
            );
            std::process::exit(1);
        }));
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(35));
            eprintln!("FAIL native-host watchdog (35s)");
            std::process::exit(2);
        });
        let state = Rc::new(Observed::default());
        let completed = state.clone();
        gpui_kit::application().with_assets(Assets).run(move |cx| {
            init(cx, false);
            let on_quit = state.clone();
            cx.on_app_quit(move |_| {
                assert!(
                    on_quit.complete.get(),
                    "premature cleanup before native-host completion"
                );
                assert_eq!(
                    on_quit.cleanup.replace(on_quit.cleanup.get() + 1),
                    0,
                    "duplicate cleanup"
                );
                assert_eq!(
                    on_quit
                        .settled
                        .borrow()
                        .iter()
                        .filter(|(r, o)| r.intent() == Intent::Quit && *o == Outcome::Approved)
                        .count(),
                    1
                );
                eprintln!("{COMPLETE}");
                std::future::ready(())
            })
            .detach();
            let requested = state.clone();
            let settled = state.clone();
            let factory = state.clone();
            let sessions = state.clone();
            let host = NativeHost::install(
                cx,
                "Host Test",
                Duration::from_secs(30),
                HostCallbacks {
                    lifecycle: LifecycleCallbacks {
                        requested: Rc::new(move |r, _| {
                            eprintln!("OBSERVED request {:?}", r.intent());
                            requested.requests.borrow_mut().push(r);
                        }),
                        settled: Rc::new(move |r, o, _| {
                            eprintln!("OBSERVED outcome {o:?}");
                            settled.settled.borrow_mut().push((r, o));
                        }),
                    },
                    new_session: Rc::new(move |_| {
                        sessions.sessions.set(sessions.sessions.get() + 1)
                    }),
                    create_window: Rc::new(move |cx| {
                        let index = factory.created.get() + 1;
                        factory.created.set(index);
                        cx.open_window(WindowOptions::default(), |w, cx| {
                            w.set_window_title(&title(index));
                            let input = cx.new(|cx| InputState::new(w, cx));
                            input.update(cx, |s, cx| {
                                s.set_value(DRAFT, w, cx);
                                s.focus(w, cx);
                            });
                            factory.editors.borrow_mut().push(input.clone());
                            let editor = cx.new(|_| Editor { input });
                            cx.new(|cx| Root::new(editor, w, cx))
                        })
                        .map(Into::into)
                    }),
                    reveal_window: Rc::new(|target, cx| native::reveal_window(target, cx).unwrap()),
                    window_failed: Rc::new(|_| panic!("owned factory failed")),
                },
            );
            let first = host.open_window(cx).expect("initial synthetic editor");
            cx.activate(true); // The only application activation; no focus/activation repair.
            cx.spawn(async move |cx| exercise(cx.clone(), host, state, first, reveal).await)
                .detach();
        });
        assert!(completed.complete.get(), "application returned prematurely");
        assert_eq!(completed.cleanup.get(), 1);
    }
}

#[cfg(target_os = "macos")]
fn main() -> std::io::Result<()> {
    if let Some(mode) = std::env::args()
        .nth(1)
        .filter(|a| a.starts_with("--owned-"))
    {
        assert!(mode == "--owned-termination" || mode == "--owned-reveal");
        macos::run(mode == "--owned-reveal");
        return Ok(());
    }
    // Copy this exact executable, never search Cargo targets. HIToolbox must not
    // scan the large deps directory for bundle resources during child startup.
    let directory = tempfile::Builder::new()
        .prefix("tau-native-host-")
        .tempdir()?;
    let executable = directory.path().join("native-host");
    std::fs::copy(std::env::current_exe()?, &executable)?;
    let mut failed = false;
    for scenario in ["termination", "reveal"] {
        eprintln!("START owned {scenario} child");
        let output = std::process::Command::new(&executable)
            .arg(format!("--owned-{scenario}"))
            .output()?;
        use std::io::Write;
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
        let marker = String::from_utf8_lossy(&output.stderr)
            .matches("PASS native-host completion-gated cleanup once")
            .count();
        if !output.status.success() || marker != 1 {
            eprintln!(
                "FAIL owned {scenario}: status={}, completion markers={marker}",
                output.status
            );
            failed = true;
        } else {
            eprintln!("PASS owned {scenario} child actually exited");
        }
    }
    std::process::exit(if failed { 1 } else { 0 });
}
