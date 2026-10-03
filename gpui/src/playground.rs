use anyhow::{Result, ensure};
use gpui_kit::{
    Context, Entity, HeadlessAppContext, IntoElement, Render, Window, base::Button, div,
    prelude::*, px, rgb, test::TestWindowExt,
};
use std::{fs, path::Path, sync::Arc};

use crate::{Assets, init, options, render};

use crate::{model::Props, ui::Scene};

const SAMPLE: &str = "A note for the multiline editor ✨\n\nWrite a thought here. This paragraph is deliberately long enough to wrap across several lines when the narrow width preset is selected, while keeping the next paragraph separate.\n\nUnicode: café · 東京 · 🦊\nLast line.";

#[derive(Clone, Copy)]
enum Choice {
    Theme,
    Border,
    Mono,
    Readonly,
    Disabled,
    Width,
    Height,
    Sample,
    Long,
    Clear,
}

pub struct Playground {
    scene: Entity<Scene>,
    props: Props,
    dark: bool,
}

impl Playground {
    pub fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Tau — Multiline Input");
        let props = Props {
            value: SAMPLE.into(),
            width: 700.,
            height: 360.,
            rows: 8,
            ..Default::default()
        };
        let scene = cx.new(|cx| Scene::new("textarea".into(), props.clone(), dark, window, cx));
        Self { scene, props, dark }
    }

    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        let patch = match choice {
            Choice::Theme => {
                self.dark = !self.dark;
                self.scene
                    .update(cx, |scene, cx| scene.set_theme(self.dark, cx));
                None
            }
            Choice::Border => {
                self.props.variant = if self.props.variant == "bare" {
                    ""
                } else {
                    "bare"
                }
                .into();
                Some(serde_json::json!({ "variant": self.props.variant }))
            }
            Choice::Mono => {
                self.props.monospaced = !self.props.monospaced;
                Some(serde_json::json!({ "monospaced": self.props.monospaced }))
            }
            Choice::Readonly => {
                self.props.readonly = !self.props.readonly;
                Some(serde_json::json!({ "readonly": self.props.readonly }))
            }
            Choice::Disabled => {
                self.props.disabled = !self.props.disabled;
                Some(serde_json::json!({ "disabled": self.props.disabled }))
            }
            Choice::Width => {
                self.props.width = if self.props.width == 700. { 420. } else { 700. };
                Some(serde_json::json!({ "width": self.props.width }))
            }
            Choice::Height => {
                self.props.height = if self.props.height == 360. {
                    220.
                } else {
                    360.
                };
                Some(serde_json::json!({ "height": self.props.height }))
            }
            Choice::Sample | Choice::Long | Choice::Clear => {
                let value = match choice {
                    Choice::Sample => SAMPLE.to_string(),
                    Choice::Long => (0..36).map(|n| format!("Paragraph {} — Edit this line, select text, or scroll through the document. 🌿\nA second line wraps naturally when the editor is narrow.\n", n + 1)).collect::<Vec<_>>().join("\n"),
                    _ => String::new(),
                };
                Some(serde_json::json!({ "value": value }))
            }
        };
        if let Some(patch) = patch {
            self.scene
                .update(cx, |scene, cx| scene.patch(patch, window, cx))
                .expect("playground props are valid");
        }
        cx.notify();
    }

    fn button(
        &self,
        id: &'static str,
        label: String,
        choice: Choice,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        Button::new(id)
            .h(px(28.))
            .px(px(9.))
            .border_1()
            .border_color(rgb(0x858d99))
            .rounded(px(4.))
            .hover(move |button| button.bg(rgb(if self.dark { 0x293342 } else { 0xdce5f2 })))
            .focus_visible(|button| button.border_color(rgb(0xe6b450)).border_2())
            .on_click(move |_, window, cx| {
                entity.update(cx, |this, cx| this.choose(choice, window, cx))
            })
            .child(label)
    }
}

impl Render for Playground {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(if self.dark { 0x10141c } else { 0xfcfcfc }))
            .font_family("Inter").text_size(px(12.))
            .text_color(rgb(if self.dark { 0xbfbdb6 } else { 0x5c6166 }))
            .flex().flex_col().gap(px(10.))
            .child(div().p(px(12.)).flex().flex_col().gap(px(8.))
                .child("TEXTAREA PLAYGROUND · Controls change the editor below; content buttons replace its draft.")
                .child(div().flex().flex_wrap().gap(px(6.))
                    .child(self.button("theme", format!("Theme: {}", if self.dark { "Dark" } else { "Light" }), Choice::Theme, cx))
                    .child(self.button("border", format!("Style: {}", if self.props.variant == "bare" { "Bare" } else { "Bordered" }), Choice::Border, cx))
                    .child(self.button("mono", format!("Font: {}", if self.props.monospaced { "Mono" } else { "Regular" }), Choice::Mono, cx))
                    .child(self.button("readonly", format!("Readonly: {}", self.props.readonly), Choice::Readonly, cx))
                    .child(self.button("disabled", format!("Disabled: {}", self.props.disabled), Choice::Disabled, cx))
                    .child(self.button("width", format!("Width: {} px", self.props.width), Choice::Width, cx))
                    .child(self.button("height", format!("Height: {} px", self.props.height), Choice::Height, cx)))
                .child(div().flex().flex_wrap().gap(px(6.))
                    .child(self.button("sample", "Load Sample (replaces draft)".into(), Choice::Sample, cx))
                    .child(self.button("long", "Load Long Text (replaces draft)".into(), Choice::Long, cx))
                    .child(self.button("clear", "Clear (replaces draft)".into(), Choice::Clear, cx))))
            .child(div().flex_1().min_h(px(0.)).child(self.scene.clone()))
    }
}

pub fn capture(out: &Path) -> Result<()> {
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
    let (handle, playground) = cx.update(|cx| {
        gpui_kit::open_window(options(800, 650, false), cx, |window, cx| {
            cx.new(|cx| Playground::new(false, window, cx))
        })
    })?;
    cx.update_window(handle, |_, window, _| {
        window.activate_window();
        ensure!(window.scale_factor() == 2., "expected 2x Metal capture");
        Ok::<_, anyhow::Error>(())
    })??;
    render(&mut cx, handle)?;
    let scene = cx.update(|cx| playground.read(cx).scene.clone());
    let mut checks = Vec::new();
    let steps = [
        ("initial", None),
        ("edited", Some("edit")),
        ("width-narrow", Some("width")),
        ("height-short", Some("height")),
        ("dark", Some("theme")),
        ("light-keyboard", Some("theme-space")),
        ("dark-again", Some("theme")),
        ("mono", Some("mono")),
        ("bare", Some("border")),
        ("readonly", Some("readonly")),
        ("readonly-off", Some("readonly")),
        ("disabled", Some("disabled")),
        ("disabled-off", Some("disabled")),
        ("width-wide", Some("width")),
        ("height-tall", Some("height")),
        ("clear", Some("clear")),
        ("sample", Some("sample")),
        ("long", Some("long")),
    ];
    let mut draft = String::new();
    let mut selection = None;
    for (name, action) in steps {
        cx.update_window(handle, |_, window, cx| {
            if let Some(action) = action {
                if action == "edit" {
                    window.click("subject", cx);
                    window.press("cmd-a", cx);
                    window.input("Personal draft\nSecond line 🦊", cx);
                    window.press("shift-left", cx);
                } else if action == "theme-space" {
                    window.click("theme", cx);
                    window.press("space", cx);
                    ensure!(playground.read(cx).dark, "Space did not activate focused theme button");
                    window.press("space", cx);
                    ensure!(!playground.read(cx).dark, "second Space did not activate theme button");
                } else {
                    window.click(action, cx);
                }
            }
            window.render_frame(cx);
            let current = scene.read(cx).observe(window, cx);
            if name == "edited" {
                draft = current.value.clone();
                selection = current.selection;
                ensure!(draft == "Personal draft\nSecond line 🦊", "typing failed");
                ensure!(selection.is_some_and(|range| range[0] < range[1]), "selection failed");
            } else if action.is_some_and(|a| !matches!(a, "clear" | "sample" | "long")) {
                ensure!(current.value == draft, "{name} lost the draft");
                ensure!(current.selection == selection, "{name} lost selection");
            }
            ensure!(playground.read(cx).scene == scene, "theme recreated editor scene");
            if name == "dark" || name == "dark-again" {
                ensure!(playground.read(cx).dark, "theme switch failed");
            }
            for id in ["theme", "border", "mono", "readonly", "disabled", "width", "height", "sample", "long", "clear", "subject"] {
                let b = window.find(id).bounds();
                ensure!(b.origin.x >= gpui_kit::px(0.) && b.origin.y >= gpui_kit::px(0.) && b.origin.x + b.size.width <= gpui_kit::px(800.) && b.origin.y + b.size.height <= gpui_kit::px(650.), "{name}: {id} outside viewport: {b:?}");
            }
            checks.push(serde_json::json!({"checkpoint": name, "observation": current, "screenshot": format!("{name}.png")}));
            Ok::<_, anyhow::Error>(())
        })??;
        render(&mut cx, handle)?;
        let image = cx.capture_screenshot(handle)?;
        ensure!(
            image.dimensions() == (1600, 1300),
            "unexpected capture dimensions"
        );
        image.save(out.join(format!("{name}.png")))?;
        if name == "height-tall" {
            cx.update_window(handle, |_, window, cx| {
                scene.update(cx, |scene, cx| scene.focus(window, cx));
                window.render_frame(cx);
                // Typing spans newline undo groups; verify history continuity, not its grouping policy.
                let mut undos = 0;
                while scene.read(cx).observe(window, cx).value != SAMPLE && undos < 64 {
                    window.press("cmd-z", cx);
                    window.render_frame(cx);
                    undos += 1;
                }
                ensure!(
                    scene.read(cx).observe(window, cx).value == SAMPLE,
                    "control changes lost undo history"
                );
                for _ in 0..undos {
                    window.press("cmd-shift-z", cx);
                    window.render_frame(cx);
                }
                ensure!(
                    scene.read(cx).observe(window, cx).value == draft,
                    "control changes lost redo history"
                );
                Ok::<_, anyhow::Error>(())
            })??;
        }
        if name == "clear" || name == "sample" || name == "long" {
            draft = cx.update_window(handle, |_, window, cx| {
                scene.read(cx).observe(window, cx).value
            })?;
            selection = cx.update_window(handle, |_, window, cx| {
                scene.read(cx).observe(window, cx).selection
            })?;
        }
    }
    fs::write(manifest, serde_json::to_vec_pretty(&checks)?)?;
    Ok(())
}
