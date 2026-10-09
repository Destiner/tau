//! Host-only coordination. No runtime, persistence, or confirmation policy lives here.
use gpui_kit::{AnyWindowHandle, App, BackgroundExecutor, Global, Window};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intent {
    Close(AnyWindowHandle),
    Quit,
}

/// Opaque identity: a decision is valid only for this host and this request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    id: u64,
    intent: Intent,
    deadline: Instant,
}
impl Request {
    pub fn intent(self) -> Intent {
        self.intent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    Cancelled,
    Expired,
    Superseded,
    TargetClosed,
}

/// Confirmation and any pre-exit shutdown belong to the injected host callback.
/// It may present an existing NativeDialog and resolve on its Submitted/Cancelled
/// events. Resolve Approved only after required shutdown/persistence succeeds.
pub struct LifecycleCallbacks {
    pub requested: Rc<RequestHandler>,
    pub settled: Rc<RequestObserver>,
}

pub type RequestHandler = dyn Fn(Request, &mut App);
pub type RequestObserver = dyn Fn(Request, Outcome, &mut App);

struct State {
    pending: HashMap<Intent, Request>,
    close_authorized: HashSet<AnyWindowHandle>,
    quit_authorized: bool,
    exiting: bool,
    windows: Vec<AnyWindowHandle>,
    callbacks: LifecycleCallbacks,
    timeout: Duration,
    clock: BackgroundExecutor,
}

#[derive(Clone)]
pub struct Lifecycle(Rc<RefCell<State>>);
impl Global for Lifecycle {}

impl Lifecycle {
    /// Explicit, once-per-App ownership of the quit guard and window-close policy.
    /// Repeated installation returns the original host; callbacks are not replaced.
    pub fn install(cx: &mut App, timeout: Duration, callbacks: LifecycleCallbacks) -> Self {
        if cx.has_global::<Self>() {
            return cx.global::<Self>().clone();
        }
        let host = Self(Rc::new(RefCell::new(State {
            pending: HashMap::new(),
            close_authorized: HashSet::new(),
            quit_authorized: false,
            exiting: false,
            windows: Vec::new(),
            callbacks,
            timeout: timeout.max(Duration::from_millis(1)),
            clock: cx.background_executor().clone(),
        })));
        cx.set_global(host.clone());
        let gate = host.clone();
        cx.on_app_should_quit(move |cx| {
            let mut state = gate.0.borrow_mut();
            if state.quit_authorized {
                state.quit_authorized = false;
                return true;
            }
            drop(state);
            gate.request(Intent::Quit, cx);
            false
        });
        let observer = host.clone();
        cx.on_window_closed(move |cx, id| {
            let target = observer
                .0
                .borrow()
                .windows
                .iter()
                .copied()
                .find(|window| window.window_id() == id);
            if let Some(target) = target {
                let request = {
                    let mut state = observer.0.borrow_mut();
                    state.windows.retain(|w| *w != target);
                    state.close_authorized.remove(&target);
                    state.pending.remove(&Intent::Close(target))
                };
                if let Some(request) = request {
                    observer.settle(request, Outcome::TargetClosed, cx);
                }
            }
        })
        .detach();
        host
    }

    /// Attach the existing window/root; never reconstruct it on cancellation.
    /// This explicitly owns (replaces) this window's synchronous close guard.
    pub fn attach(&self, window: &mut Window, cx: &mut App) {
        let target = window.window_handle();
        if self.0.borrow().windows.contains(&target) {
            return;
        }
        self.0.borrow_mut().windows.push(target);
        let host = self.clone();
        window.on_window_should_close(cx, move |_, cx| {
            if host.0.borrow_mut().close_authorized.remove(&target) {
                return true;
            }
            host.request(Intent::Close(target), cx);
            false
        });
    }

    pub fn windows(&self) -> Vec<AnyWindowHandle> {
        self.0.borrow().windows.clone()
    }
    pub fn is_exiting(&self) -> bool {
        self.0.borrow().exiting
    }
    pub fn can_open_window(&self) -> bool {
        let state = self.0.borrow();
        !state.exiting && !state.pending.contains_key(&Intent::Quit)
    }

    fn request(&self, intent: Intent, cx: &mut App) {
        let expired = {
            let state = self.0.borrow();
            let now = state.clock.now();
            state
                .pending
                .values()
                .copied()
                .filter(|request| now >= request.deadline)
                .collect::<Vec<_>>()
        };
        for request in expired {
            self.finish(request, Outcome::Expired, cx);
        }
        let (request, superseded, timeout) = {
            let mut state = self.0.borrow_mut();
            if state.exiting
                || state.pending.contains_key(&intent)
                || (intent != Intent::Quit && state.pending.contains_key(&Intent::Quit))
            {
                return;
            }
            let Ok(id) = NEXT_REQUEST
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            else {
                return;
            };
            let superseded = if intent == Intent::Quit {
                state.close_authorized.clear();
                state
                    .pending
                    .drain()
                    .map(|(_, request)| request)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let request = Request {
                id: id + 1,
                intent,
                deadline: state.clock.now() + state.timeout,
            };
            state.pending.insert(intent, request);
            (request, superseded, state.timeout)
        };
        for old in superseded {
            self.settle(old, Outcome::Superseded, cx);
        }
        let host = self.clone();
        cx.defer(move |cx| {
            if host.is_pending(request) {
                let callback = host.0.borrow().callbacks.requested.clone();
                callback(request, cx);
            } else {
                host.finish(request, Outcome::Expired, cx);
            }
        });
        let host = self.clone();
        let timer = cx.background_executor().timer(timeout);
        cx.spawn(async move |cx| {
            timer.await;
            cx.update(|cx| host.finish(request, Outcome::Expired, cx));
        })
        .detach();
    }

    /// Check identity and the executor's monotonic deadline before another async phase.
    /// This read-only check does not deliver settlement; the timer or next decision does.
    pub fn is_pending(&self, request: Request) -> bool {
        let state = self.0.borrow();
        state.pending.get(&request.intent) == Some(&request) && state.clock.now() < request.deadline
    }

    /// Stale, duplicate, expired, and superseded decisions return false.
    /// Cancellation never closes a window, drops a root, or calls shutdown.
    pub fn resolve(&self, request: Request, approved: bool, cx: &mut App) -> bool {
        self.finish(
            request,
            if approved {
                Outcome::Approved
            } else {
                Outcome::Cancelled
            },
            cx,
        )
    }

    fn finish(&self, request: Request, outcome: Outcome, cx: &mut App) -> bool {
        let requested_outcome = outcome;
        let outcome = {
            let mut state = self.0.borrow_mut();
            if state.pending.get(&request.intent) != Some(&request) {
                return false;
            }
            let outcome = if state.clock.now() >= request.deadline {
                Outcome::Expired
            } else {
                outcome
            };
            state.pending.remove(&request.intent);
            if outcome == Outcome::Approved && request.intent == Intent::Quit {
                state.exiting = true;
                state.quit_authorized = true;
            }
            outcome
        };
        self.settle(request, outcome, cx);
        if outcome == Outcome::Approved {
            let host = self.clone();
            cx.defer(move |cx| match request.intent {
                Intent::Quit => cx.quit(),
                Intent::Close(target) => {
                    // A newer Quit supersedes even an approved but not yet dispatched close.
                    if host.is_exiting()
                        || host.0.borrow().pending.contains_key(&Intent::Quit)
                        || host.0.borrow().pending.contains_key(&Intent::Close(target))
                    {
                        return;
                    }
                    host.0.borrow_mut().close_authorized.insert(target);
                    let _ = target.update(cx, |_, window, cx| window.request_close(cx));
                    host.0.borrow_mut().close_authorized.remove(&target);
                }
            });
        }
        outcome == requested_outcome
    }

    fn settle(&self, request: Request, outcome: Outcome, cx: &mut App) {
        let callback = self.0.borrow().callbacks.settled.clone();
        cx.defer(move |cx| callback(request, outcome, cx));
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
