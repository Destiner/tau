//! Window-owned version surfaces. This routes an affordance, not an updater operation.
use gpui_kit::{AnyWindowHandle, App, Context, EntityId, Global, Window, actions};
use std::rc::Rc;

actions!(native_version, [CheckForUpdates]);

type Available = dyn Fn(&App) -> bool;
type Reveal = dyn Fn(&mut Window, &mut App);
struct Surface {
    owner: EntityId,
    window: AnyWindowHandle,
    available: Rc<Available>,
    reveal: Rc<Reveal>,
}
#[derive(Default)]
struct VersionSurfaces(Vec<Surface>);
impl Global for VersionSurfaces {}

/// Register a weak, window-local destination. Replacing a shell never retains its old entity.
/// Availability must reflect the owner's modal/lifecycle admission, not updater support.
pub fn register_version_surface<T: 'static>(
    window: &mut Window,
    cx: &mut Context<T>,
    available: fn(&T) -> bool,
    reveal: fn(&mut T, &mut Window, &mut Context<T>),
) {
    if !cx.has_global::<VersionSurfaces>() {
        cx.set_global(VersionSurfaces::default());
    }
    let weak = cx.entity().downgrade();
    let probe = weak.clone();
    let surface = Surface {
        owner: cx.entity_id(),
        window: window.window_handle(),
        available: Rc::new(move |cx| {
            probe
                .upgrade()
                .is_some_and(|entity| available(entity.read(cx)))
        }),
        reveal: Rc::new(move |window, cx| {
            let _ = weak.update(cx, |owner, cx| {
                if available(owner) {
                    reveal(owner, window, cx);
                }
            });
        }),
    };
    let windows = cx.windows();
    let surfaces = cx.global_mut::<VersionSurfaces>();
    surfaces.0.retain(|old| {
        windows.contains(&old.window) && old.window != surface.window && old.owner != surface.owner
    });
    surfaces.0.push(surface);
}

fn active_surface(cx: &App) -> Option<&Surface> {
    let window = cx.active_window()?;
    if !cx.has_global::<VersionSurfaces>() {
        return None;
    }
    cx.global::<VersionSurfaces>()
        .0
        .iter()
        .find(|surface| surface.window == window && (surface.available)(cx))
}

pub(super) fn install(cx: &mut App) {
    cx.on_action_available::<CheckForUpdates>(|_, cx| {
        active_surface(cx).is_some() && !cx.global::<super::NativeHost>().lifecycle().is_exiting()
    });
    cx.on_action(|_: &CheckForUpdates, cx| {
        if cx.global::<super::NativeHost>().lifecycle().is_exiting() {
            return;
        }
        if let Some(surface) = active_surface(cx) {
            let target = surface.window;
            let reveal = surface.reveal.clone();
            // Global actions bubble while their window is already borrowed.
            cx.defer(move |cx| {
                let current = active_surface(cx).is_some_and(|surface| {
                    surface.window == target && Rc::ptr_eq(&surface.reveal, &reveal)
                });
                if current && !cx.global::<super::NativeHost>().lifecycle().is_exiting() {
                    let _ = target.update(cx, |_, window, cx| reveal(window, cx));
                }
            });
        }
    });
}

#[cfg(test)]
#[path = "menu_version_tests.rs"]
mod tests;
