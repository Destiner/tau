use std::sync::Arc;

use anyhow::{Result, ensure};
use gpui_kit::{
    App, ClipboardItem, Context, Entity, FocusHandle, Focusable, Image, ImageFormat, IntoElement,
    ObjectFit, Render, Window,
    base::{TestSupportExt, Textarea, input::TextareaState},
    div, img,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};

use super::probe::{self, Palette, Probe, Step};

const PLAIN: &str = "A synthetic note. Select any words and copy them.\nNo filesystem or network access is involved.\n";
const CODE: &str = "fn main() {\n    println!(\"Synthetic source 🦀\");\n}\n";
const LONG_LINE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const IMAGE: &[u8] = include_bytes!("../../assets/probes/files/landscape.png");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum File {
    Note,
    Code,
    Long,
    Image,
    Failure,
    Unsupported,
}
impl File {
    fn name(self) -> &'static str {
        match self {
            Self::Note => "notes.txt",
            Self::Code => "sample.rs",
            Self::Long => "wide.log",
            Self::Image => "landscape.png",
            Self::Failure => "missing.txt",
            Self::Unsupported => "archive.zip",
        }
    }
    fn source(self) -> Option<String> {
        match self {
            Self::Note => Some(PLAIN.into()),
            Self::Code => Some(CODE.into()),
            Self::Long => Some(format!(
                "{}\n",
                format!("{}{}\n", LONG_LINE.repeat(30), " end").repeat(120)
            )),
            _ => None,
        }
    }
}
const FILES: [File; 6] = [
    File::Note,
    File::Code,
    File::Long,
    File::Image,
    File::Failure,
    File::Unsupported,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    Ready,
    Error,
    Unsupported,
}

pub struct FilesProbe {
    p: Palette,
    active: Option<File>,
    phase: Phase,
    revision: u64,
    pending: Option<(u64, File)>,
    stale: Option<(u64, File)>,
    retries: usize,
    copied: bool,
    source: Option<String>,
    text: Entity<TextareaState>,
    image: Arc<Image>,
    triggers: Vec<FocusHandle>,
    close_focus: FocusHandle,
    copy_focus: FocusHandle,
    retry_focus: FocusHandle,
    finish_focus: FocusHandle,
    _keys: gpui_kit::Subscription,
}
impl FilesProbe {
    fn open(&mut self, file: File, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((ticket, previous)) = self.pending.take() {
            self.stale = Some((ticket, previous));
        }
        self.revision += 1;
        self.active = Some(file);
        self.phase = if file == File::Unsupported {
            Phase::Unsupported
        } else {
            Phase::Loading
        };
        self.pending = (self.phase == Phase::Loading).then_some((self.revision, file));
        self.source = None;
        self.copied = false;
        self.text.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        self.close_focus.focus(window, cx);
        cx.notify();
    }
    fn complete(&mut self, ticket: (u64, File), window: &mut Window, cx: &mut Context<Self>) {
        if self.pending != Some(ticket)
            || self.active != Some(ticket.1)
            || self.revision != ticket.0
        {
            return;
        }
        self.pending = None;
        if ticket.1 == File::Failure {
            self.phase = Phase::Error;
            self.retry_focus.focus(window, cx);
        } else {
            self.phase = Phase::Ready;
            self.source = ticket.1.source();
            if let Some(source) = &self.source {
                self.text
                    .update(cx, |state, cx| state.set_value(source.clone(), window, cx));
            }
            self.close_focus.focus(window, cx);
        }
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file) = self.active.take() {
            self.revision += 1;
            self.pending = None;
            self.source = None;
            self.triggers[FILES.iter().position(|item| *item == file).unwrap()].focus(window, cx);
            cx.notify();
        }
    }
    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        focus: &FocusHandle,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui_kit::base::Button {
        probe::button(id, label, self.p)
            .track_focus(focus)
            .on_click(cx.listener(move |s, _, w, cx| handler(s, w, cx)))
    }
}
impl Probe for FilesProbe {
    const ID: &'static str = "files";
    const TITLE: &'static str = "Tau · Native File Viewer";
    const DESCRIPTION: &'static str =
        "Synthetic text, source and image preview with owned loading, retry and focus";
    const LIMITATIONS: &'static str = "Fixture-only controlled completion; no filesystem, Pi, network, Markdown rendering, syntax highlighting, PDF, video, audio or OS file chooser. Text uses a readonly Base textarea with soft wrapping; window resizing is not driven by the capture harness.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = Palette::new(dark);
        let text = cx.new(|cx| TextareaState::new(window, cx));
        text.update(cx, |s, cx| {
            s.set_readonly(true, cx);
            s.set_editor_style(probe::text_style(p));
        });
        let text_focus = text.read(cx).focus_handle(cx).clone();
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key == "tab"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
                && let Some(scene) = weak.upgrade()
                && scene.read(cx).active.is_some()
            {
                let state = scene.read(cx);
                let extra = if state.phase == Phase::Error {
                    Some(&state.retry_focus)
                } else if state.phase == Phase::Ready && state.source.is_some() {
                    Some(&state.copy_focus)
                } else {
                    None
                };
                let mut cycle = vec![&state.close_focus];
                if let Some(extra) = extra {
                    cycle.push(extra);
                }
                if state.phase == Phase::Ready && state.source.is_some() {
                    cycle.push(&text_focus);
                }
                let index = cycle
                    .iter()
                    .position(|handle| handle.is_focused(window))
                    .unwrap_or(0);
                let next = if event.keystroke.modifiers.shift {
                    (index + cycle.len() - 1) % cycle.len()
                } else {
                    (index + 1) % cycle.len()
                };
                let next_focus = cycle[next].clone();
                next_focus.focus(window, cx);
                cx.stop_propagation();
            }
            if event.keystroke.key == "escape"
                && let Some(scene) = weak.upgrade()
                && scene.read(cx).active.is_some()
            {
                scene.update(cx, |s, cx| s.close(window, cx));
                cx.stop_propagation();
            }
        });
        Self {
            p,
            active: None,
            phase: Phase::Ready,
            revision: 0,
            pending: None,
            stale: None,
            retries: 0,
            copied: false,
            source: None,
            text,
            image: Arc::new(Image::from_bytes(ImageFormat::Png, IMAGE.to_vec())),
            triggers: FILES.iter().map(|_| cx.focus_handle()).collect(),
            close_focus: cx.focus_handle(),
            copy_focus: cx.focus_handle(),
            retry_focus: cx.focus_handle(),
            finish_focus: cx.focus_handle(),
            _keys: keys,
        }
    }
    fn steps() -> Vec<Step> {
        use Step::*;
        vec![
            Check("fixture-list"),
            Click("note"),
            Check("note-loading"),
            Click("finish"),
            Check("note-ready"),
            Key("tab"),
            Check("tab-copy"),
            Key("tab"),
            Check("tab-text"),
            Key("tab"),
            Check("tab-close"),
            Key("shift-tab"),
            Check("reverse-tab-text"),
            Pointer {
                phase: "down",
                x: 42.,
                y: 137.,
            },
            Pointer {
                phase: "move",
                x: 195.,
                y: 137.,
            },
            Pointer {
                phase: "up",
                x: 195.,
                y: 137.,
            },
            Check("text-selected"),
            Key("a"),
            Check("readonly"),
            Click("copy"),
            Check("note-copied"),
            Click("switch-note"),
            Click("switch-note"),
            Click("finish-stale"),
            Check("same-file-stale"),
            Click("code"),
            Check("code-loading"),
            Click("finish-stale"),
            Check("stale-ignored"),
            Click("finish"),
            Check("code-ready"),
            Key("escape"),
            Check("closed-focus"),
            Click("long"),
            Click("finish"),
            Check("long-ready"),
            Wheel("text-area", 430.),
            Check("long-scrolled"),
            Click("image"),
            Click("finish"),
            Check("image-ready"),
            Click("failure"),
            Click("finish"),
            Check("local-error"),
            Key("tab"),
            Check("error-tab-close"),
            Key("shift-tab"),
            Check("error-tab-retry"),
            Click("retry"),
            Check("retry-loading"),
            Click("finish"),
            Check("retry-error"),
            Click("unsupported"),
            Check("unsupported"),
            Click("close"),
            Check("unsupported-closed"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let shown = |id| probe::visible(window, id);
        let focus = |h: &FocusHandle| h.is_focused(window);
        match name {
            "fixture-list" => ensure!(
                self.active.is_none()
                    && [
                        "note",
                        "fixture-code",
                        "long",
                        "fixture-image",
                        "fixture-failure",
                        "fixture-unsupported"
                    ]
                    .iter()
                    .all(|id| shown(id)),
                "fixtures missing"
            ),
            "note-loading" | "code-loading" | "retry-loading" => ensure!(
                self.phase == Phase::Loading
                    && shown("loading")
                    && self.pending.is_some()
                    && !shown("text-area"),
                "loading leaked content"
            ),
            "same-file-stale" => ensure!(
                self.active == Some(File::Note)
                    && self.phase == Phase::Loading
                    && self.pending == Some((self.revision, File::Note))
                    && self.source.is_none()
                    && self.text.read(cx).value().is_empty(),
                "older request for same file replaced newer request"
            ),
            "stale-ignored" => ensure!(
                self.active == Some(File::Code)
                    && self.phase == Phase::Loading
                    && self.pending.is_some()
                    && self.source.is_none()
                    && self.text.read(cx).value().is_empty(),
                "stale completion replaced current preview"
            ),
            "note-ready" | "note-copied" => ensure!(
                self.active == Some(File::Note)
                    && self.phase == Phase::Ready
                    && self.text.read(cx).value().as_ref() == PLAIN
                    && shown("text-area"),
                "note not selectable"
            ),
            "tab-copy" => ensure!(
                shown("viewer") && focus(&self.copy_focus),
                "Tab escaped viewer copy"
            ),
            "tab-text" | "reverse-tab-text" => ensure!(
                shown("viewer") && self.text.read(cx).focus_handle(cx).is_focused(window),
                "Tab did not reach readonly text"
            ),
            "tab-close" => ensure!(
                shown("viewer") && focus(&self.close_focus),
                "Tab did not wrap to close"
            ),
            "text-selected" => ensure!(
                self.text.read(cx).selected_text().len() > 0,
                "pointer selection missing"
            ),
            "readonly" => ensure!(
                self.text.read(cx).value().as_ref() == PLAIN,
                "readonly viewer accepted typing"
            ),
            "code-ready" => ensure!(
                self.active == Some(File::Code)
                    && self.phase == Phase::Ready
                    && self.text.read(cx).value().as_ref() == CODE
                    && shown("text-area"),
                "source not displayed"
            ),
            "closed-focus" => ensure!(
                self.active.is_none() && focus(&self.triggers[1]) && !shown("viewer"),
                "Escape did not restore originating focus"
            ),
            "long-ready" | "long-scrolled" => {
                ensure!(
                    self.active == Some(File::Long)
                        && self.phase == Phase::Ready
                        && self.source.as_ref().is_some_and(
                            |s| s.len() > 100_000 && self.text.read(cx).value().as_ref() == s
                        ),
                    "long source lost"
                );
                probe::inside(window, "text-area", Self::WIDTH as f32, Self::HEIGHT as f32)?;
                if name == "long-scrolled" {
                    ensure!(
                        self.text.read(cx).scroll_offset().y < px(-1.),
                        "wheel did not scroll native text"
                    );
                }
            }
            "image-ready" => {
                ensure!(
                    self.active == Some(File::Image)
                        && self.phase == Phase::Ready
                        && shown("image-canvas")
                        && shown("native-image"),
                    "image not painted"
                );
                ensure!(
                    IMAGE.len() > 1024
                        && &IMAGE[..8] == b"\x89PNG\r\n\x1a\n"
                        && u32::from_be_bytes(IMAGE[16..20].try_into()?) == 320
                        && u32::from_be_bytes(IMAGE[20..24].try_into()?) == 180,
                    "embedded image bytes do not match fixture dimensions"
                );
                let b = window.find("native-image").bounds();
                ensure!(
                    (f32::from(b.size.width) / f32::from(b.size.height) - 320. / 180.).abs() < 0.02,
                    "image aspect ratio changed: {b:?}"
                );
                probe::inside(
                    window,
                    "native-image",
                    Self::WIDTH as f32,
                    Self::HEIGHT as f32,
                )?;
            }
            "error-tab-close" => ensure!(
                self.phase == Phase::Error && shown("error") && focus(&self.close_focus),
                "Tab escaped error viewer"
            ),
            "error-tab-retry" => ensure!(
                self.phase == Phase::Error && shown("retry") && focus(&self.retry_focus),
                "Shift-Tab missed retry"
            ),
            "local-error" | "retry-error" => ensure!(
                self.active == Some(File::Failure)
                    && self.phase == Phase::Error
                    && shown("error")
                    && shown("retry")
                    && focus(&self.retry_focus)
                    && self.retries == if name == "retry-error" { 1 } else { 0 },
                "error/retry not local"
            ),
            "unsupported" => ensure!(
                self.active == Some(File::Unsupported)
                    && self.phase == Phase::Unsupported
                    && shown("unsupported-state")
                    && !shown("retry"),
                "unsupported is not distinct"
            ),
            "unsupported-closed" => ensure!(
                self.active.is_none() && focus(&self.triggers[5]),
                "close focus lost"
            ),
            _ if name != "note-copied" => anyhow::bail!("unknown check {name}"),
            _ => {}
        }
        if name == "note-copied" {
            ensure!(
                self.copied
                    && cx.read_from_clipboard().and_then(|i| i.text()).as_deref() == Some(PLAIN),
                "full source not copied to local clipboard"
            );
        }
        let bounds = if self.active.is_some() {
            Some(probe::inside(
                window,
                "viewer",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        let image_bounds = if name == "image-ready" {
            Some(probe::inside(
                window,
                "native-image",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        Ok(
            json!({"file":self.active.map(File::name), "phase":format!("{:?}",self.phase), "revision":self.revision,"pending":self.pending.is_some(),"retries":self.retries,"source_bytes":self.source.as_ref().map(String::len),"scroll_y":f32::from(self.text.read(cx).scroll_offset().y),"bounds":bounds,"image_bounds":image_bounds}),
        )
    }
}
impl Render for FilesProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let mut scene =
            probe::page(p)
                .child(div().text_size(px(18.)).child("File preview fixtures"))
                .child(
                    div()
                        .text_color(rgb(p.muted))
                        .child("Synthetic sources only · open a fixture to preview"),
                )
                .child(div().flex().flex_wrap().gap(px(8.)).children(
                    FILES.iter().enumerate().map(|(i, file)| {
                        let file = *file;
                        self.action(
                            match file {
                                File::Note => "note",
                                File::Code => "fixture-code",
                                File::Long => "long",
                                File::Image => "fixture-image",
                                File::Failure => "fixture-failure",
                                File::Unsupported => "fixture-unsupported",
                            },
                            file.name(),
                            &self.triggers[i],
                            move |s, w, cx| s.open(file, w, cx),
                            cx,
                        )
                    }),
                ));
        if let Some(file) = self.active {
            let mut viewer = div()
                .id("viewer")
                .test_support()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .bg(rgb(p.canvas))
                .border_1()
                .border_color(rgb(p.border))
                .overflow_hidden()
                .child(
                    div()
                        .h(px(56.))
                        .px(px(20.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .border_b_1()
                        .border_color(rgb(p.border))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .child(file.name()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(p.muted))
                                .child("fixtures / synthetic"),
                        )
                        .when(self.phase == Phase::Ready && self.source.is_some(), |d| {
                            d.child(self.action(
                                "copy",
                                "Copy Source",
                                &self.copy_focus,
                                |s, _, cx| {
                                    if let Some(source) = &s.source {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            source.clone(),
                                        ));
                                        s.copied = true;
                                        cx.notify();
                                    }
                                },
                                cx,
                            ))
                        })
                        .child(self.action(
                            "close",
                            "Close Preview",
                            &self.close_focus,
                            |s, w, cx| s.close(w, cx),
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .p(px(10.))
                        .border_b_1()
                        .border_color(rgb(p.border))
                        .children(FILES.iter().enumerate().map(|(i, f)| {
                            let f = *f;
                            self.action(
                                match f {
                                    File::Note => "switch-note",
                                    File::Code => "code",
                                    File::Long => "switch-long",
                                    File::Image => "image",
                                    File::Failure => "failure",
                                    File::Unsupported => "unsupported",
                                },
                                f.name(),
                                &self.triggers[i],
                                move |s, w, cx| s.open(f, w, cx),
                                cx,
                            )
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .relative()
                        .overflow_hidden()
                        .when(self.phase == Phase::Ready, |d| {
                            if file == File::Image {
                                d.child(
                                    div()
                                        .id("image-canvas")
                                        .test_support()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            img(self.image.clone())
                                                .object_fit(ObjectFit::Contain)
                                                .id("native-image")
                                                .test_support()
                                                .w(px(640.))
                                                .h(px(360.)),
                                        ),
                                )
                            } else {
                                d.child(
                                    div()
                                        .id("text-area")
                                        .test_support()
                                        .size_full()
                                        .overflow_hidden()
                                        .p(px(20.))
                                        .font_family("Menlo")
                                        .text_size(px(13.))
                                        .child(Textarea::new(&self.text)),
                                )
                            }
                        })
                        .when(self.phase == Phase::Loading, |d| {
                            d.child(
                                div()
                                    .id("loading")
                                    .test_support()
                                    .p(px(24.))
                                    .child("Loading preview…"),
                            )
                        })
                        .when(self.phase == Phase::Error, |d| {
                            d.child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .id("error")
                                            .test_support()
                                            .text_color(rgb(p.danger))
                                            .child("Couldn’t preview this file. Try again."),
                                    )
                                    .child(self.action(
                                        "retry",
                                        "Retry",
                                        &self.retry_focus,
                                        |s, w, cx| {
                                            s.retries += 1;
                                            s.open(File::Failure, w, cx);
                                        },
                                        cx,
                                    )),
                            )
                        })
                        .when(self.phase == Phase::Unsupported, |d| {
                            d.child(
                                div()
                                    .id("unsupported-state")
                                    .test_support()
                                    .p(px(24.))
                                    .child("Preview unavailable for this file type."),
                            )
                        }),
                )
                .when(self.phase == Phase::Loading, |d| {
                    d.child(
                        div()
                            .p(px(10.))
                            .flex()
                            .gap(px(8.))
                            .child(self.action(
                                "finish",
                                "Complete Pending",
                                &self.finish_focus,
                                |s, w, cx| {
                                    if let Some(ticket) = s.pending {
                                        s.complete(ticket, w, cx);
                                    }
                                },
                                cx,
                            ))
                            .when(self.stale.is_some(), |d| {
                                d.child(self.action(
                                    "finish-stale",
                                    "Complete Superseded",
                                    &self.finish_focus,
                                    |s, w, cx| {
                                        if let Some(ticket) = s.stale.take() {
                                            s.complete(ticket, w, cx);
                                        }
                                    },
                                    cx,
                                ))
                            }),
                    )
                });
            if self.copied {
                viewer = viewer.child(
                    div()
                        .absolute()
                        .bottom(px(12.))
                        .right(px(16.))
                        .child("Source copied"),
                );
            }
            scene = scene.child(viewer);
        }
        scene
    }
}
