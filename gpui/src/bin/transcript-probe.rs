#[path = "../text.rs"]
mod text;
#[path = "../transcript.rs"]
mod transcript;

use anyhow::{Result, ensure};
use gpui_kit::{
    AnyWindowHandle, AppContext, AssetSource, Bounds, HeadlessAppContext, InputEvent, SharedString,
    WindowBounds, WindowOptions, px, size, test::TestWindowExt,
};
use serde::Serialize;
use std::{borrow::Cow, fs, path::Path, sync::Arc, time::Duration};
use transcript::Transcript;

struct Assets;
impl AssetSource for Assets {
    fn load(&self, _: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }
    fn list(&self, _: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
fn init(cx: &mut gpui_kit::App) {
    text::init();
    gpui_kit::init(cx);
    cx.text_system()
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../../assets/InterVariable.ttf"
        ))])
        .expect("bundled font");
}
fn options(show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Default::default(),
            size: size(px(760.), px(620.)),
        })),
        show,
        focus: show,
        ..Default::default()
    }
}
fn frame(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> Result<()> {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx))?;
    Ok(())
}
#[derive(Serialize)]
struct Geometry {
    viewport_bottom: f32,
    last_row_bottom: f32,
    gap: f32,
}
#[derive(Serialize)]
struct Check {
    name: &'static str,
    passed: bool,
    detail: String,
    screenshot: String,
    geometry: Option<Geometry>,
}
fn click(cx: &mut HeadlessAppContext, window: AnyWindowHandle, id: &'static str) -> Result<()> {
    cx.update_window(window, |_, window, cx| window.click(id, cx))?;
    frame(cx, window)
}
fn geometry(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    scene: &gpui_kit::Entity<Transcript>,
) -> Result<Geometry> {
    let viewport = cx.update_window(window, |_, window, _| {
        window.find("transcript-list").bounds()
    })?;
    let last = scene.read_with(cx, |s, _| s.bounds(s.len() - 1));
    let last = last.ok_or_else(|| anyhow::anyhow!("last transcript row not measured"))?;
    let viewport_bottom: f32 = (viewport.origin.y + viewport.size.height).into();
    let last_row_bottom: f32 = (last.origin.y + last.size.height).into();
    ensure!(
        viewport_bottom.is_finite() && last_row_bottom.is_finite() && viewport.size.height > px(0.),
        "invalid list geometry"
    );
    Ok(Geometry {
        viewport_bottom,
        last_row_bottom,
        gap: (viewport_bottom - last_row_bottom).abs(),
    })
}
fn capture(out: &Path, dark: bool, fault: bool) -> Result<()> {
    fs::create_dir_all(out)?;
    let manifest = out.join("checks.json");
    if manifest.exists() {
        fs::remove_file(&manifest)?;
    }
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(init);
    let (window, scene) = cx.update(|cx| {
        gpui_kit::open_window(options(false), cx, |_, cx| {
            cx.new(|cx| Transcript::new(dark, cx))
        })
    })?;
    cx.update_window(window, |_, window, _| {
        window.activate_window();
        ensure!(window.scale_factor() == 2., "capture needs 2x Metal");
        Ok::<_, anyhow::Error>(())
    })??;
    frame(&mut cx, window)?;
    ensure!(
        cx.update_window(window, |_, window, _| window.is_window_active())?,
        "capture window must be active"
    );
    let mut checks = Vec::new();
    macro_rules! snap {
        ($name:literal, $pass:expr, $detail:expr) => {{
            frame(&mut cx, window)?;
            let screenshot = format!("{}.png", $name);
            let image = cx.capture_screenshot(window)?;
            ensure!(
                image.dimensions() == (1520, 1240),
                "unexpected capture dimensions: {:?}",
                image.dimensions()
            );
            image.save(out.join(&screenshot))?;
            checks.push(Check {
                name: $name,
                passed: $pass,
                detail: $detail,
                screenshot,
                geometry: None,
            });
        }};
    }
    macro_rules! snap_pin {
        ($name:literal, $condition:expr, $detail:expr) => {{
            frame(&mut cx, window)?;
            let g = geometry(&mut cx, window, &scene)?;
            let passed = $condition && g.gap <= 1.;
            let detail = format!(
                "{} viewportBottom={} lastRowBottom={} gap={}",
                $detail, g.viewport_bottom, g.last_row_bottom, g.gap
            );
            snap!($name, passed, detail);
            checks.last_mut().unwrap().geometry = Some(g);
        }};
    }
    frame(&mut cx, window)?;
    let initial = scene.read_with(&cx, |s, _| (s.len(), s.following()));
    snap!(
        "initial",
        initial == (24, true),
        format!("count={} following={}", initial.0, initial.1)
    );
    click(&mut cx, window, "step")?;
    frame(&mut cx, window)?;
    let follow = scene.read_with(&cx, |s, _| (s.len(), s.following(), s.anchor().item_ix));
    snap_pin!(
        "follow",
        follow.0 == 25 && follow.1,
        format!(
            "count={} following={} topIndex={}",
            follow.0, follow.1, follow.2
        )
    );
    let stable_id = scene.read_with(&cx, |s, _| s.id(s.len() - 1));
    let before_growth = scene.read_with(&cx, |s, _| s.last_text_len());
    for _ in 0..3 {
        cx.update_window(window, |_, _, cx| {
            scene.update(cx, |s, cx| s.grow(s.len() - 1, cx))
        })?;
        frame(&mut cx, window)?;
        ensure!(
            geometry(&mut cx, window, &scene)?.gap <= 1.,
            "repeated growth unpinned"
        );
    }
    let growth = scene.read_with(&cx, |s, _| (s.id(s.len() - 1), s.last_text_len()));
    snap_pin!(
        "growth",
        growth.0 == stable_id && growth.1 > before_growth,
        format!(
            "stableId={} textBytes={} -> {}",
            stable_id, before_growth, growth.1
        )
    );
    click(&mut cx, window, "shrink")?;
    snap_pin!(
        "shrink",
        scene.read_with(&cx, |s, _| s.following()),
        "synthetic footer=110px".to_string()
    );
    click(&mut cx, window, "shrink")?;
    click(&mut cx, window, "stream")?;
    for _ in 0..6 {
        cx.advance_clock(Duration::from_millis(200));
        frame(&mut cx, window)?;
    }
    let live = scene.read_with(&cx, |s, _| (s.ticks(), s.len(), s.paused()));
    snap_pin!(
        "stream",
        live == (6, 27, false),
        format!("ticks={} count={} paused={}", live.0, live.1, live.2)
    );
    click(&mut cx, window, "stream")?;
    cx.advance_clock(Duration::from_secs(20));
    frame(&mut cx, window)?;
    let stopped = scene.read_with(&cx, |s, _| (s.ticks(), s.len(), s.paused()));
    snap_pin!(
        "pause",
        stopped == (6, 27, true),
        format!(
            "ticks={} count={} paused={}",
            stopped.0, stopped.1, stopped.2
        )
    );
    cx.update_window(window, |_, window, cx| {
        window.dispatch_event(
            gpui_kit::ScrollWheelEvent {
                position: window.find("transcript-list").bounds().center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(340.))),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
    })?;
    frame(&mut cx, window)?;
    let wheel = scene.read_with(&cx, |s, _| {
        (s.following(), s.anchor(), s.bounds(s.anchor().item_ix))
    });
    click(&mut cx, window, "step")?;
    let wheel_after = scene.read_with(&cx, |s, _| {
        (s.following(), s.anchor(), s.bounds(wheel.1.item_ix))
    });
    snap!(
        "wheel",
        !wheel.0
            && !wheel_after.0
            && wheel.1.item_ix == wheel_after.1.item_ix
            && wheel.1.offset_in_item == wheel_after.1.offset_in_item
            && wheel.2 == wheel_after.2
            && wheel.2.is_some(),
        format!(
            "following={} -> {} anchor={:?} -> {:?} visibleRow={:?} -> {:?}",
            wheel.0, wheel_after.0, wheel.1, wheel_after.1, wheel.2, wheel_after.2
        )
    );
    cx.update_window(window, |_, _, cx| {
        scene.update(cx, |s, cx| s.inspect_above(cx))
    })?;
    frame(&mut cx, window)?;
    let height_before = scene.read_with(&cx, |s, _| s.bounds(5).map(|b| b.size.height));
    click(&mut cx, window, "older")?;
    frame(&mut cx, window)?;
    let before = scene.read_with(&cx, |s, _| (s.anchor(), s.bounds(6), s.id(6)));
    click(&mut cx, window, "collapse-7")?;
    let collapsed = scene.read_with(&cx, |s, _| !s.expanded(6));
    click(&mut cx, window, "collapse-7")?;
    let expanded = scene.read_with(&cx, |s, _| s.expanded(6));
    snap!(
        "collapse",
        collapsed && expanded,
        format!(
            "rowId={} collapsed={} expanded={}",
            before.2, collapsed, expanded
        )
    );
    let before = scene.read_with(&cx, |s, _| (s.anchor(), s.bounds(6), s.id(6)));
    let selected = scene.read_with(&cx, |s, _| s.view(6));
    cx.update_window(window, |_, _, cx| {
        selected.update(cx, |s, cx| s.select_all(cx))
    })?;
    if fault {
        cx.update_window(window, |_, _, cx| scene.update(cx, |s, cx| s.latest(cx)))?;
    }
    for _ in 0..3 {
        cx.update_window(window, |_, _, cx| scene.update(cx, |s, cx| s.step(cx)))?;
        frame(&mut cx, window)?;
    }
    let after = scene.read_with(&cx, |s, _| {
        (s.anchor(), s.bounds(6), s.id(6), s.following())
    });
    let selection = selected.read_with(&cx, |s, _| s.selected_text());
    let steady = before.0.item_ix == after.0.item_ix
        && before.2 == after.2
        && before.1 == after.1
        && !after.3
        && !selection.is_empty();
    snap!(
        "reader",
        steady,
        format!(
            "id={} topIndex={} -> {} bounds={:?} -> {:?} following={}",
            after.2, before.0.item_ix, after.0.item_ix, before.1, after.1, after.3
        )
    );
    cx.update_window(window, |_, _, cx| {
        scene.update(cx, |s, cx| s.change_above(cx))
    })?;
    frame(&mut cx, window)?;
    let changed = scene.read_with(&cx, |s, _| {
        (s.anchor(), s.bounds(6), s.id(6), s.expanded(5))
    });
    let selection_after = selected.read_with(&cx, |s, _| s.selected_text());
    cx.update_window(window, |_, _, cx| {
        scene.update(cx, |s, cx| s.inspect_above(cx))
    })?;
    frame(&mut cx, window)?;
    let height_after = scene.read_with(&cx, |s, _| s.bounds(5).map(|b| b.size.height));
    cx.update_window(window, |_, _, cx| scene.update(cx, |s, cx| s.older(cx)))?;
    frame(&mut cx, window)?;
    let restored = scene.read_with(&cx, |s, _| s.bounds(6));
    snap!(
        "above",
        changed.0.item_ix == before.0.item_ix
            && changed.1 == before.1
            && changed.2 == before.2
            && changed.3
            && selection == selection_after
            && height_before.zip(height_after).is_some_and(|(a, b)| b > a)
            && restored == before.1,
        format!(
            "id={} bounds={:?} -> {:?} restored={:?} above height={:?} -> {:?} expanded={}",
            changed.2, before.1, changed.1, restored, height_before, height_after, changed.3
        )
    );
    cx.update_window(window, |_, _, cx| scene.update(cx, |s, cx| s.toggle(5, cx)))?;
    frame(&mut cx, window)?;
    click(&mut cx, window, "latest")?;
    frame(&mut cx, window)?;
    let latest = scene.read_with(&cx, |s, _| {
        (s.following(), s.anchor().item_ix, s.len(), s.expanded(5))
    });
    snap_pin!(
        "latest",
        latest.0 && !latest.3,
        format!(
            "following={} topIndex={} count={} expansion={}",
            latest.0, latest.1, latest.2, latest.3
        )
    );
    click(&mut cx, window, "large")?;
    frame(&mut cx, window)?;
    let large = scene.read_with(&cx, |s, _| (s.len(), s.following(), s.anchor().item_ix));
    snap_pin!(
        "large",
        large.0 == 1000 && large.1,
        format!(
            "count={} following={} topIndex={}",
            large.0, large.1, large.2
        )
    );
    click(&mut cx, window, "step")?;
    snap_pin!(
        "large-growth",
        scene.read_with(&cx, |s, _| s.len()) == 1001,
        "append and grow in one event".to_string()
    );
    click(&mut cx, window, "reset")?;
    cx.advance_clock(Duration::from_secs(20));
    frame(&mut cx, window)?;
    let reset = scene.read_with(&cx, |s, _| {
        (
            s.len(),
            s.id(0),
            s.expanded(5),
            s.following(),
            s.paused(),
            s.ticks(),
        )
    });
    snap!(
        "reset",
        reset == (24, 1, true, true, true, 0),
        format!(
            "count={} firstId={} expanded={} following={} paused={} ticks={}",
            reset.0, reset.1, reset.2, reset.3, reset.4, reset.5
        )
    );
    click(&mut cx, window, "stream")?;
    frame(&mut cx, window)?;
    click(&mut cx, window, "reset")?;
    cx.advance_clock(Duration::from_secs(20));
    frame(&mut cx, window)?;
    let interrupted = scene.read_with(&cx, |s, _| (s.len(), s.ticks(), s.paused()));
    snap!(
        "interrupted",
        interrupted == (24, 0, true),
        format!(
            "count={} ticks={} paused={}",
            interrupted.0, interrupted.1, interrupted.2
        )
    );
    click(&mut cx, window, "stream")?;
    for _ in 0..60 {
        cx.advance_clock(Duration::from_millis(200));
        frame(&mut cx, window)?;
    }
    let bounded = scene.read_with(&cx, |s, _| (s.ticks(), s.len(), s.paused()));
    cx.advance_clock(Duration::from_secs(20));
    frame(&mut cx, window)?;
    let bounded_after = scene.read_with(&cx, |s, _| (s.ticks(), s.len(), s.paused()));
    snap_pin!(
        "bounded",
        bounded == (60, 36, true) && bounded_after == bounded,
        format!(
            "ticks={} count={} paused={} after={:?}",
            bounded.0, bounded.1, bounded.2, bounded_after
        )
    );
    fs::write(out.join("checks.json"), serde_json::to_vec_pretty(&checks)?)?;
    ensure!(
        checks.iter().all(|c| c.passed),
        "transcript behavioral check failed; see checks.json"
    );
    println!("{}: {} Metal checks passed", out.display(), checks.len());
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let dark = args.iter().any(|a| a == "--dark");
    if args.first().is_some_and(|a| a == "--capture") {
        ensure!(
            args.len() == 2
                || (args.len() == 3 && (args[2] == "--dark" || args[2] == "--fault-anchor")),
            "usage: --capture <directory> [--dark|--fault-anchor]"
        );
        return capture(
            Path::new(&args[1]),
            dark,
            args.iter().any(|a| a == "--fault-anchor"),
        );
    }
    ensure!(
        args.is_empty() || args == ["--dark"],
        "usage: [--dark] | --capture <directory> [--dark]"
    );
    gpui_kit::application().run(move |cx| {
        init(cx);
        gpui_kit::open_window(options(true), cx, |window, cx| {
            window.set_window_title("Tau — Streaming Transcript");
            cx.new(|cx| Transcript::new(dark, cx))
        })
        .expect("native window");
        cx.activate(true);
    });
    Ok(())
}
