//! Real production MarkdownView / Root ink oracle, rendered only with offscreen Metal.
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext, Bounds, Context, Entity, HeadlessAppContext, IntoElement, Render, Window,
    WindowBounds, WindowOptions, base::Root, div, platform, prelude::*, px, size,
};
use std::sync::Arc;
use tau_gpui_prototype::components::{Assets, init, markdown::MarkdownView, support};
const SOURCE: &str = "- Marker one\n- Marker two\n  - Nested circle\n    - Nested square\n\n7. Ordered control\n8. Ordered control two\n\n- [ ] Task control\n- [x] Checked control";
struct Host {
    markdown: Entity<MarkdownView>,
    dark: bool,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        support::root(self.dark).child(div().size_full().p(px(24.)).child(self.markdown.clone()))
    }
}
fn main() {
    let out = std::env::var("TAU_MARKER_CAPTURE").ok();
    if let Some(out) = &out {
        std::fs::create_dir_all(out).unwrap();
    }
    for dark in [false, true] {
        let mut cx = HeadlessAppContext::with_platform(
            platform::current_platform(true).text_system(),
            Arc::new(Assets),
            platform::current_headless_renderer,
        );
        cx.update(|cx| init(cx, dark));
        let (handle, _) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            Default::default(),
                            size(px(420.), px(400.)),
                        ))),
                        show: false,
                        focus: false,
                        ..Default::default()
                    },
                    cx,
                    |w, cx| {
                        let host = cx.new(|cx| Host {
                            markdown: cx.new(|cx| {
                                MarkdownView::new(SOURCE, dark, |_| {}, cx).natural_height()
                            }),
                            dark,
                        });
                        cx.new(|cx| Root::new(host, w, cx))
                    },
                )
            })
            .unwrap();
        cx.run_until_parked();
        for _ in 0..3 {
            cx.update_window(handle, |_, w, cx| w.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
        let image = cx.capture_screenshot(handle).unwrap();
        println!(
            "dark={dark} backing_scale={} dimensions={}x{}",
            image.width() as f32 / 420.,
            image.width(),
            image.height()
        );
        let scale = image.width() as f32 / 420.;
        let background = image.get_pixel(0, 0);
        let foreground = if dark {
            [90., 99., 120.]
        } else {
            [161., 168., 178.]
        };
        let delta: [f32; 3] = std::array::from_fn(|i| foreground[i] - background[i] as f32);
        let denominator: f32 = delta.iter().map(|v| v * v).sum();
        let mut area = 0.;
        let mut min = [f32::MAX; 2];
        let mut max = [0_f32; 2];
        for (x, y, pixel) in image.enumerate_pixels() {
            let position = [x as f32 / scale, y as f32 / scale];
            if !(24. ..41.).contains(&position[0]) || !(24. ..46.).contains(&position[1]) {
                continue;
            }
            let coverage = ((0..3)
                .map(|i| (pixel[i] as f32 - background[i] as f32) * delta[i])
                .sum::<f32>()
                / denominator)
                .clamp(0., 1.);
            if coverage > 0.02 {
                area += coverage / scale.powi(2);
                for i in 0..2 {
                    min[i] = min[i].min(position[i]);
                    max[i] = max[i].max(position[i] + 1. / scale);
                }
            }
        }
        // User preference is twice the original native diameter, not Vue pixel parity.
        // Base35 measured 4.5px / 12.4px²; actual Vue measured 5px / 17.2px².
        assert!(
            (45. ..57.).contains(&area),
            "marker area={area}, dark={dark}"
        );
        for axis in 0..2 {
            assert!((8. ..9.5).contains(&(max[axis] - min[axis])));
        }
        if let Some(out) = &out {
            image
                .save(format!(
                    "{out}/native-{}.png",
                    if dark { "dark" } else { "light" }
                ))
                .unwrap();
        }
    }
}
