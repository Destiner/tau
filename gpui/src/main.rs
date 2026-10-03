mod model;
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
}

fn init(cx: &mut gpui_kit::App) {
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
            if let Step::Checkpoint { name, .. } = step {
                ensure!(
                    safe_id(name) && checkpoints.insert(name),
                    "invalid or duplicate checkpoint"
                );
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
                let screenshot = format!("{}--{name}.png", scenario.id);
                cx.capture_screenshot(handle)
                    .context("real Metal capture unavailable; no fake renderer fallback")?
                    .save(out.join(&screenshot))?;
                captures.push(Capture {
                    scenario: scenario.id.clone(),
                    checkpoint: name.clone(),
                    observation,
                    bounds,
                    screenshot,
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
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dark" => dark = true,
            "--component" if index + 1 < args.len() => {
                index += 1;
                component = args[index].clone();
            }
            _ => bail!("usage: [--component button|input|textarea] [--dark]"),
        }
        index += 1;
    }
    ensure!(
        matches!(component.as_str(), "button" | "input" | "textarea"),
        "unknown component"
    );
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
