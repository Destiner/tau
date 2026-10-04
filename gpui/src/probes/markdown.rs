use anyhow::{Result, ensure};
use gpui_kit::{
    App, ClipboardItem, Context, Entity, IntoElement, Render, TestSupportExt, Window,
    base::{TextView, TextViewState, TextViewStyle},
    div,
    prelude::*,
    px, rgb,
};
use serde_json::{Value, json};

use super::{
    highlight,
    probe::{Palette, Probe, Step, button, inside, page, visible},
};

const INTRO: &str = "# Markdown in Metal\nA **selectable** paragraph with `inline code`, emphasis, and a [safe link](https://example.invalid/).\n\n- First item\n- Second item\n\n| Item | Value |\n| --- | --- |\n| Native | GPUI |\n\n```rust\nfn answer() -> u32 { 42 }\n```\n";
const MIXED: &str = "# Mixed code\nRust, JSON, Unicode and an unknown grammar.\n\n```rust\nfn café() { println!(\"🦀\"); }\n```\n\n```json\n{\"ready\": true, \"count\": 3}\n```\n\n```unknown-grammar\nplain text\n```\n\n```rust\n```\n";
const LONG: &str = "# Long code\nThe fence below has a long line; code scrolls horizontally without wrapping.\n\n```rust\nlet long_identifier = \"alpha_beta_gamma_delta_epsilon_zeta_eta_theta_iota_kappa_lambda_mu_nu_xi_omicron_pi_rho_sigma_tau_upsilon_phi_chi_psi_omega\";\n```\n";
const STREAM: &str =
    "# Streaming fence\nThe following Rust block is not yet closed.\n\n```rust\nfn stream() {\n";
const TALL_LINE: &str =
    "\n\n## Section\nA paragraph to exercise Base's scrollable block list with selectable text.\n";
const STREAM_CHUNKS: [&str; 3] = [
    "    let value = 42;\n",
    "    println!(\"{value}\");\n",
    "}\n```\n\nFence complete.\n",
];

pub struct MarkdownProbe {
    palette: Palette,
    view: Entity<TextViewState>,
    source: String,
    mode: &'static str,
    chunk: usize,
    copied: Option<String>,
    highlighter: highlight::CodeHighlighter,
    activated: Option<String>,
}

impl MarkdownProbe {
    fn replace(&mut self, mode: &'static str, source: &str, cx: &mut Context<Self>) {
        self.mode = mode;
        self.source = source.into();
        self.chunk = 0;
        self.copied = None;
        self.activated = None;
        self.view.update(cx, |view, cx| view.set_text(source, cx));
        cx.notify();
    }
    fn step(&mut self, cx: &mut Context<Self>) {
        if self.mode != "stream" {
            self.replace("stream", STREAM, cx);
        }
        if let Some(chunk) = STREAM_CHUNKS.get(self.chunk) {
            self.source.push_str(chunk);
            self.chunk += 1;
            self.view.update(cx, |view, cx| view.push_str(chunk, cx));
            cx.notify();
        }
    }
}

impl Probe for MarkdownProbe {
    const ID: &'static str = "markdown";
    const TITLE: &'static str = "Markdown · Native GPUI";
    const DESCRIPTION: &'static str =
        "Base TextView Markdown, code fences, selection and incremental parsing";
    const LIMITATIONS: &'static str = "No file preview, external navigation, streaming timer or editable source control; links activate locally. Copy Code writes to the clipboard only when clicked.";
    fn new(dark: bool, _: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            palette: Palette::new(dark),
            view: cx.new(|cx| TextViewState::markdown(INTRO, cx)),
            source: INTRO.into(),
            mode: "intro",
            chunk: 0,
            copied: None,
            highlighter: highlight::code_block_highlighter(dark),
            activated: None,
        }
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Check("initial"),
            Step::Pointer {
                phase: "down",
                x: 427.,
                y: 197.,
            },
            Step::Pointer {
                phase: "up",
                x: 427.,
                y: 197.,
            },
            Step::Check("safe-link"),
            Step::Click("copy-short"),
            Step::Check("copied-short"),
            Step::Pointer {
                phase: "down",
                x: 50.,
                y: 371.,
            },
            Step::Pointer {
                phase: "move",
                x: 195.,
                y: 371.,
            },
            Step::Pointer {
                phase: "up",
                x: 195.,
                y: 371.,
            },
            Step::Check("code-selection"),
            Step::Click("mixed"),
            Step::Check("mixed-code"),
            Step::Click("long"),
            Step::Check("long-code"),
            Step::Pointer {
                phase: "down",
                x: 100.,
                y: 245.,
            },
            Step::Pointer {
                phase: "move",
                x: 335.,
                y: 245.,
            },
            Step::Pointer {
                phase: "up",
                x: 335.,
                y: 245.,
            },
            Step::Check("long-selection"),
            Step::WheelXAt {
                x: 400.,
                y: 245.,
                delta: 850.,
            },
            Step::Check("long-scrolled"),
            Step::Pointer {
                phase: "down",
                x: 100.,
                y: 245.,
            },
            Step::Pointer {
                phase: "move",
                x: 335.,
                y: 245.,
            },
            Step::Pointer {
                phase: "up",
                x: 335.,
                y: 245.,
            },
            Step::Check("scrolled-selection"),
            Step::WheelXAt {
                x: 400.,
                y: 245.,
                delta: 4000.,
            },
            Step::Check("long-end"),
            Step::Pointer {
                phase: "down",
                x: 689.,
                y: 245.,
            },
            Step::Pointer {
                phase: "move",
                x: 746.,
                y: 245.,
            },
            Step::Pointer {
                phase: "up",
                x: 746.,
                y: 245.,
            },
            Step::Check("end-selection"),
            Step::WheelXAt {
                x: 400.,
                y: 245.,
                delta: -5000.,
            },
            Step::Check("long-restored"),
            Step::Pointer {
                phase: "down",
                x: 100.,
                y: 245.,
            },
            Step::Pointer {
                phase: "move",
                x: 335.,
                y: 245.,
            },
            Step::Pointer {
                phase: "up",
                x: 335.,
                y: 245.,
            },
            Step::Check("restored-selection"),
            Step::Click("copy-long"),
            Step::Check("copied-long"),
            Step::Click("stream"),
            Step::Check("unclosed-fence"),
            Step::Click("step"),
            Step::Check("stream-appended"),
            Step::Click("step"),
            Step::Click("step"),
            Step::Check("fence-closed"),
            Step::Click("tall"),
            Step::Check("tall-top"),
            Step::Wheel("markdown-area", 350.),
            Step::Check("tall-scrolled"),
            Step::Click("reset"),
            Step::Check("reset"),
            Step::Pointer {
                phase: "down",
                x: 47.,
                y: 157.,
            },
            Step::Pointer {
                phase: "move",
                x: 215.,
                y: 157.,
            },
            Step::Pointer {
                phase: "up",
                x: 215.,
                y: 157.,
            },
            Step::Check("pointer-selection"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let rendered = self.view.read(cx).rendered_text();
        let text = rendered.as_str();
        let bounds = inside(
            window,
            "markdown-area",
            Self::WIDTH as f32,
            Self::HEIGHT as f32,
        )?;
        let state = self.view.read(cx);
        ensure!(
            state.list_state().item_count() > 0,
            "Base did not lay out Markdown blocks"
        );
        match name {
            "initial" | "reset" => {
                ensure!(
                    self.mode == "intro" && self.source == INTRO,
                    "intro not restored"
                );
                ensure!(
                    text.contains("Markdown in Metal")
                        && text.contains("Native")
                        && text.contains("answer()")
                        && !text.contains("```"),
                    "Markdown was not parsed to rendered text: {text}"
                );
                ensure!(
                    visible(window, "copy-short"),
                    "short code action not visible"
                );
            }
            "safe-link" => ensure!(
                self.activated.as_deref() == Some("https://example.invalid/"),
                "Base link handler did not receive safe destination"
            ),
            "copied-short" => ensure!(
                self.copied.as_deref() == Some("fn answer() -> u32 { 42 }")
                    && cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref()
                        == Some("fn answer() -> u32 { 42 }"),
                "code action did not write Base code to clipboard"
            ),
            "code-selection" => ensure!(
                state.selected_text().contains("fn answer")
                    && state.selected_source_range().is_some(),
                "pointer drag did not select the code block"
            ),
            "mixed-code" => {
                ensure!(
                    self.mode == "mixed"
                        && text.contains("café")
                        && text.contains("ready")
                        && text.contains("plain text"),
                    "mixed fences not parsed"
                );
                ensure!(
                    highlight::highlight(
                        &gpui_kit::base::text::CodeBlock::from_code(
                            "fn café() { println!(\"🦀\"); }",
                            Some("rust")
                        ),
                        self.palette.canvas == 0x10141c
                    )
                    .len()
                        > 3,
                    "Rust syntax not highlighted"
                );
            }
            "long-code" | "long-selection" | "long-scrolled" | "scrolled-selection"
            | "long-end" | "end-selection" | "long-restored" | "restored-selection"
            | "copied-long" => {
                ensure!(
                    self.mode == "long"
                        && text.contains("long_identifier")
                        && !text.contains("answer()"),
                    "long content not parsed"
                );
                ensure!(visible(window, "copy-long"), "long code action missing");
                let action = inside(window, "copy-long", Self::WIDTH as f32, Self::HEIGHT as f32)?;
                ensure!(
                    action["y"]
                        .as_f64()
                        .is_some_and(|y| (220. ..290.).contains(&y))
                        && action["x"].as_f64().is_some_and(|x| x > 730. && x < 840.),
                    "code action should stay pinned in a single-line-height block: {action}"
                );
                if name == "scrolled-selection" {
                    ensure!(
                        state.selected_text().contains("eta_iota_kappa"),
                        "scrolled pointer selection: {:?}",
                        state.selected_text()
                    );
                }
                if name == "end-selection" {
                    ensure!(
                        state.selected_text().contains("omega"),
                        "last glyph must remain selectable clear of the copy action: {:?}",
                        state.selected_text()
                    );
                }
                if name == "restored-selection" {
                    ensure!(
                        state.selected_text().contains("identifier ="),
                        "reverse scroll should restore pointer geometry: {:?}",
                        state.selected_text()
                    );
                }
                if name == "long-selection" {
                    ensure!(
                        state.selected_text().contains("identifier =")
                            && state.selected_text().contains("alpha_beta"),
                        "code pointer selection must select text inside the clipped viewport: {:?}",
                        state.selected_text()
                    );
                }
                if name == "copied-long" {
                    ensure!(
                        self.copied.as_deref().is_some_and(|s| s.contains("omega"))
                            && cx.read_from_clipboard().and_then(|item| item.text()) == self.copied,
                        "long block copy missing end of line"
                    );
                }
            }
            "unclosed-fence" => ensure!(
                self.mode == "stream"
                    && text.contains("fn stream()")
                    && !text.contains("Fence complete"),
                "unclosed fence did not render"
            ),
            "stream-appended" => ensure!(
                self.chunk == 1
                    && text.contains("let value = 42")
                    && !text.contains("Fence complete"),
                "incremental parse missing first chunk"
            ),
            "fence-closed" => ensure!(
                self.chunk == 3 && text.contains("Fence complete") && text.contains("println!"),
                "closed fence not parsed"
            ),
            "tall-top" => ensure!(
                self.mode == "tall"
                    && state.list_state().item_count() > 20
                    && state.list_state().logical_scroll_top().item_ix == 0,
                "tall document did not start at top"
            ),
            "tall-scrolled" => ensure!(
                self.mode == "tall" && state.list_state().logical_scroll_top().item_ix > 0,
                "wheel did not move Base block list"
            ),
            "pointer-selection" => ensure!(
                !state.selected_text().is_empty() && state.selected_source_range().is_some(),
                "pointer drag did not select rendered Markdown"
            ),
            _ => anyhow::bail!("unknown check {name}"),
        }
        Ok(
            json!({"mode": self.mode, "source_bytes": self.source.len(), "rendered_bytes": text.len(), "blocks": state.list_state().item_count(), "selected": state.selected_text(), "copied_bytes": self.copied.as_ref().map(String::len), "bounds": bounds}),
        )
    }
}

impl Render for MarkdownProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let scene = cx.entity().clone();
        let style = TextViewStyle::default()
            .with_code_block_horizontal_scroll(true)
            .with_foreground(rgb(p.text).into())
            .with_muted_foreground(rgb(p.muted).into())
            .with_link(
                rgb(if p.canvas == 0x10141c {
                    0x59c2ff
                } else {
                    0x22a4e6
                })
                .into(),
            )
            .with_code_background(rgb(p.code).into())
            .with_border(rgb(p.border).into())
            .with_code_block(
                gpui_kit::StyleRefinement::default()
                    .rounded(px(9.))
                    .border_1()
                    .border_color(rgb(p.border)),
            );
        let link_scene = scene.clone();
        let copy_scene = scene.clone();
        page(p)
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Markdown / code blocks"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button("reset", "Reset", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.replace("intro", INTRO, cx))
                    }))
                    .child(button("mixed", "Mixed Code", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.replace("mixed", MIXED, cx))
                    }))
                    .child(button("long", "Long Code", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.replace("long", LONG, cx))
                    }))
                    .child(button("stream", "Open Fence", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.replace("stream", STREAM, cx))
                    }))
                    .child(
                        button("step", "Append Chunk", p)
                            .on_click(move |_, _, cx| scene.update(cx, |s, cx| s.step(cx))),
                    )
                    .child(button("tall", "Tall Page", p).on_click({
                        let scene = cx.entity().clone();
                        move |_, _, cx| {
                            scene.update(cx, |s, cx| {
                                let source = format!("# Tall Markdown{}", TALL_LINE.repeat(24));
                                s.replace("tall", &source, cx);
                            })
                        }
                    })),
            )
            .child(div().text_color(rgb(p.muted)).child(format!(
                    "{} · {} chunks · {}",
                    self.mode,
                    self.chunk,
                    self.copied
                        .as_ref()
                        .map_or("Copy Code → clipboard", |_| "Code copied to clipboard")
                )))
            .child(
                div()
                    .id("markdown-area")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .p(px(16.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.surface))
                    .child(
                        TextView::new(&self.view)
                            .scrollable(true)
                            .size_full()
                            .style(style)
                            .shared_code_block_highlighter(self.highlighter.clone())
                            .on_link_click(move |url, _, _, cx| {
                                let url = url.to_string();
                                link_scene.update(cx, |s, cx| {
                                    s.activated = Some(url);
                                    cx.notify();
                                });
                            })
                            .code_block_actions(move |block, _, _| {
                                let code = block.code().to_string();
                                let id = if code.contains("long_identifier") {
                                    "copy-long"
                                } else {
                                    "copy-short"
                                };
                                let scene = copy_scene.clone();
                                button(id, "Copy Code", p).on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                    scene.update(cx, |s, cx| {
                                        s.copied = Some(code.clone());
                                        cx.notify();
                                    });
                                })
                            }),
                    ),
            )
            .child(div().text_color(rgb(p.muted)).text_size(px(11.)).child(
                self.activated.as_ref().map_or(
                    "Links are simulated; no external navigation.",
                    |_| "Link activated locally.",
                ),
            ))
    }
}
