//! Public AppKit state and notifications for the synthetic owned window only.
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Object, Sel, YES},
    sel, sel_impl,
};
use std::{cell::Cell, ffi::CStr, time::Instant};

type Id = *mut Object;
thread_local! {
    static START: Instant = Instant::now();
    static WILL_MINIMIZE: Cell<usize> = const { Cell::new(0) };
    static DID_MINIMIZE: Cell<usize> = const { Cell::new(0) };
}

pub fn summary() {
    eprintln!(
        "AppKit trace: elapsed={:.3}s will_miniaturize={} did_miniaturize={}",
        START.with(|s| s.elapsed().as_secs_f64()),
        WILL_MINIMIZE.with(Cell::get),
        DID_MINIMIZE.with(Cell::get),
    );
}

pub fn snapshot(label: &str, w: Id) {
    unsafe {
        let app: Id = msg_send![class!(NSApplication), sharedApplication];
        let active: BOOL = msg_send![app, isActive];
        let key: BOOL = msg_send![w, isKeyWindow];
        let main: BOOL = msg_send![w, isMainWindow];
        let visible: BOOL = msg_send![w, isVisible];
        let minimized: BOOL = msg_send![w, isMiniaturized];
        let miniaturizable: BOOL = msg_send![w, isMiniaturizable];
        let style: usize = msg_send![w, styleMask];
        let animation: isize = msg_send![w, animationBehavior];
        eprintln!(
            "AppKit {:.3}s {label}: active={active} key={key} main={main} visible={visible} minimized={minimized} miniaturizable={miniaturizable} style={style} animation={animation}",
            START.with(|s| s.elapsed().as_secs_f64()),
        );
    }
}

extern "C" fn notification(_: &Object, _: Sel, notification: Id) {
    unsafe {
        let w: Id = msg_send![notification, object];
        let name: Id = msg_send![notification, name];
        let p: *const std::ffi::c_char = msg_send![name, UTF8String];
        let name = CStr::from_ptr(p).to_str().unwrap();
        match name {
            "NSWindowWillMiniaturizeNotification" => WILL_MINIMIZE.with(|c| c.set(c.get() + 1)),
            "NSWindowDidMiniaturizeNotification" => DID_MINIMIZE.with(|c| c.set(c.get() + 1)),
            _ => {}
        }
        snapshot(name, w);
    }
}

pub struct WindowTrace {
    observer: Id,
    center: Id,
}
impl WindowTrace {
    pub fn install(w: Id) -> Self {
        unsafe {
            let mut class = ClassDecl::new("TauNativeWindowTrace", class!(NSObject)).unwrap();
            class.add_method(
                sel!(observe:),
                notification as extern "C" fn(&Object, Sel, Id),
            );
            let observer: Id = msg_send![class.register(), new];
            let center: Id = msg_send![class!(NSNotificationCenter), defaultCenter];
            for name in [
                "NSWindowWillMiniaturizeNotification",
                "NSWindowDidMiniaturizeNotification",
                "NSWindowDidBecomeKeyNotification",
                "NSWindowDidResignKeyNotification",
                "NSWindowDidBecomeMainNotification",
                "NSWindowDidResignMainNotification",
                "NSWindowWillCloseNotification",
            ] {
                let string: Id = msg_send![class!(NSString), alloc];
                let string: Id = msg_send![string, initWithBytes:name.as_ptr() length:name.len() encoding:4usize];
                let _: () = msg_send![center, addObserver:observer selector:sel!(observe:) name:string object:w];
                let _: () = msg_send![string, release];
            }
            let main_thread: BOOL = msg_send![class!(NSThread), isMainThread];
            assert_eq!(main_thread, YES);
            snapshot("observer installed", w);
            Self { observer, center }
        }
    }
}
impl Drop for WindowTrace {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.center, removeObserver:self.observer];
            let _: () = msg_send![self.observer, release];
        }
    }
}
