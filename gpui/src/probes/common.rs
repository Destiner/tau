#![allow(dead_code)]

use anyhow::{Result, bail, ensure};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, AssetSource, Bounds, Context, ElementId, Entity,
    HeadlessAppContext, InputEvent, IntoElement, Render, SharedString, Window, WindowBounds,
    WindowOptions,
    base::{Button, ScrollbarStyles, Theme, ThemeAppearance},
    div,
    prelude::*,
    px, rgb, size,
    test::TestWindowExt,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{borrow::Cow, collections::HashSet, fs, path::Path, sync::Arc, time::Duration};

#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: u32,
    pub surface: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub border: u32,
    pub hover: u32,
    pub danger: u32,
    pub code: u32,
}
impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                canvas: 0x10141c,
                surface: 0x171d27,
                text: 0xbfbdb6,
                muted: 0x6c7380,
                accent: 0xe6b450,
                border: 0x303744,
                hover: 0x232c38,
                danger: 0xd95757,
                code: 0x0b0f16,
            }
        } else {
            Self {
                canvas: 0xfcfcfc,
                surface: 0xf4f5f6,
                text: 0x5c6166,
                muted: 0x6e757f,
                accent: 0xf29718,
                border: 0xd9dee2,
                hover: 0xeeeff1,
                danger: 0xe65050,
                code: 0xf1f3f5,
            }
        }
    }
}
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, p: Palette) -> Button {
    Button::new(id)
        .px(px(10.))
        .py(px(6.))
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(p.border))
        .bg(rgb(p.surface))
        .text_color(rgb(p.text))
        .text_size(px(12.))
        .hover(move |s| s.bg(rgb(p.hover)))
        .focus_visible(move |s| s.border_color(rgb(p.accent)))
        .child(label.into())
}
pub fn page(p: Palette) -> gpui_kit::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(20.))
        .bg(rgb(p.canvas))
        .text_color(rgb(p.text))
        .font_family("Inter")
        .text_size(px(13.))
        .line_height(gpui_kit::relative(1.45))
}
pub fn text_style(p: Palette) -> gpui_kit::base::input::InputEditorStyle {
    gpui_kit::base::input::InputEditorStyle {
        foreground: rgb(p.text).into(),
        muted_foreground: rgb(p.muted).into(),
        background: rgb(p.canvas).into(),
        border: rgb(p.border).into(),
        caret: rgb(p.accent).into(),
        caret_width: Some(px(2.)),
        center_caret: true,
        ..Default::default()
    }
}
pub fn visible(window: &Window, id: &'static str) -> bool {
    gpui_kit::base::test_support::find(window, &[], &id.into()).is_some_and(|s| s.visible())
}
pub fn inside(window: &Window, id: &'static str, width: f32, height: f32) -> Result<Value> {
    let b = window.find(id).bounds();
    ensure!(
        b.size.width > px(0.)
            && b.size.height > px(0.)
            && b.left() >= px(-0.1)
            && b.top() >= px(-0.1)
            && b.right() <= px(width + 0.1)
            && b.bottom() <= px(height + 0.1),
        "{id} must be inside viewport: {b:?}"
    );
    Ok(
        json!({"x": f32::from(b.left()), "y": f32::from(b.top()), "width": f32::from(b.size.width), "height": f32::from(b.size.height)}),
    )
}

#[derive(Clone)]
pub enum Step {
    Click(&'static str),
    RightClick(&'static str),
    Hover(&'static str),
    Key(&'static str),
    Type(&'static str),
    Wheel(&'static str, f32),
    WheelXAt { x: f32, y: f32, delta: f32 },
    Pointer { phase: &'static str, x: f32, y: f32 },
    Wait(u64),
    Check(&'static str),
}
pub trait Probe: Render + Sized + 'static {
    const ID: &'static str;
    const TITLE: &'static str;
    const DESCRIPTION: &'static str;
    const LIMITATIONS: &'static str;
    const WIDTH: u32 = 900;
    const HEIGHT: u32 = 700;
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self;
    fn steps() -> Vec<Step>;
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value>;
}
struct Assets;
impl AssetSource for Assets {
    fn load(&self, _: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }
    fn list(&self, _: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
struct Host<P: Probe> {
    scene: Entity<P>,
    hidden: bool,
}
impl<P: Probe> Render for Host<P> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .opacity(if self.hidden { 0. } else { 1. })
            .child(self.scene.clone())
    }
}
fn init(cx: &mut App, dark: bool) {
    crate::text::init();
    gpui_kit::init(cx);
    let theme = Theme::global_mut(cx);
    theme.appearance = if dark {
        ThemeAppearance::Dark
    } else {
        ThemeAppearance::Light
    };
    theme.tokens.colors.foreground = rgb(Palette::new(dark).text).into();
    theme.scrollbar = theme
        .scrollbar
        .clone()
        .with_styles(ScrollbarStyles::default().thumb(|s| s.radius(px(4.))));
    cx.text_system()
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../../assets/InterVariable.ttf"
        ))])
        .expect("bundled Inter font");
}
fn options<P: Probe>(show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Default::default(),
            size: size(px(P::WIDTH as f32), px(P::HEIGHT as f32)),
        })),
        show,
        focus: show,
        ..Default::default()
    }
}
fn frame(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) -> Result<()> {
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| w.render_frame(cx))?;
    Ok(())
}
#[derive(Serialize)]
struct Check {
    name: String,
    passed: bool,
    observations: Value,
    screenshot: String,
    ink_pixels: usize,
}
fn capture<P: Probe>(out: &Path, dark: bool, fault: Option<&str>) -> Result<()> {
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
    cx.update(|cx| init(cx, dark));
    let (handle, host) = cx.update(|cx| {
        gpui_kit::open_window(options::<P>(false), cx, |window, cx| {
            let scene = cx.new(|cx| P::new(dark, window, cx));
            cx.new(|_| Host {
                scene,
                hidden: fault == Some("hide-content"),
            })
        })
    })?;
    let scene = host.read_with(&cx, |h, _| h.scene.clone());
    cx.update_window(handle, |_, window, _| {
        window.activate_window();
        ensure!(
            window.scale_factor() == 2.,
            "Metal capture requires scale 2"
        );
        Ok::<_, anyhow::Error>(())
    })??;
    frame(&mut cx, handle)?;
    let mut checks = Vec::new();
    let mut held = false;
    let mut names = HashSet::new();
    for step in P::steps() {
        if let Step::Check(name) = step {
            ensure!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    && names.insert(name),
                "invalid or duplicate check name"
            );
            frame(&mut cx, handle)?;
            let result = cx.update_window(handle, |_, window, cx| {
                ensure!(window.is_window_active(), "capture window inactive");
                scene.read(cx).check(name, window, cx)
            })?;
            let image = cx.capture_screenshot(handle)?;
            ensure!(
                image.dimensions() == (P::WIDTH * 2, P::HEIGHT * 2),
                "unexpected Metal frame dimensions"
            );
            let bg = image.get_pixel(0, 0);
            let ink_pixels = image
                .pixels()
                .filter(|pixel| {
                    pixel.0[..3]
                        .iter()
                        .zip(&bg.0[..3])
                        .any(|(a, b)| a.abs_diff(*b) > 8)
                })
                .count();
            let screenshot = format!("{name}.png");
            image.save(out.join(&screenshot))?;
            let passed = result.is_ok() && ink_pixels > 400;
            let observations = result.unwrap_or_else(|error| json!({"error": error.to_string()}));
            checks.push(Check {
                name: name.into(),
                passed,
                observations,
                screenshot,
                ink_pixels,
            });
        } else if let Step::Wait(ms) = step {
            ensure!(ms <= 5000, "unbounded wait");
            cx.advance_clock(Duration::from_millis(ms));
            frame(&mut cx, handle)?;
        } else {
            if fault == Some("skip-input") {
                continue;
            }
            cx.update_window(handle, |_, window, cx| {
                match step {
                    Step::Click(id) => window.click(id, cx),
                    Step::RightClick(id) => window.right_click(id, cx),
                    Step::Hover(id) => window.hover(id, cx),
                    Step::Key(key) => window.press(key, cx),
                    Step::Type(text) => window.input(text, cx),
                    Step::Wheel(id, delta) => {
                        window.dispatch_event(
                            gpui_kit::ScrollWheelEvent {
                                position: window.find(id).bounds().center(),
                                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                    px(0.),
                                    px(-delta),
                                )),
                                ..Default::default()
                            }
                            .to_platform_input(),
                            cx,
                        );
                    }
                    Step::WheelXAt { x, y, delta } => {
                        ensure!(
                            x.is_finite() && y.is_finite() && delta.is_finite(),
                            "invalid horizontal wheel event"
                        );
                        window.dispatch_event(
                            gpui_kit::ScrollWheelEvent {
                                position: gpui_kit::point(px(x), px(y)),
                                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                    px(-delta),
                                    px(0.),
                                )),
                                ..Default::default()
                            }
                            .to_platform_input(),
                            cx,
                        );
                    }
                    Step::Pointer { phase, x, y } => {
                        let position = gpui_kit::point(px(x), px(y));
                        let event = match phase {
                            "down" => {
                                held = true;
                                gpui_kit::MouseDownEvent {
                                    button: gpui_kit::MouseButton::Left,
                                    position,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                    first_mouse: false,
                                }
                                .to_platform_input()
                            }
                            "up" => {
                                held = false;
                                gpui_kit::MouseUpEvent {
                                    button: gpui_kit::MouseButton::Left,
                                    position,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                }
                                .to_platform_input()
                            }
                            "move" => gpui_kit::MouseMoveEvent {
                                position,
                                pressed_button: held.then_some(gpui_kit::MouseButton::Left),
                                modifiers: Default::default(),
                            }
                            .to_platform_input(),
                            _ => bail!("invalid pointer phase"),
                        };
                        window.dispatch_event(event, cx);
                    }
                    Step::Wait(_) | Step::Check(_) => unreachable!(),
                }
                Ok::<_, anyhow::Error>(())
            })??;
            frame(&mut cx, handle)?;
        }
    }
    ensure!(!held && !checks.is_empty(), "unfinished or empty capture");
    fs::write(manifest, serde_json::to_vec_pretty(&checks)?)?;
    let failed: Vec<_> = checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| &c.name)
        .collect();
    ensure!(failed.is_empty(), "{} failed checks: {failed:?}", P::ID);
    println!("{}: {} real Metal checks passed", P::ID, checks.len());
    Ok(())
}
pub fn run<P: Probe>() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--list-checks"] {
        let checks: Vec<_> = P::steps()
            .into_iter()
            .filter_map(|s| {
                if let Step::Check(name) = s {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        println!(
            "{}",
            json!({"id": P::ID, "title": P::TITLE, "description": P::DESCRIPTION, "limitations": P::LIMITATIONS, "width": P::WIDTH, "height": P::HEIGHT, "checks": checks})
        );
        return Ok(());
    }
    let mut dark = false;
    let mut out = None;
    let mut fault = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dark" => dark = true,
            "--capture" if i + 1 < args.len() => {
                i += 1;
                out = Some(Path::new(&args[i]));
            }
            "--fault" if i + 1 < args.len() => {
                i += 1;
                fault = Some(args[i].as_str());
            }
            _ => bail!("usage: [--dark] [--capture directory [--fault hide-content|skip-input]]"),
        }
        i += 1;
    }
    ensure!(
        fault.is_none() || (out.is_some() && matches!(fault, Some("hide-content" | "skip-input"))),
        "faults require capture mode"
    );
    if let Some(out) = out {
        return capture::<P>(out, dark, fault);
    }
    gpui_kit::application().run(move |cx| {
        init(cx, dark);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        gpui_kit::open_window(options::<P>(true), cx, |window, cx| {
            window.set_window_title(P::TITLE);
            cx.new(|cx| P::new(dark, window, cx))
        })
        .expect("open isolated UI probe");
        cx.activate(true);
    });
    Ok(())
}
