mod model;
mod playground;
mod text;
mod ui;

use anyhow::{Context as _, Result, bail, ensure};
use gpui_kit::{
    AnyWindowHandle, AppContext, AssetSource, Bounds, Entity, HeadlessAppContext, InputEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, WindowBounds,
    WindowOptions, px, size, test::TestWindowExt,
};
use model::{Observation, PointerPhase, Scenario, Step};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashSet, fs, path::Path, sync::Arc};
use ui::{Scene, TestFault};

struct Assets;
impl AssetSource for Assets {
    fn load(&self, _: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }
    fn list(&self, _: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Suite {
    schema_version: usize,
    theme: String,
    scale: u32,
    viewport: Viewport,
    scenarios: Vec<Scenario>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Viewport {
    width: u32,
    height: u32,
}
#[derive(Serialize)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}
#[derive(Serialize)]
struct Capture {
    scenario: String,
    checkpoint: String,
    observation: Observation,
    bounds: Rect,
    screenshot: String,
    caret: &'static str,
}

fn init(cx: &mut gpui_kit::App) {
    text::init();
    gpui_kit::init(cx);
    ui::init(cx);
    cx.text_system()
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/InterVariable.ttf"
        ))])
        .expect("the bundled Inter font must load");
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}
fn options(width: u32, height: u32, show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Default::default(),
            size: size(px(width as f32), px(height as f32)),
        })),
        show,
        focus: show,
        ..Default::default()
    }
}
fn keystroke(key: &str) -> Result<String> {
    let mut parts = Vec::new();
    for part in key.split('+') {
        parts.push(
            match part {
                "ControlOrMeta" | "Meta" => "cmd",
                "Control" => "ctrl",
                "Shift" => "shift",
                "Alt" => "alt",
                "ArrowLeft" => "left",
                "ArrowRight" => "right",
                "ArrowUp" => "up",
                "ArrowDown" => "down",
                "Backspace" => "backspace",
                "Delete" => "delete",
                "Enter" => "enter",
                "Space" => "space",
                "Tab" => "tab",
                "Escape" => "escape",
                "Home" => "home",
                "End" => "end",
                "PageUp" => "pageup",
                "PageDown" => "pagedown",
                p if p.len() == 1 && p.is_ascii() => p,
                _ => bail!("unsupported key: {key}"),
            }
            .to_ascii_lowercase(),
        );
    }
    Ok(parts.join("-"))
}
fn render(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) -> Result<()> {
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))?;
    Ok(())
}
fn pointer(
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
    phase: PointerPhase,
    position: gpui_kit::Point<gpui_kit::Pixels>,
    pressed: &mut bool,
) {
    window.dispatch_event(
        MouseMoveEvent {
            position,
            pressed_button: pressed.then_some(MouseButton::Left),
            modifiers: Default::default(),
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
    match phase {
        PointerPhase::Move => {}
        PointerPhase::Down => {
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            *pressed = true;
        }
        PointerPhase::Up => {
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Default::default(),
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            *pressed = false;
        }
    }
}

fn action(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    scene: &Entity<Scene>,
    step: &Step,
    pressed: &mut bool,
) -> Result<()> {
    cx.update_window(handle, |_, window, cx| -> Result<()> {
        match step {
            Step::Focus => scene.update(cx, |scene, cx| scene.focus(window, cx)),
            Step::Blur => scene.update(cx, |scene, cx| scene.blur(window, cx)),
            Step::Click => window.click("subject", cx),
            Step::Hover => window.hover("subject", cx),
            Step::MouseDown => pointer(
                window,
                cx,
                PointerPhase::Down,
                window.find("subject").bounds().center(),
                pressed,
            ),
            Step::MouseUp => pointer(
                window,
                cx,
                PointerPhase::Up,
                window.mouse_position(),
                pressed,
            ),
            Step::Pointer { phase, x, y } => {
                ensure!(
                    x.is_finite()
                        && y.is_finite()
                        && (0.0..640.0).contains(x)
                        && (0.0..320.0).contains(y),
                    "invalid pointer coordinates"
                );
                pointer(window, cx, *phase, gpui_kit::point(px(*x), px(*y)), pressed);
            }
            Step::Key { key } => window.press(&keystroke(key)?, cx),
            Step::Wheel { delta_y } => {
                ensure!(delta_y.is_finite(), "invalid scroll delta");
                window.dispatch_event(
                    gpui_kit::ScrollWheelEvent {
                        position: window.find("subject").bounds().center(),
                        delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                            px(0.),
                            px(-*delta_y),
                        )),
                        ..Default::default()
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            Step::Type { text } => window.input(text, cx),
            Step::Select { start, end } => {
                scene.update(cx, |scene, cx| scene.select(*start, *end, cx))?
            }
            Step::Props { props } => {
                scene.update(cx, |scene, cx| scene.patch(props.clone(), window, cx))?
            }
            Step::Checkpoint { .. } => bail!("checkpoint is not an input action"),
        }
        window.render_frame(cx);
        Ok(())
    })??;
    Ok(())
}
fn run_suite(request: &Path, out: &Path, fault: TestFault) -> Result<()> {
    fs::create_dir_all(out)?;
    let manifest = out.join("observations.json");
    if manifest.exists() {
        fs::remove_file(&manifest)?;
    }
    let suite: Suite = serde_json::from_slice(&fs::read(request)?)?;
    ensure!(suite.schema_version == 1, "unsupported schema version");
    ensure!(suite.scale == 2, "capture scale must be 2");
    ensure!(
        matches!(suite.theme.as_str(), "light" | "dark"),
        "unknown theme"
    );
    ensure!(
        suite.viewport.width == 640 && suite.viewport.height == 320,
        "unsupported viewport"
    );
    ensure!(!suite.scenarios.is_empty(), "empty suite");
    let mut ids = HashSet::new();
    let mut captures = Vec::new();
    for scenario in suite.scenarios {
        ensure!(
            safe_id(&scenario.id) && ids.insert(scenario.id.clone()),
            "invalid or duplicate scenario id"
        );
        ensure!(
            matches!(scenario.component.as_str(), "button" | "input" | "textarea"),
            "unknown component"
        );
        scenario.props.validate()?;
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(init);
        let (handle, scene) = cx.update(|cx| {
            gpui_kit::open_window(
                options(suite.viewport.width, suite.viewport.height, false),
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let mut scene = Scene::new(
                            scenario.component.clone(),
                            scenario.props.clone(),
                            suite.theme == "dark",
                            window,
                            cx,
                        );
                        scene.prepare_capture(fault);
                        scene
                    })
                },
            )
        })?;
        cx.update_window(handle, |_, window, _| {
            window.activate_window();
            ensure!(
                window.scale_factor() == suite.scale as f32,
                "native capture scale differs from suite"
            );
            Ok::<_, anyhow::Error>(())
        })??;
        render(&mut cx, handle)?;
        ensure!(
            cx.update_window(handle, |_, window, _| window.is_window_active())?,
            "capture window must be active to paint text selections"
        );
        let mut checkpoints = HashSet::new();
        let mut pressed = false;
        for step in &scenario.steps {
            if let Step::Checkpoint { name, caret, .. } = step {
                ensure!(
                    safe_id(name) && checkpoints.insert(name),
                    "invalid or duplicate checkpoint"
                );
                cx.update_window(handle, |_, _, cx| {
                    scene.update(cx, |scene, cx| scene.set_capture_caret(caret.is_some(), cx))
                })?;
                render(&mut cx, handle)?;
                let (observation, bounds) = cx.update_window(handle, |_, window, cx| {
                    let b = window.find("subject").bounds();
                    (
                        scene.read(cx).observe(window, cx),
                        Rect {
                            x: b.origin.x.into(),
                            y: b.origin.y.into(),
                            width: b.size.width.into(),
                            height: b.size.height.into(),
                        },
                    )
                })?;
                if caret.is_some() {
                    ensure!(
                        observation.focused
                            && !observation.disabled
                            && !observation.readonly
                            && observation
                                .selection
                                .is_some_and(|range| range[0] == range[1]),
                        "visible caret requires a focused editable collapsed selection"
                    );
                }
                let screenshot = format!("{}--{name}.png", scenario.id);
                let image = if caret.is_some() {
                    cx.update_window(handle, |_, _, cx| {
                        scene.update(cx, |scene, cx| scene.set_capture_caret(false, cx))
                    })?;
                    render(&mut cx, handle)?;
                    let hidden = cx.capture_screenshot(handle)?;
                    cx.update_window(handle, |_, _, cx| {
                        scene.update(cx, |scene, cx| scene.set_capture_caret(true, cx))
                    })?;
                    render(&mut cx, handle)?;
                    let visible = cx.capture_screenshot(handle)?;
                    ensure!(
                        hidden.dimensions() == visible.dimensions(),
                        "caret capture size changed"
                    );
                    let mut extent: Option<(u32, u32, u32, u32)> = None;
                    for (x, y, pixel) in visible.enumerate_pixels() {
                        if pixel != hidden.get_pixel(x, y) {
                            extent = Some(match extent {
                                None => (x, y, x, y),
                                Some((left, top, right, bottom)) => {
                                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                                }
                            });
                        }
                    }
                    let (left, top, right, bottom) = extent
                        .context("native caret did not paint; no synthetic cursor fallback")?;
                    let scale = suite.scale as f32;
                    ensure!(
                        right - left < 3 * suite.scale
                            && bottom - top >= 4 * suite.scale
                            && left as f32 >= bounds.x * scale
                            && (right as f32) < (bounds.x + bounds.width) * scale
                            && top as f32 >= bounds.y * scale
                            && (bottom as f32) < (bounds.y + bounds.height) * scale,
                        "visible native caret changed pixels outside a narrow in-control stroke"
                    );
                    let after = cx.update_window(handle, |_, window, cx| {
                        scene.read(cx).observe(window, cx)
                    })?;
                    ensure!(
                        serde_json::to_value(&after)? == serde_json::to_value(&observation)?,
                        "caret capture changed editor state"
                    );
                    visible
                } else {
                    cx.capture_screenshot(handle)
                        .context("real Metal capture unavailable; no fake renderer fallback")?
                };
                image.save(out.join(&screenshot))?;
                captures.push(Capture {
                    scenario: scenario.id.clone(),
                    checkpoint: name.clone(),
                    observation,
                    bounds,
                    screenshot,
                    caret: if caret.is_some() { "visible" } else { "hidden" },
                });
            } else {
                action(&mut cx, handle, &scene, step, &mut pressed)
                    .with_context(|| format!("{}: {step:?}", scenario.id))?;
            }
        }
        ensure!(!checkpoints.is_empty(), "scenario has no checkpoints");
    }
    fs::write(&manifest, serde_json::to_vec_pretty(&captures)?)?;
    println!("GPUI Metal: {} captures ({})", captures.len(), suite.theme);
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--playground-capture")
    {
        ensure!(args.len() == 2, "usage: --playground-capture directory");
        return playground::capture(Path::new(&args[1]));
    }
    if args.first().is_some_and(|arg| arg == "--suite") {
        ensure!(
            (args.len() == 4 || args.len() == 6)
                && args[2] == "--out"
                && (args.len() == 4 || args[4] == "--fault"),
            "usage: --suite request.json --out directory [--fault hide-control|drop-clicks|drop-changes]"
        );
        let fault = if args.len() == 6 {
            match args[5].as_str() {
                "hide-control" => TestFault::HideControl,
                "drop-clicks" => TestFault::DropClicks,
                "drop-changes" => TestFault::DropChanges,
                _ => bail!("unknown test fault"),
            }
        } else {
            TestFault::None
        };
        return run_suite(Path::new(&args[1]), Path::new(&args[3]), fault);
    }
    let mut component = "button".to_string();
    let mut dark = false;
    let mut playground = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dark" => dark = true,
            "--playground" => playground = true,
            "--component" if index + 1 < args.len() => {
                index += 1;
                component = args[index].clone();
            }
            _ => bail!(
                "usage: [--component button|input|textarea] [--dark] [--playground (textarea only)]"
            ),
        }
        index += 1;
    }
    ensure!(
        matches!(component.as_str(), "button" | "input" | "textarea"),
        "unknown component"
    );
    ensure!(
        !playground || component == "textarea",
        "playground requires --component textarea"
    );
    if playground {
        gpui_kit::application().run(move |cx| {
            init(cx);
            gpui_kit::open_window(options(800, 650, true), cx, |window, cx| {
                cx.new(|cx| playground::Playground::new(dark, window, cx))
            })
            .expect("open textarea playground");
            cx.activate(true);
        });
        return Ok(());
    }
    gpui_kit::application().run(move |cx| {
        init(cx);
        gpui_kit::open_window(options(640, 320, true), cx, |window, cx| {
            cx.new(|cx| {
                Scene::new(
                    component,
                    model::Props {
                        placeholder: "Type here".into(),
                        ..Default::default()
                    },
                    dark,
                    window,
                    cx,
                )
            })
        })
        .expect("open prototype window");
        cx.activate(true);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_keys() {
        assert!(safe_id("input-emoji-2"));
        for id in ["", "../escape", "with_underscore", "Capital", "💬"] {
            assert!(!safe_id(id));
        }
        assert_eq!(
            keystroke("ControlOrMeta+Shift+ArrowLeft").unwrap(),
            "cmd-shift-left"
        );
        assert_eq!(keystroke("Meta+ArrowRight").unwrap(), "cmd-right");
        assert_eq!(keystroke("Tab").unwrap(), "tab");
        assert!(keystroke("Unmapped").is_err());
    }
}
