use super::DiagramBlock;
use gpui_kit::*;

pub(super) fn install(
    weak: WeakEntity<DiagramBlock>,
    hitbox: Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    // Hitbox IDs are frame-local. Keep an active drag captured across redraws.
    let _ = weak.update(cx, |s, _| {
        if let Some(v) = &mut s.viewer
            && v.capture
                .is_some_and(|id| window.captured_hitbox() == Some(id))
        {
            window.capture_pointer(hitbox.id);
            v.capture = Some(hitbox.id);
        }
    });
    let down = weak.clone();
    let id = hitbox.id;
    window.on_mouse_event(move |e: &MouseDownEvent, phase, w, cx| {
        if !phase.bubble() || !hitbox.is_hovered(w) {
            return;
        }
        if e.button == MouseButton::Left {
            w.capture_pointer(id);
            let _ = down.update(cx, |s, cx| {
                if let Some(v) = &mut s.viewer {
                    let p = super::point_array(e.position);
                    v.drag = Some((p, p));
                    v.capture = Some(id);
                }
                cx.notify();
            });
        }
        cx.stop_propagation();
    });
    let moving = weak.clone();
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, w, cx| {
        if !phase.bubble() || w.captured_hitbox() != Some(id) {
            return;
        }
        let _ = moving.update(cx, |s, cx| {
            if e.pressed_button != Some(MouseButton::Left) {
                s.cancel_pan(w);
            }
            if let Some(v) = &mut s.viewer
                && let Some((_, last)) = &mut v.drag
            {
                let p = super::point_array(e.position);
                v.camera.pan([p[0] - last[0], p[1] - last[1]]);
                *last = p;
            }
            s.sync(w, cx);
            cx.notify();
        });
        cx.stop_propagation();
    });
    window.on_mouse_event(move |e: &MouseUpEvent, phase, w, cx| {
        if !phase.bubble() || w.captured_hitbox() != Some(id) || e.button != MouseButton::Left {
            return;
        }
        let _ = weak.update(cx, |s, cx| {
            let drag = s.viewer.as_ref().and_then(|v| v.drag);
            s.cancel_pan(w);
            if let Some((down, _)) = drag {
                let p = super::point_array(e.position);
                if (p[0] - down[0]).hypot(p[1] - down[1]) < 5. {
                    s.dismiss(w, cx);
                }
            }
            cx.notify();
        });
        cx.stop_propagation();
    });
}
