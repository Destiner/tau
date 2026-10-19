use super::*;
use gpui_kit::TestApp;
use std::cell::Cell;

#[test]
fn inline_cancel_reentrant_quit_and_installation_are_safe() {
    let mut app = TestApp::new();
    let count = Rc::new(Cell::new(0));
    app.update(|cx| {
        let count = count.clone();
        Lifecycle::install(
            cx,
            Duration::from_secs(1),
            LifecycleCallbacks {
                requested: Rc::new(move |request, cx| {
                    count.set(count.get() + 1);
                    let host = cx.global::<Lifecycle>().clone();
                    assert!(host.resolve(request, false, cx));
                    if count.get() == 1 {
                        cx.quit();
                    }
                }),
                settled: Rc::new(|_, _, _| {}),
            },
        );
        Lifecycle::install(
            cx,
            Duration::from_secs(99),
            LifecycleCallbacks {
                requested: Rc::new(|_, _| panic!("must retain original host")),
                settled: Rc::new(|_, _, _| panic!("must retain original host")),
            },
        );
    });
    assert!(!app.simulate_quit());
    assert_eq!(count.get(), 2);
    assert!(!app.simulate_quit());
    assert_eq!(count.get(), 3);
    assert_eq!(app.accepted_quits(), 0);
}

#[test]
fn elapsed_request_can_be_replaced_before_timer_and_settlement_is_reentrant() {
    let mut app = TestApp::new();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let settled = Rc::new(RefCell::new(Vec::new()));
    let host = app.update(|cx| {
        let requests = requests.clone();
        let settled = settled.clone();
        Lifecycle::install(
            cx,
            Duration::from_secs(1),
            LifecycleCallbacks {
                requested: Rc::new(move |request, _| requests.borrow_mut().push(request)),
                settled: Rc::new(move |request, outcome, cx| {
                    settled.borrow_mut().push((request, outcome));
                    let host = cx.global::<Lifecycle>().clone();
                    assert!(!host.resolve(request, true, cx));
                    if outcome == Outcome::Expired {
                        // Coalesce with the already-installed replacement, without
                        // holding a coordinator borrow across this callback.
                        host.request(Intent::Quit, cx);
                    }
                }),
            },
        )
    });
    assert!(!app.simulate_quit());
    let old = requests.borrow()[0];
    app.update(|cx| {
        cx.background_executor()
            .dispatcher()
            .as_test()
            .unwrap()
            .scheduler()
            .clock()
            .advance(Duration::from_secs(1));
        assert!(settled.borrow().is_empty());
        assert!(!host.is_pending(old));
        host.request(Intent::Quit, cx);
    });
    assert_eq!(requests.borrow().len(), 2);
    let current = requests.borrow()[1];
    assert_ne!(old, current);
    assert!(host.is_pending(current));
    assert_eq!(*settled.borrow(), [(old, Outcome::Expired)]);
    app.update(|cx| assert!(host.resolve(current, false, cx)));
    assert_eq!(
        *settled.borrow(),
        [(old, Outcome::Expired), (current, Outcome::Cancelled)]
    );
    assert_eq!(app.accepted_quits(), 0);
}

#[test]
fn deferred_request_callback_does_not_start_an_expired_phase() {
    let mut app = TestApp::new();
    let settled = Rc::new(RefCell::new(Vec::new()));
    app.update(|cx| {
        let observed = settled.clone();
        let host = Lifecycle::install(
            cx,
            Duration::from_secs(1),
            LifecycleCallbacks {
                requested: Rc::new(|_, _| panic!("expired phase must not start")),
                settled: Rc::new(move |request, outcome, _| {
                    observed.borrow_mut().push((request, outcome));
                }),
            },
        );
        host.request(Intent::Quit, cx);
        cx.background_executor()
            .dispatcher()
            .as_test()
            .unwrap()
            .scheduler()
            .clock()
            .advance(Duration::from_secs(1));
    });
    assert_eq!(settled.borrow().len(), 1);
    assert_eq!(settled.borrow()[0].1, Outcome::Expired);
    assert_eq!(app.accepted_quits(), 0);
}

#[test]
fn core_guard_replacement_does_not_borrow_callback_across_invocation() {
    let mut app = TestApp::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    app.update(|cx| {
        cx.on_app_should_quit(move |cx| {
            count.set(count.get() + 1);
            cx.on_app_should_quit(|_| true);
            false
        })
    });
    assert!(!app.simulate_quit_while_borrowed());
    assert_eq!(calls.get(), 0);
    assert!(!app.simulate_quit());
    assert_eq!(calls.get(), 1);
    assert!(app.simulate_quit());
    assert_eq!(calls.get(), 1);
}
