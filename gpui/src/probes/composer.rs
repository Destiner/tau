use anyhow::{Result, ensure};
use gpui_kit::{
    App, Context, Entity, Focusable, FollowMode, IntoElement, ListAlignment, ListOffset, ListState,
    Render, Subscription, TestSupportExt, Window,
    base::{
        InputBase, Textarea,
        input::{InputEditorStyle, InputEvent, TextareaState},
    },
    div, list,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc};

use super::probe::{Palette, Probe, Step, button, inside, page, visible};

const LONG: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
const ROWS: usize = 30;

pub struct ComposerProbe {
    palette: Palette,
    editor: Entity<TextareaState>,
    list: ListState,
    messages: Vec<String>,
    model: bool,
    context: bool,
    compact: bool,
    busy: bool,
    completion: bool,
    completion_dismissed: bool,
    submissions: usize,
    stops: usize,
    reader_top: Cell<Option<f32>>,
    _events: Subscription,
}

gpui_kit::actions!(composer_probe, [DismissCompletion]);

impl ComposerProbe {
    fn dismiss(&mut self, _: &DismissCompletion, _: &mut Window, cx: &mut Context<Self>) {
        if self.completion {
            self.completion = false;
            self.completion_dismissed = true;
            cx.notify();
        }
    }
    fn append(&mut self, text: String, cx: &mut Context<Self>) {
        let ix = self.messages.len();
        self.messages.push(text);
        self.list.splice(ix..ix, 1);
        if self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.editor.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.append(format!("You · {text}"), cx);
        self.editor.update(cx, |s, cx| s.set_value("", window, cx));
        self.completion = false;
        self.completion_dismissed = false;
        self.busy = true;
        self.submissions += 1;
        cx.notify();
    }
    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            self.busy = false;
            self.stops += 1;
            self.append("Assistant · Stopped (synthetic)".into(), cx);
            self.editor.update(cx, |s, cx| s.focus(window, cx));
        }
    }
    fn latest(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        cx.notify();
    }
    fn older(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: 4,
            offset_in_item: px(0.),
        });
        cx.notify();
    }
    fn repin(&mut self, cx: &mut Context<Self>) {
        if self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
}

impl Probe for ComposerProbe {
    const ID: &'static str = "composer";
    const TITLE: &'static str = "Composer · Native GPUI";
    const DESCRIPTION: &'static str =
        "Base textarea and measured transcript with isolated synthetic workflow";
    const LIMITATIONS: &'static str = "Synthetic messages, commands, model and context; no Pi, persistence, remote completion or runtime. Resize fixture uses footer height, not OS window resizing.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "escape",
            DismissCompletion,
            Some("ComposerProbe"),
        )]);
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Message Pi…")
                .auto_grow(2, 8)
                .submit_on_enter(true)
        });
        editor.update(cx, |s, cx| {
            s.set_scroll_sensitivity(0.5);
            s.set_editor_paddings(gpui_kit::Edges {
                top: px(6.),
                right: px(8.),
                bottom: px(6.),
                left: px(8.),
            });
            s.focus(window, cx);
        });
        let events = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { shift: false, .. } => {
                        if this.completion {
                            this.editor
                                .update(cx, |s, cx| s.set_value("/help ", window, cx));
                            this.completion = false;
                            this.completion_dismissed = false;
                        } else {
                            this.submit(window, cx);
                        }
                    }
                    InputEvent::Change => {
                        let slash = this.editor.read(cx).value().starts_with('/');
                        if !slash {
                            this.completion_dismissed = false;
                        }
                        this.completion = slash && !this.completion_dismissed;
                        this.repin(cx);
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
        let list = ListState::new(ROWS, ListAlignment::Top, px(70.));
        list.set_follow_mode(FollowMode::Tail);
        list.scroll_to_end();
        Self {
            palette: Palette::new(dark), editor, list,
            messages: (0..ROWS).map(|i| format!("{} · {}", if i % 2 == 0 { "You" } else { "Assistant" },
                if i % 3 == 0 { "A measured message which wraps across multiple lines as the available transcript width changes. Its height must come from layout rather than a hardcoded row." } else { "Synthetic conversation context." })).collect(),
            model: false, context: false, compact: false, busy: false, completion: false, completion_dismissed: false, submissions: 0, stops: 0, reader_top: Cell::new(None), _events: events,
        }
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Check("initial"),
            Step::Click("editor-shell"),
            Step::Type("hello"),
            Step::Check("draft"),
            Step::Key("shift-enter"),
            Step::Type("world"),
            Step::Check("newline"),
            Step::Click("model"),
            Step::Click("context"),
            Step::Check("toggles"),
            Step::Click("editor-shell"),
            Step::Key("enter"),
            Step::Check("submitted"),
            Step::Click("editor-shell"),
            Step::Type("pending"),
            Step::Key("enter"),
            Step::Check("busy-guard"),
            Step::Click("stop"),
            Step::Check("stopped"),
            Step::Click("clear"),
            Step::Click("editor-shell"),
            Step::Type("/"),
            Step::Check("completion"),
            Step::Key("escape"),
            Step::Check("dismissed"),
            Step::Type("h"),
            Step::Check("dismissed-typing"),
            Step::Click("clear"),
            Step::Click("editor-shell"),
            Step::Type("/he"),
            Step::Key("enter"),
            Step::Check("accepted"),
            Step::Click("clear"),
            Step::Type("/"),
            Step::Click("completion-item"),
            Step::Check("pointer-accepted"),
            Step::Click("clear"),
            Step::Check("cleared"),
            Step::Click("editor-shell"),
            Step::Type("a"),
            Step::Click("long"),
            Step::Check("long-wrap"),
            Step::Click("tall"),
            Step::Check("max-height"),
            Step::Click("older"),
            Step::Check("reader"),
            Step::Click("resize"),
            Step::Check("reader-resize"),
            Step::Click("latest"),
            Step::Check("latest"),
            Step::Click("resize"),
            Step::Check("follow-resize"),
            Step::Click("clear"),
            Step::Check("shrink"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let editor = self.editor.read(cx);
        let draft = editor.value().to_string();
        let focused = editor.focus_handle(cx).is_focused(window);
        let shell = inside(
            window,
            "editor-shell",
            Self::WIDTH as f32,
            Self::HEIGHT as f32,
        )?;
        let viewport = inside(
            window,
            "transcript-list",
            Self::WIDTH as f32,
            Self::HEIGHT as f32,
        )?;
        let last = self.list.bounds_for_item(self.messages.len() - 1);
        let reader = self.list.bounds_for_item(4);
        let reader_y = reader.map(|b| f32::from(b.top()));
        let gap = last.map(|b| {
            (f32::from(window.find("transcript-list").bounds().bottom()) - f32::from(b.bottom()))
                .abs()
        });
        match name {
            "initial" => ensure!(
                draft.is_empty() && self.messages.len() == ROWS && self.list.is_following_tail(),
                "initial state"
            ),
            "draft" => ensure!(draft == "hello" && focused, "typing did not reach Base"),
            "newline" => ensure!(
                draft == "hello\nworld" && focused,
                "Shift+Enter did not insert newline"
            ),
            "toggles" => ensure!(
                self.model && self.context && draft == "hello\nworld" && focused,
                "toggles lost draft"
            ),
            "submitted" => ensure!(
                self.busy
                    && self.submissions == 1
                    && draft.is_empty()
                    && self
                        .messages
                        .last()
                        .is_some_and(|m| m.contains("hello\nworld")),
                "Enter did not submit"
            ),
            "busy-guard" => ensure!(
                self.busy
                    && self.submissions == 1
                    && draft == "pending"
                    && !visible(window, "send"),
                "busy Enter submitted again or lost pending draft"
            ),
            "stopped" => ensure!(
                !self.busy && self.stops == 1 && visible(window, "send") && draft == "pending",
                "stop did not restore send"
            ),
            "completion" => ensure!(
                self.completion && visible(window, "completion-item") && draft == "/",
                "slash completion absent"
            ),
            "dismissed" => ensure!(
                !self.completion && draft == "/" && focused,
                "Escape changed draft, focus or did not dismiss"
            ),
            "dismissed-typing" => ensure!(
                !self.completion && draft == "/h" && focused && !visible(window, "completion-item"),
                "completion reopened while continuing dismissed draft"
            ),
            "accepted" | "pointer-accepted" => ensure!(
                draft == "/help " && !self.completion && focused && self.submissions == 1,
                "Enter did not insert selected command into editor"
            ),
            "cleared" => ensure!(draft.is_empty() && !self.completion, "clear failed"),
            "long-wrap" => ensure!(
                draft.len() > 500
                    && f32::from(window.find("editor-shell").bounds().right())
                        <= Self::WIDTH as f32
                    && f32::from(window.find("editor-shell").bounds().size.height) > 60.
                    && f32::from(window.find("editor-shell").bounds().size.height) <= 160.,
                "unbroken text overflow or did not grow"
            ),
            "max-height" => ensure!(
                shell["height"]
                    .as_f64()
                    .is_some_and(|h| (105. ..=160.).contains(&h))
                    && draft.lines().count() > 8
                    && focused,
                "textarea did not cap shaped multiline height"
            ),
            "shrink" => ensure!(
                draft.is_empty()
                    && shell["height"]
                        .as_f64()
                        .is_some_and(|h| (40. ..=55.).contains(&h))
                    && self.list.is_following_tail()
                    && gap.is_some_and(|g| g <= 1.),
                "textarea did not shrink to minimum rows or transcript lost pin"
            ),
            "reader" => {
                ensure!(
                    !self.list.is_following_tail()
                        && self.list.logical_scroll_top().item_ix == 4
                        && reader_y.is_some(),
                    "reader item has no measured bounds"
                );
                self.reader_top.set(reader_y);
            }
            "reader-resize" => ensure!(
                !self.list.is_following_tail()
                    && self.list.logical_scroll_top().item_ix == 4
                    && reader_y
                        .zip(self.reader_top.get())
                        .is_some_and(|(now, before)| (now - before).abs() <= 1.),
                "reader's measured top moved: {reader_y:?} vs {:?}",
                self.reader_top.get()
            ),
            "latest" | "follow-resize" => ensure!(
                self.list.is_following_tail() && gap.is_some_and(|g| g <= 1.),
                "tail not pinned: {gap:?}"
            ),
            _ => anyhow::bail!("unknown check {name}"),
        }
        Ok(
            json!({"draft_bytes": draft.len(), "focused": focused, "rows": self.messages.len(), "submissions": self.submissions, "stops": self.stops, "busy": self.busy, "completion": self.completion, "following": self.list.is_following_tail(), "anchor": self.list.logical_scroll_top().item_ix, "gap": gap, "reader_y": reader_y, "editor": shell, "viewport": viewport}),
        )
    }
}

impl Render for ComposerProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let scene = cx.entity().clone();
        self.editor.update(cx, |s, _| {
            s.set_editor_style(InputEditorStyle {
                foreground: rgb(p.text).into(),
                muted_foreground: rgb(p.muted).into(),
                background: rgb(p.canvas).into(),
                border: rgb(p.border).into(),
                caret: rgb(p.accent).into(),
                caret_width: Some(px(2.)),
                center_caret: true,
                macos_textarea_selection: cfg!(target_os = "macos"),
                ..Default::default()
            })
        });
        let rows = Rc::new(self.messages.clone());
        let list_state = self.list.clone();
        page(p)
            .key_context("ComposerProbe")
            .on_action(cx.listener(Self::dismiss))
            .child(
                div()
                    .text_size(px(17.))
                    .child("Composer / synthetic session"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button("older", "Read older", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.older(cx))
                    }))
                    .child(button("latest", "Latest", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| scene.update(cx, |s, cx| s.latest(cx))
                    }))
                    .child(button("resize", "Resize viewport", p).on_click({
                        let scene = scene.clone();
                        move |_, _, cx| {
                            scene.update(cx, |s, cx| {
                                s.compact = !s.compact;
                                s.repin(cx);
                            })
                        }
                    })),
            )
            .child(
                div()
                    .id("transcript-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        list(list_state, move |ix, _, _| {
                            div()
                                .w_full()
                                .px(px(16.))
                                .py(px(12.))
                                .border_b_1()
                                .border_color(rgb(p.border))
                                .child(rows[ix].clone())
                                .into_any_element()
                        })
                        .flex_1()
                        .min_h_0(),
                    ),
            )
            .when(self.compact, |root| {
                root.child(
                    div()
                        .h(px(100.))
                        .flex_shrink_0()
                        .bg(rgb(p.surface))
                        .child("Synthetic viewport resize"),
                )
            })
            .child(
                div()
                    .id("composer-footer")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .when(self.completion, |d| {
                        d.child(
                            button("completion-item", "/help · synthetic command", p).on_click({
                                let scene = scene.clone();
                                move |_, window, cx| {
                                    scene.update(cx, |s, cx| {
                                        s.editor
                                            .update(cx, |e, cx| e.set_value("/help ", window, cx));
                                        s.completion = false;
                                        s.completion_dismissed = false;
                                        s.editor.update(cx, |e, cx| e.focus(window, cx));
                                        cx.notify();
                                    })
                                }
                            }),
                        )
                    })
                    .child(
                        InputBase::new("editor-shell")
                            .w_full()
                            .min_w_0()
                            .border_1()
                            .border_color(rgb(p.border))
                            .rounded(px(6.))
                            .bg(rgb(p.canvas))
                            .font_family(".AppleSystemUIFontMonospaced")
                            .text_size(px(11.))
                            .line_height(gpui_kit::relative(1.45))
                            .child(Textarea::new(&self.editor)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                button(
                                    "model",
                                    if self.model { "Model: B" } else { "Model: A" },
                                    p,
                                )
                                .on_click({
                                    let scene = scene.clone();
                                    move |_, window, cx| {
                                        scene.update(cx, |s, cx| {
                                            s.model = !s.model;
                                            s.editor.update(cx, |e, cx| e.focus(window, cx));
                                            cx.notify();
                                        })
                                    }
                                }),
                            )
                            .child(
                                button(
                                    "context",
                                    if self.context {
                                        "Context: on"
                                    } else {
                                        "Context: off"
                                    },
                                    p,
                                )
                                .on_click({
                                    let scene = scene.clone();
                                    move |_, window, cx| {
                                        scene.update(cx, |s, cx| {
                                            s.context = !s.context;
                                            s.editor.update(cx, |e, cx| e.focus(window, cx));
                                            cx.notify();
                                        })
                                    }
                                }),
                            )
                            .child(button("clear", "Clear", p).on_click({
                                let scene = scene.clone();
                                move |_, window, cx| {
                                    scene.update(cx, |s, cx| {
                                        s.editor.update(cx, |e, cx| {
                                            e.set_value("", window, cx);
                                            e.focus(window, cx);
                                        });
                                        s.completion = false;
                                        s.completion_dismissed = false;
                                        cx.notify();
                                    })
                                }
                            }))
                            .child(button("tall", "Tall draft", p).on_click({
                                let scene = scene.clone();
                                move |_, window, cx| {
                                    scene.update(cx, |s, cx| {
                                        s.editor.update(cx, |e, cx| {
                                            e.set_value("a\n".repeat(20), window, cx);
                                            e.focus(window, cx);
                                        });
                                    })
                                }
                            }))
                            .child(button("long", "Long draft", p).on_click({
                                let scene = scene.clone();
                                move |_, window, cx| {
                                    scene.update(cx, |s, cx| {
                                        s.editor.update(cx, |e, cx| {
                                            e.set_value(LONG.repeat(20), window, cx);
                                            e.focus(window, cx);
                                        })
                                    })
                                }
                            }))
                            .child(if self.busy {
                                button("stop", "Stop", p)
                                    .on_click(move |_, window, cx| {
                                        scene.update(cx, |s, cx| s.stop(window, cx))
                                    })
                                    .into_any_element()
                            } else {
                                button("send", "Send", p)
                                    .on_click(move |_, window, cx| {
                                        scene.update(cx, |s, cx| s.submit(window, cx))
                                    })
                                    .into_any_element()
                            }),
                    ),
            )
    }
}
