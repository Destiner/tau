//! Opt-in own-process NSAccessibility provider contract using a synthetic window.
//! No AXUIElement, trust checks, permissions, other apps, clipboard, or text AX getters.
#![cfg(feature = "native-os-tests")]
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native-accessibility requires macOS and an unlocked WindowServer session");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2.7 macros test the legacy cargo-clippy feature.
mod macos {
    use gpui_kit::{
        AnyWindowHandle, AppContext, AsyncApp, Context, Entity, FocusHandle, Focusable,
        IntoElement, Render, Subscription, Window, WindowOptions, accesskit,
        base::{
            FocusTrapElement, Input, Root,
            input::{InputState, Textarea, TextareaState},
        },
        div,
        prelude::*,
        px,
    };
    use objc::{
        class, msg_send,
        runtime::{BOOL, Object, Sel, YES},
        sel, sel_impl,
    };
    use std::{
        cell::Cell,
        collections::HashSet,
        ffi::CStr,
        rc::Rc,
        time::{Duration, Instant},
    };
    use tau_gpui_prototype::components::{Assets, init};

    type Id = *mut Object;
    const SINGLE: &str = "Owned readonly single";
    const MULTI: &str = "Owned multiline";
    const BARRIER: &str = "Owned focus barrier";
    const SINGLE_TEXT: &str = "synthetic-private-single";
    const MULTI_TEXT: &str = "synthetic-private\nmultiline";

    struct Host {
        single: Entity<InputState>,
        multi: Entity<TextareaState>,
        trap: bool,
        trap_focus: Option<FocusHandle>,
        barrier_focus: FocusHandle,
        receipts: Rc<Cell<usize>>,
        outside_focus_changes: Rc<Cell<usize>>,
        _outside_subscription: Subscription,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let focus = self.barrier_focus.clone();
            let outside = self.multi.read(cx).focus_handle(cx);
            let receipts = self.receipts.clone();
            let barrier = div()
                .id("focus-barrier")
                .role(accesskit::Role::Button)
                .aria_label(BARRIER)
                .track_focus(&self.barrier_focus)
                .on_a11y_action(accesskit::Action::Focus, move |_, w, cx| {
                    // Focus subscriptions are frame-based. Sample BEFORE moving focus
                    // too, so a queued escape cannot be hidden by this honest action.
                    assert!(
                        !outside.is_focused(w),
                        "outside editor focused before barrier"
                    );
                    focus.focus(w, cx);
                    receipts.set(receipts.get() + 1);
                })
                .child("Focus barrier");
            let inside = div()
                .id("inside")
                .when_some(self.trap_focus.as_ref(), |this, focus| {
                    this.track_focus(focus)
                })
                .w(px(320.))
                .child(Input::new(&self.single))
                .child(barrier);
            let inside = if self.trap {
                inside
                    .focus_trap("owned-trap", self.trap_focus.as_ref().unwrap())
                    .into_any_element()
            } else {
                inside.into_any_element()
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(inside)
                .child(div().w(px(320.)).child(Textarea::new(&self.multi)))
        }
    }

    // NSApplication/window/view and AccessKit nodes are exclusively this process's.
    // All ObjC calls are main-thread and OUTSIDE cx.update/window.update borrows.
    unsafe fn app() -> Id {
        unsafe { msg_send![class!(NSApplication), sharedApplication] }
    }
    unsafe fn text(s: Id) -> Option<String> {
        if s.is_null() {
            return None;
        }
        unsafe {
            let p: *const std::ffi::c_char = msg_send![s, UTF8String];
            assert!(!p.is_null());
            Some(CStr::from_ptr(p).to_string_lossy().into_owned())
        }
    }
    unsafe fn own_window(title: &str) -> Id {
        unsafe {
            let windows: Id = msg_send![app(), windows];
            let count: usize = msg_send![windows, count];
            for i in 0..count {
                let w: Id = msg_send![windows, objectAtIndex: i];
                let t: Id = msg_send![w, title];
                if text(t).as_deref() == Some(title) {
                    return w;
                }
            }
            std::ptr::null_mut()
        }
    }
    fn active(w: Id) -> bool {
        unsafe {
            let a: BOOL = msg_send![app(), isActive];
            let k: BOOL = msg_send![w, isKeyWindow];
            let v: BOOL = msg_send![w, isVisible];
            a == YES && k == YES && v == YES
        }
    }
    struct Retained(Id);
    impl Retained {
        unsafe fn new(id: Id) -> Self {
            assert!(!id.is_null());
            unsafe {
                let _: Id = msg_send![id, retain];
            }
            Self(id)
        }
    }
    impl Drop for Retained {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![self.0, release];
            }
        }
    }
    #[derive(Debug)]
    struct Metadata {
        title: Option<String>,
        role: Option<String>,
        subrole: Option<String>,
        enabled: bool,
        focused: bool,
        focus_allowed: bool,
    }
    unsafe fn optional_string(id: Id, selector: Sel) -> Option<String> {
        unsafe {
            let responds: BOOL = msg_send![id, respondsToSelector: selector];
            if responds != YES {
                return None;
            }
            let value: Id = msg_send![id, performSelector: selector];
            text(value)
        }
    }
    unsafe fn metadata(id: Id) -> Metadata {
        unsafe {
            let enabled: BOOL = msg_send![id, isAccessibilityEnabled];
            let focused: BOOL = msg_send![id, isAccessibilityFocused];
            let allowed: BOOL =
                msg_send![id, isAccessibilitySelectorAllowed: sel!(setAccessibilityFocused:)];
            Metadata {
                title: optional_string(id, sel!(accessibilityTitle)),
                role: optional_string(id, sel!(accessibilityRole)),
                subrole: optional_string(id, sel!(accessibilitySubrole)),
                enabled: enabled == YES,
                focused: focused == YES,
                focus_allowed: allowed == YES,
            }
        }
    }
    // Own native provider only. Bounds apply before expanding arrays, including cycles.
    // All raw children remain within the synchronous autorelease pool; matches retained.
    fn nodes(w: Id) -> Vec<(Retained, Metadata)> {
        unsafe {
            let pool: Id = msg_send![class!(NSAutoreleasePool), new];
            let view: Id = msg_send![w, contentView];
            assert!(!view.is_null());
            assert!(
                (*view).class().name().starts_with("AccessKitSubclassOf"),
                "contentView is not the actual AccessKit subclass"
            );
            let children: Id = msg_send![view, accessibilityChildren];
            let mut stack = Vec::new();
            push_children(children, 0, &mut stack);
            let mut seen = HashSet::new();
            let mut result = Vec::new();
            while let Some((id, depth)) = stack.pop() {
                assert!(depth <= 8, "owned provider depth limit");
                assert!(seen.insert(id as usize), "owned provider cycle/duplicate");
                assert!(seen.len() <= 32, "owned provider node limit");
                assert_eq!(
                    (*id).class().name(),
                    "AccessKitNode",
                    "unexpected provider object"
                );
                let role = optional_string(id, sel!(accessibilityRole));
                if matches!(
                    role.as_deref(),
                    Some("AXTextField" | "AXTextArea" | "AXButton")
                ) {
                    let m = metadata(id);
                    assert!(
                        matches!(m.title.as_deref(), Some(SINGLE | MULTI | BARRIER)),
                        "unexpected editor name (content not logged)"
                    );
                    result.push((Retained::new(id), m));
                }
                let next: Id = msg_send![id, accessibilityChildren];
                push_children(next, depth + 1, &mut stack);
                assert!(
                    seen.len() + stack.len() <= 32,
                    "owned provider traversal budget"
                );
            }
            let _: () = msg_send![pool, drain];
            result
        }
    }
    unsafe fn push_children(array: Id, depth: usize, stack: &mut Vec<(Id, usize)>) {
        if array.is_null() {
            return;
        }
        unsafe {
            let count: usize = msg_send![array, count];
            assert!(
                count <= 32 && stack.len() + count <= 32,
                "owned provider array limit"
            );
            for i in 0..count {
                let child: Id = msg_send![array, objectAtIndex: i];
                assert!(!child.is_null());
                stack.push((child, depth));
            }
        }
    }
    fn named(w: Id, name: &str) -> Option<(Retained, Metadata)> {
        let mut matches = nodes(w)
            .into_iter()
            .filter(|(_, m)| m.title.as_deref() == Some(name));
        let first = matches.next();
        assert!(matches.next().is_none(), "duplicate named editor node");
        first
    }
    async fn poll(cx: &AsyncApp, label: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(Instant::now() < deadline, "BLOCKED/FAIL {label} (5s)");
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
        }
    }
    fn guards(
        cx: &AsyncApp,
        window: AnyWindowHandle,
        host: &Entity<Host>,
        single_focus: Option<bool>,
    ) {
        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    let h = host.read(cx);
                    // The fixture guard alone reads content/selection; the AX observer never does.
                    assert_eq!(h.single.read(cx).value().as_ref(), SINGLE_TEXT);
                    assert_eq!(h.multi.read(cx).value().as_ref(), MULTI_TEXT);
                    assert_eq!(
                        h.single.read(cx).selected_range(),
                        SINGLE_TEXT.len()..SINGLE_TEXT.len()
                    );
                    assert_eq!(h.multi.read(cx).selected_range(), 0..0);
                    assert!(h.single.read(cx).context_menu_capabilities().is_readonly());
                    assert_eq!(
                        h.single.read(cx).focus_handle(cx).is_focused(w),
                        single_focus == Some(true)
                    );
                    assert_eq!(
                        h.multi.read(cx).focus_handle(cx).is_focused(w),
                        single_focus == Some(false)
                    );
                    assert_eq!(h.barrier_focus.is_focused(w), single_focus.is_none());
                    assert!(w.is_window_active(), "lost actual native activation");
                })
                .unwrap()
        });
    }
    async fn rejected_then_barrier(
        cx: &AsyncApp,
        window: AnyWindowHandle,
        host: &Entity<Host>,
        w: Id,
        target: &Retained,
    ) {
        let (receipts, outside_changes) = cx.update(|cx| {
            let h = host.read(cx);
            (h.receipts.clone(), h.outside_focus_changes.clone())
        });
        let before_receipts = receipts.get();
        let before_changes = outside_changes.get();
        let (barrier, m) = named(w, BARRIER).expect("real barrier node");
        assert_eq!(m.role.as_deref(), Some("AXButton"));
        assert!(m.enabled && m.focus_allowed && !m.focused);
        set_focus(target);
        // Both setters synchronously enqueue into the SAME production FIFO.
        // The normal barrier handler samples outside focus before focusing itself.
        set_focus(&barrier);
        poll(
            cx,
            "real FIFO barrier receipt and native/handle focus",
            || {
                assert!(active(w), "lost activation; no forced recovery");
                assert_eq!(
                    outside_changes.get(),
                    before_changes,
                    "transient outside focus"
                );
                receipts.get() == before_receipts + 1
                    && named(w, BARRIER).unwrap().1.focused
                    && cx.update(|cx| {
                        window
                            .update(cx, |_, w, cx| host.read(cx).barrier_focus.is_focused(w))
                            .unwrap()
                    })
            },
        )
        .await;
        assert_eq!(outside_changes.get(), before_changes);
        guards(cx, window, host, None);
        assert!(!named(w, MULTI).unwrap().1.focused);
        eprintln!("OBSERVED real FIFO Focus barrier receipt; outside-focus counter unchanged");
    }
    fn set_focus(node: &Retained) {
        unsafe {
            let _: () = msg_send![node.0, setAccessibilityFocused: YES];
        }
    }
    async fn ax_focus(
        cx: &AsyncApp,
        window: AnyWindowHandle,
        host: &Entity<Host>,
        w: Id,
        name: &str,
    ) {
        let (node, m) = named(w, name).expect("named owned node");
        assert!(m.enabled && m.focus_allowed);
        set_focus(&node);
        poll(cx, "native setter -> existing GPUI focus handle", || {
            let native = named(w, name).unwrap().1.focused;
            let handle = cx.update(|cx| {
                window
                    .update(cx, |_, w, cx| {
                        let h = host.read(cx);
                        if name == SINGLE {
                            h.single.read(cx).focus_handle(cx).is_focused(w)
                        } else {
                            h.multi.read(cx).focus_handle(cx).is_focused(w)
                        }
                    })
                    .unwrap()
            });
            native && handle
        })
        .await;
        assert!(active(w));
        guards(cx, window, host, Some(name == SINGLE));
    }
    async fn exercise(cx: AsyncApp, window: AnyWindowHandle, host: Entity<Host>, title: String) {
        poll(&cx, "owned native window", || unsafe {
            !own_window(&title).is_null()
        })
        .await;
        let w = unsafe { Retained::new(own_window(&title)) };
        poll(&cx, "one-time startup activation", || active(w.0)).await;
        poll(&cx, "initial existing handle", || {
            cx.update(|cx| {
                window
                    .update(cx, |_, w, cx| {
                        host.read(cx).single.read(cx).focus_handle(cx).is_focused(w)
                    })
                    .unwrap()
            })
        })
        .await;
        let before_getter =
            cx.update(|cx| window.update(cx, |_, w, _| w.is_a11y_enabled()).unwrap());
        eprintln!("BEFORE own contentView getter: a11y_active={before_getter}");
        // Inactive adapters activate through this getter; AppKit may already have
        // queried it at startup. Do not reset activation to manufacture causality.
        poll(
            &cx,
            "getter activates actual AccessKit provider and both editor nodes",
            || {
                let found = nodes(w.0);
                found.len() == 3
                    && found
                        .iter()
                        .any(|(_, m)| m.title.as_deref() == Some(SINGLE))
                    && found.iter().any(|(_, m)| m.title.as_deref() == Some(MULTI))
            },
        )
        .await;
        assert!(cx.update(|cx| window.update(cx, |_, w, _| w.is_a11y_enabled()).unwrap()));
        eprintln!("AFTER own contentView getter/poll: a11y_active=true; both real nodes present");
        let (single, s) = named(w.0, SINGLE).unwrap();
        let (multi, m) = named(w.0, MULTI).unwrap();
        eprintln!("OBSERVED actual owned provider single={s:?} multiline={m:?}");
        assert_eq!(s.role.as_deref(), Some("AXTextField"));
        assert_eq!(s.subrole.as_deref(), Some("AXSecureTextField"));
        assert!(s.enabled && s.focused && s.focus_allowed);
        assert_eq!(m.role.as_deref(), Some("AXTextArea"));
        assert!(!m.enabled && !m.focused && !m.focus_allowed);
        guards(&cx, window, &host, Some(true));
        // The pinned provider checks is_focusable synchronously before do_action.
        set_focus(&multi);
        guards(&cx, window, &host, Some(true));
        assert!(named(w.0, SINGLE).unwrap().1.focused);
        assert!(!named(w.0, MULTI).unwrap().1.focused);
        eprintln!(
            "PASS real provider roles/title/secure subrole; disabled focus inert; content guards"
        );

        cx.update(|cx| {
            window
                .update(cx, |_, _, cx| {
                    host.read(cx)
                        .multi
                        .clone()
                        .update(cx, |s, cx| s.set_disabled(false, cx));
                })
                .unwrap()
        });
        poll(&cx, "enabled native metadata", || {
            named(w.0, MULTI).is_some_and(|(_, m)| m.enabled && m.focus_allowed)
        })
        .await;
        assert!(cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    gpui_kit::base::active_focus_trap(w, cx).is_some()
                })
                .unwrap()
        }));
        // Allowed by provider, rejected by live InputState trap guard.
        rejected_then_barrier(&cx, window, &host, w.0, &multi).await;
        ax_focus(&cx, window, &host, w.0, SINGLE).await;
        eprintln!("PASS actual AX Focus cannot escape active trap");

        // Remove fixture trap without moving focus or activating the app.
        cx.update(|cx| {
            host.update(cx, |h, cx| {
                h.trap = false;
                h.trap_focus = None; // Manager retains weak handles: drop the fixture owner too.
                cx.notify();
            })
        });
        poll(&cx, "trap removal rendered", || {
            cx.update(|cx| {
                window
                    .update(cx, |_, w, cx| {
                        gpui_kit::base::active_focus_trap(w, cx).is_none()
                    })
                    .unwrap()
            })
        })
        .await;
        let outside_before = cx.update(|cx| host.read(cx).outside_focus_changes.get());
        ax_focus(&cx, window, &host, w.0, MULTI).await;
        assert_eq!(
            cx.update(|cx| host.read(cx).outside_focus_changes.get()),
            outside_before + 1,
            "real positive transfer must exercise the focus subscription"
        );
        cx.update(|cx| {
            window
                .update(cx, |_, w, cx| {
                    host.read(cx)
                        .single
                        .clone()
                        .update(cx, |s, cx| s.set_masked(false, w, cx));
                })
                .unwrap()
        });
        poll(&cx, "single unmasked native subrole", || {
            named(w.0, SINGLE).is_some_and(|(_, m)| {
                m.role.as_deref() == Some("AXTextField")
                    && m.subrole.as_deref() != Some("AXSecureTextField")
            })
        })
        .await;
        ax_focus(&cx, window, &host, w.0, SINGLE).await;
        eprintln!("PASS native Focus transfers to existing multiline and readonly single handles");

        // Retain a formerly enabled node, disable state, then deliver its REAL setter
        // before forcing/awaiting a redraw: editor live-state guard must reject it too.
        cx.update(|cx| {
            window
                .update(cx, |_, _, cx| {
                    host.read(cx)
                        .multi
                        .clone()
                        .update(cx, |s, cx| s.set_disabled(true, cx));
                })
                .unwrap()
        });
        rejected_then_barrier(&cx, window, &host, w.0, &multi).await;
        ax_focus(&cx, window, &host, w.0, SINGLE).await;
        poll(&cx, "disabled metadata caught up", || {
            named(w.0, MULTI).is_some_and(|(_, m)| !m.enabled && !m.focus_allowed)
        })
        .await;
        eprintln!(
            "PASS retained native setter cannot focus newly disabled state (provider may already be current)"
        );
        drop(single);
        drop(multi);
    }
    pub fn run() {
        std::panic::set_hook(Box::new(|p| {
            eprintln!("FAIL own-provider: {p}");
            std::process::exit(1);
        }));
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(30));
            eprintln!("BLOCKED own-provider watchdog");
            std::process::exit(2);
        });
        let completed = Rc::new(Cell::new(false));
        let result = completed.clone();
        gpui_kit::application().with_assets(Assets).run(move |cx| {
            let finished = result.clone();
            cx.on_app_quit(move |_| {
                assert!(
                    finished.get(),
                    "native-accessibility quit before completion"
                );
                std::future::ready(())
            })
            .detach();
            init(cx, false);
            let title = format!("Tau own-provider synthetic {}", std::process::id());
            let mut host = None;
            let window = cx
                .open_window(WindowOptions::default(), |w, cx| {
                    w.set_window_title(&title);
                    let single = cx.new(|cx| {
                        InputState::new(w, cx)
                            .masked(true)
                            .accessibility_label(SINGLE)
                    });
                    single.update(cx, |s, cx| {
                        s.set_value(SINGLE_TEXT, w, cx);
                        s.set_readonly(true, cx);
                        s.focus(w, cx); // ONLY initial startup focus, never used by test actions.
                    });
                    let multi = cx.new(|cx| TextareaState::new(w, cx).accessibility_label(MULTI));
                    multi.update(cx, |s, cx| {
                        s.set_value(MULTI_TEXT, w, cx);
                        s.set_disabled(true, cx);
                    });
                    let h = cx.new(|cx| {
                        let outside_focus_changes = Rc::new(Cell::new(0));
                        let changes = outside_focus_changes.clone();
                        let outside = multi.read(cx).focus_handle(cx);
                        let subscription = cx.on_focus(&outside, w, move |_, _, _| {
                            changes.set(changes.get() + 1);
                        });
                        Host {
                            single,
                            multi,
                            trap: true,
                            trap_focus: Some(cx.focus_handle()),
                            barrier_focus: cx.focus_handle(),
                            receipts: Rc::new(Cell::new(0)),
                            outside_focus_changes,
                            _outside_subscription: subscription,
                        }
                    });
                    host = Some(h.clone());
                    cx.new(|cx| Root::new(h, w, cx))
                })
                .expect("owned synthetic window");
            cx.activate(true);
            cx.spawn(async move |cx| {
                exercise(cx.clone(), window.into(), host.unwrap(), title).await;
                result.set(true);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
        assert!(
            completed.get(),
            "native-accessibility interrupted before completion"
        );
    }
}
#[cfg(target_os = "macos")]
fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--owned-child") {
        macos::run();
        return Ok(());
    }
    // Keep HIToolbox's bundle-resource scan out of Cargo's large deps directory.
    let directory = tempfile::Builder::new()
        .prefix("tau-own-provider-")
        .tempdir()?;
    let executable = directory.path().join("own-provider");
    std::fs::copy(std::env::current_exe()?, &executable)?;
    let status = std::process::Command::new(&executable)
        .arg("--owned-child")
        .status()?;
    let code = status.code().unwrap_or(1);
    drop(directory);
    std::process::exit(code);
}
