//! Native inline Mermaid and chrome-free, nonmodal fullscreen layer.
//! The parent owns the 0.7em block gap and subscribes to DiagramChanged to
//! invalidate TextView layout. Return this entity only when ready; retain code otherwise.
mod camera;
mod pointer;
#[cfg(test)]
mod tests;
mod worker;

use super::{
    diagram::{Diagram, DiagramError, Operation, RasterCamera, ViewBox},
    support::tokens,
};
use camera::Camera;
use gpui_kit::base::Button as BaseButton;
use gpui_kit::{prelude::*, *};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, Default)]
pub struct DiagramChanged;
impl EventEmitter<DiagramChanged> for DiagramBlock {}

pub struct DiagramBlock {
    source: Arc<str>,
    source_valid: bool,
    dark: bool,
    revision: u64,
    diagram: Option<Diagram>,
    image: Option<Arc<RenderImage>>,
    inline_image: Option<Arc<RenderImage>>,
    icon: Option<Arc<RenderImage>>,
    error: Option<DiagramError>,
    operation: Option<Operation>,
    task: Option<Task<()>>,
    expand_focus: FocusHandle,
    viewer_focus: FocusHandle,
    origin: Option<WeakFocusHandle>,
    viewer: Option<Viewer>,
    inline_size: [f64; 2],
    viewer_observers: Vec<Subscription>,
    requested: Option<Spec>,
    painted: Option<Spec>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spec {
    viewport: ViewBox,
    zoom: f64,
    backing: f64,
    rect: [f64; 4],
    fullscreen: bool,
}
struct Viewer {
    camera: Camera,
    size: [f64; 2],
    minimum: f64,
    drag: Option<([f64; 2], [f64; 2])>,
    capture: Option<HitboxId>,
}
impl Drop for DiagramBlock {
    fn drop(&mut self) {
        if let Some(op) = &self.operation {
            op.cancel();
        }
    }
}
impl DiagramBlock {
    pub fn new(source: String, dark: bool, cx: &mut Context<Self>) -> Self {
        let source_valid = worker::source_allowed(&source);
        let mut block = Self {
            // The parent retains fallback code; rejected views must not duplicate huge input.
            source: if source_valid {
                Arc::from(source)
            } else {
                Arc::from("")
            },
            source_valid,
            dark,
            revision: 0,
            diagram: None,
            image: None,
            inline_image: None,
            icon: None,
            error: None,
            operation: None,
            task: None,
            expand_focus: cx.focus_handle(),
            viewer_focus: cx.focus_handle(),
            origin: None,
            viewer: None,
            inline_size: [0., 0.],
            viewer_observers: Vec::new(),
            requested: None,
            painted: None,
        };
        block.start(None, cx);
        block
    }
    pub fn ready(&self) -> bool {
        self.diagram.is_some() && self.inline_image.is_some() && self.error.is_none()
    }
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
    pub fn busy(&self) -> bool {
        self.operation.is_some()
    }
    pub fn error_kind(&self) -> Option<DiagramError> {
        self.error
    }
    pub fn natural_bounds(&self) -> Option<ViewBox> {
        self.diagram.as_ref().map(Diagram::view_box)
    }
    pub fn is_expanded(&self) -> bool {
        self.viewer.is_some()
    }
    /// The prepared production SVG, never a fixture reconstruction.
    pub fn rendered_svg(&self) -> Option<&str> {
        self.diagram.as_ref().map(Diagram::svg)
    }
    pub fn semantic_model(&self) -> Option<&serde_json::Value> {
        self.diagram.as_ref().map(Diagram::semantic_model)
    }
    pub fn camera_state(&self) -> Option<[f64; 3]> {
        self.viewer
            .as_ref()
            .map(|v| [v.camera.scale, v.camera.tx, v.camera.ty])
    }
    pub fn expand_focused(&self, window: &Window) -> bool {
        self.expand_focus.is_focused(window)
    }
    pub fn set_theme(&mut self, dark: bool, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        // Keep the mounted block and its reservation while the new scheme prepares.
        self.error = None;
        self.requested = None;
        self.painted = None;
        self.start(None, cx);
        self.changed(cx);
    }
    pub fn set_source(&mut self, source: String, cx: &mut Context<Self>) {
        if self.source_valid && *self.source == source {
            return;
        }
        self.source_valid = worker::source_allowed(&source);
        self.source = if self.source_valid {
            Arc::from(source)
        } else {
            Arc::from("")
        };
        self.diagram = None;
        self.image = None;
        self.inline_image = None;
        self.error = None;
        self.requested = None;
        self.painted = None;
        self.start(None, cx);
        self.changed(cx);
    }
    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(DiagramChanged);
        cx.notify();
    }
    fn start(&mut self, spec: Option<Spec>, cx: &mut Context<Self>) {
        if let Some(op) = self.operation.take() {
            op.cancel();
        }
        self.task = None;
        self.revision += 1;
        let revision = self.revision;
        self.requested = spec;
        if !self.source_valid {
            self.error = Some(DiagramError::SourceLimit);
            self.changed(cx);
            return;
        }
        let result = worker::submit(
            self.source.clone(),
            self.dark,
            spec.and_then(|_| self.diagram.clone()),
            spec.map(|s| RasterCamera {
                viewport: s.viewport,
                zoom: s.zoom,
                backing_scale: s.backing,
            }),
            cx.svg_renderer(),
        );
        let (op, mut request) = match result {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                self.changed(cx);
                return;
            }
        };
        self.operation = Some(op.clone());
        self.requested = spec;
        self.task = Some(cx.spawn(async move |weak, cx| {
            loop {
                let Some(result) = request.poll() else {
                    cx.background_executor()
                        .timer(Duration::from_millis(8))
                        .await;
                    if weak.upgrade().is_none() {
                        break;
                    }
                    continue;
                };
                let _ = weak.update(cx, |block, cx| {
                    if block.revision != revision {
                        return;
                    }
                    block.operation = None;
                    match result {
                        Ok((diagram, image, icon)) => {
                            block.icon = Some(icon);
                            if !spec.is_some_and(|s| s.fullscreen) {
                                block.inline_image = Some(image.clone());
                            }
                            block.diagram = Some(diagram);
                            block.image = Some(image);
                            block.painted = spec;
                            block.error = None;
                        }
                        Err(e) => {
                            op.cancel();
                            block.error = Some(e);
                        }
                    }
                    block.changed(cx);
                });
                break;
            }
        }));
    }
    pub fn expand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.natural_bounds() else {
            return;
        };
        if self.viewer.is_some() {
            return;
        }
        let size = viewport(window);
        let camera = Camera::fit(b, size);
        self.origin = Some(self.expand_focus.downgrade());
        self.viewer = Some(Viewer {
            camera,
            size,
            minimum: camera.scale.min(0.1),
            drag: None,
            capture: None,
        });
        self.viewer_observers = vec![
            cx.observe_window_activation(window, |s, w, _| {
                if !w.is_window_active() {
                    s.cancel_pan(w);
                }
            }),
            cx.on_blur(&self.viewer_focus.clone(), window, |s, w, _| {
                s.cancel_pan(w)
            }),
            cx.on_release_in(window, |s, w, _| s.cancel_pan(w)),
        ];
        self.viewer_focus.focus(window, cx);
        self.requested = None;
        self.sync(window, cx);
        cx.notify();
    }
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_pan(window);
        if self.viewer.take().is_none() {
            return;
        }
        self.viewer_observers.clear();
        if self.viewer_focus.is_focused(window)
            && let Some(f) = self.origin.take().and_then(|f| f.upgrade())
        {
            f.focus(window, cx);
        }
        self.origin = None;
        self.requested = None;
        self.sync(window, cx);
        cx.notify();
    }
    pub fn cancel_pan(&mut self, window: &mut Window) {
        if let Some(v) = &mut self.viewer {
            if v.capture
                .take()
                .is_some_and(|id| window.captured_hitbox() == Some(id))
            {
                window.release_pointer();
            }
            v.drag = None;
        }
    }
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.natural_bounds() else {
            return;
        };
        let (camera, size, fullscreen) = if let Some(v) = &mut self.viewer {
            let size = viewport(window);
            v.camera.resize(v.size, size);
            v.size = size;
            (v.camera, size, true)
        } else {
            // The first preparation reserves natural bounds; wait for actual layout,
            // rather than scheduling a one-pixel raster before the canvas is measured.
            if self.inline_size[0] <= 0. || self.inline_size[1] <= 0. {
                return;
            }
            let scale = (self.inline_size[0] / b.width).min(1.);
            (
                Camera {
                    scale,
                    tx: -b.x * scale,
                    ty: -b.y * scale,
                },
                self.inline_size,
                false,
            )
        };
        let Some(spec) = visible_spec(
            camera,
            size,
            b,
            f64::from(window.scale_factor()),
            fullscreen,
        ) else {
            return;
        };
        if self.requested != Some(spec) && !self.busy() {
            self.start(Some(spec), cx);
        }
    }
    fn overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let size = viewport(window);
        let t = tokens(self.dark);
        let v = self.viewer.as_ref().unwrap();
        let camera = v.camera;
        let image = self.image.clone();
        let current = self
            .natural_bounds()
            .and_then(|b| visible_spec(camera, size, b, f64::from(window.scale_factor()), true));
        let painted = self.painted.filter(|p| p.fullscreen && current.is_some());
        let weak = cx.entity().downgrade();
        let grid = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                pointer::install(weak.clone(), hitbox, window, cx);
                let sx = camera.tx.rem_euclid(24.);
                let sy = camera.ty.rem_euclid(24.);
                for x in 0..(size[0] / 24.).ceil() as usize + 1 {
                    for y in 0..(size[1] / 24.).ceil() as usize + 1 {
                        window.paint_quad(
                            fill(
                                Bounds::new(
                                    bounds.origin
                                        + point(
                                            px((sx + x as f64 * 24.) as f32),
                                            px((sy + y as f64 * 24.) as f32),
                                        ),
                                    gpui_kit::size(px(2.), px(2.)),
                                ),
                                rgb(t.border),
                            )
                            .corner_radii(px(1.)),
                        );
                    }
                }
                if let (Some(image), Some(p)) = (&image, painted) {
                    let r = Bounds::new(
                        bounds.origin
                            + point(
                                px((p.viewport.x * camera.scale + camera.tx) as f32),
                                px((p.viewport.y * camera.scale + camera.ty) as f32),
                            ),
                        // Keep the last crop visible while a sharper raster is in flight.
                        // Its viewport is in diagram units, not the old camera's pixels.
                        gpui_kit::size(
                            px((p.viewport.width * camera.scale) as f32),
                            px((p.viewport.height * camera.scale) as f32),
                        ),
                    );
                    let _ = window.paint_image(
                        bounds,
                        r,
                        Corners::all(px(0.)),
                        image.clone(),
                        0,
                        false,
                    );
                }
            },
        )
        .size_full();
        let overlay = div()
            .id("diagram-fullscreen")
            .test_support()
            .role(accesskit::Role::Dialog)
            .aria_label("Diagram")
            .w(px(size[0] as f32))
            .h(px(size[1] as f32))
            .bg(rgb(t.canvas))
            .cursor(if v.drag.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::Arrow
            })
            .overflow_hidden()
            .occlude()
            .track_focus(&self.viewer_focus)
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
                if s.viewer_focus.is_focused(w) && e.keystroke.key == "escape" {
                    s.dismiss(w, cx);
                }
                cx.stop_propagation();
            }))
            .on_key_up(cx.listener(|_, _: &KeyUpEvent, _, cx| cx.stop_propagation()))
            .on_scroll_wheel(cx.listener(|s, e: &ScrollWheelEvent, w, cx| {
                let delta = e.delta.pixel_delta(px(16.));
                if let Some(v) = &mut s.viewer {
                    if e.modifiers.control || e.modifiers.platform {
                        v.camera.zoom(
                            point_array(e.position),
                            v.camera.scale * (f64::from(delta.y) * 0.01).exp(),
                            v.minimum,
                        );
                    } else {
                        v.camera.pan([f64::from(delta.x), f64::from(delta.y)]);
                    }
                }
                s.sync(w, cx);
                cx.notify();
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|s, e: &PinchEvent, w, cx| {
                if e.phase == TouchPhase::Cancelled {
                    s.cancel_pan(w);
                } else if let Some(v) = &mut s.viewer {
                    v.camera.zoom(
                        point_array(e.position),
                        v.camera.scale * (1. + f64::from(e.delta)).max(0.01),
                        v.minimum,
                    );
                }
                s.sync(w, cx);
                cx.notify();
                cx.stop_propagation();
            }))
            .child(grid);
        deferred(anchored().position(point(px(0.), px(0.))).child(overlay))
            .with_priority(30)
            .into_any_element()
    }
}
impl Render for DiagramBlock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let t = tokens(self.dark);
        let b = self.natural_bounds().unwrap_or(ViewBox {
            x: 0.,
            y: 0.,
            width: 1.,
            height: 1.,
        });
        let weak = cx.entity().downgrade();
        let image = self.inline_image.clone();
        let inline = canvas(
            move |bounds, window, cx| {
                let size = [f64::from(bounds.size.width), f64::from(bounds.size.height)];
                let weak = weak.clone();
                window.defer(cx, move |w, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        if s.inline_size != size {
                            s.inline_size = size;
                            s.changed(cx);
                            s.sync(w, cx);
                        }
                    });
                });
            },
            move |bounds, _, window, _| {
                if let Some(image) = &image {
                    let _ = window.paint_image(
                        bounds,
                        bounds,
                        Corners::all(px(0.)),
                        image.clone(),
                        0,
                        false,
                    );
                }
            },
        )
        .w_full()
        .aspect_ratio((b.width / b.height) as f32);
        let keyboard_focus =
            self.expand_focus.is_focused(window) && window.last_input_was_keyboard();
        let mut root = div()
            .id("diagram-block")
            .test_support()
            .group("diagram-block")
            .relative()
            .w_full()
            .min_w(px(0.))
            .border_1()
            .border_color(rgb(t.border))
            .rounded(px(9.))
            .p(px(4.))
            .flex()
            .justify_center()
            .child(div().w_full().max_w(px(b.width as f32)).child(inline))
            .child(
                BaseButton::new("diagram-expand")
                    .accessibility_label("Expand Diagram")
                    .group("diagram-expand")
                    // Keep the hitbox and focus registration painted, hiding only the ink.
                    .child(
                        div()
                            .id("diagram-expand-paint")
                            .test_support()
                            .absolute()
                            .top(px(0.))
                            .left(px(0.))
                            .size(px(21.))
                            .opacity(if keyboard_focus { 1. } else { 0. })
                            .group_hover("diagram-block", |s| s.opacity(1.))
                            .child(
                                div()
                                    .size(px(21.))
                                    .p(px(4.))
                                    .rounded(px(4.))
                                    .bg(rgb(t.canvas))
                                    .opacity(if keyboard_focus { 1. } else { 0.45 })
                                    .group_hover("diagram-expand", |s| s.opacity(1.))
                                    .children(
                                        self.icon.clone().map(|icon| img(icon).size(px(13.))),
                                    ),
                            ),
                    )
                    .w(px(21.))
                    .h(px(21.))
                    .p(px(4.))
                    .rounded(px(4.))
                    .track_focus(&self.expand_focus)
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|s, _, w, cx| {
                            s.expand_focus.focus(w, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .on_click(cx.listener(|s, _, w, cx| {
                        s.expand(w, cx);
                        cx.stop_propagation();
                    })),
            );
        if self.viewer.is_some() {
            root = root.child(self.overlay(window, cx));
        }
        root
    }
}
fn viewport(w: &Window) -> [f64; 2] {
    let s = w.viewport_size();
    [f64::from(s.width).max(1.), f64::from(s.height).max(1.)]
}
fn point_array(p: Point<Pixels>) -> [f64; 2] {
    [f64::from(p.x), f64::from(p.y)]
}
fn visible_spec(
    c: Camera,
    size: [f64; 2],
    b: ViewBox,
    backing: f64,
    fullscreen: bool,
) -> Option<Spec> {
    let r = c.raster(size, backing).viewport;
    let x = r.x.max(b.x);
    let y = r.y.max(b.y);
    let width = (r.x + r.width).min(b.x + b.width) - x;
    let height = (r.y + r.height).min(b.y + b.height) - y;
    if width <= 0. || height <= 0. {
        return None;
    }
    // Cap allocation while keeping the actual camera crop; never allocate a whole zoomed diagram.
    let effective = backing
        .min(4.)
        .min(4096. / (width * c.scale).max(height * c.scale))
        .min((16_777_216. / (width * height * c.scale * c.scale)).sqrt());
    Some(Spec {
        viewport: ViewBox {
            x,
            y,
            width,
            height,
        },
        zoom: c.scale,
        backing: effective,
        rect: [
            x * c.scale + c.tx,
            y * c.scale + c.ty,
            width * c.scale,
            height * c.scale,
        ],
        fullscreen,
    })
}
