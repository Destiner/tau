use super::{
    highlight,
    probe::{self, Palette, Probe, Step},
};
use anyhow::{Result, ensure};
use gpui_kit::{
    Anchor, App, Context, FocusHandle, IntoElement, KeyDownEvent, Render, Subscription, Task,
    TestSupportExt, Window,
    base::{Popup, TextView, TextViewState, TextViewStyle},
    div,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};
use std::time::Duration;

const LONG: &str = "# Long queued note\n\n**Important**: keep the draft. [Safe destination](https://example.invalid/)\n\n- First point\n- Second point\n\n```rust\nfn queue() { println!(\"local only\"); }\n```\n";

struct Entry {
    id: u64,
    kind: Kind,
    text: String,
    focus: FocusHandle,
    view: gpui_kit::Entity<TextViewState>,
    _focus_subscription: Subscription,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Steering,
    FollowUp,
}

pub struct QueueProbe {
    p: Palette,
    entries: Vec<Entry>,
    next_id: u64,
    preview: Option<u64>,
    pointer_pill: Option<u64>,
    pointer_preview: bool,
    suppressed: Option<u64>,
    epoch: u64,
    timer: Option<Task<()>>,
    preview_focus: FocusHandle,
    _keyboard: Subscription,
    link: Option<String>,
}
impl QueueProbe {
    fn add(&mut self, kind: Kind, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let focus = cx.focus_handle();
        let subscription = cx.on_focus(&focus, window, move |this, _, cx| {
            if this.suppressed == Some(id) {
                return;
            }
            this.show(id, cx);
        });
        let entry = Entry {
            id,
            kind,
            text: text.into(),
            focus,
            view: cx.new(|cx| TextViewState::markdown(text, cx)),
            _focus_subscription: subscription,
        };
        self.entries.push(entry);
        cx.notify();
    }
    fn invalidate(&mut self) {
        self.epoch += 1;
        self.timer = None;
    }
    fn show(&mut self, id: u64, cx: &mut Context<Self>) {
        self.invalidate();
        if self.entries.iter().any(|e| e.id == id) {
            self.preview = Some(id);
            cx.notify();
        }
    }
    fn close(&mut self, cx: &mut Context<Self>) {
        self.invalidate();
        self.preview = None;
        self.pointer_preview = false;
        cx.notify();
    }
    fn delay(&mut self, ms: u64, id: Option<u64>, cx: &mut Context<Self>) {
        self.invalidate();
        let epoch = self.epoch;
        self.timer = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(ms))
                .await;
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |this, cx| {
                    if this.epoch != epoch {
                        return;
                    }
                    match id {
                        Some(id) if this.pointer_pill == Some(id) => this.show(id, cx),
                        None if this.pointer_pill.is_none() && !this.pointer_preview => {
                            this.close(cx)
                        }
                        _ => {}
                    }
                });
            }
        }));
    }
    fn hover_pill(&mut self, id: u64, hover: bool, cx: &mut Context<Self>) {
        if hover {
            if self.suppressed == Some(id) && self.pointer_pill != Some(id) {
                self.suppressed = None;
            }
            self.pointer_pill = Some(id);
            if self.suppressed != Some(id) {
                self.delay(180, Some(id), cx);
            }
        } else {
            if self.pointer_pill == Some(id) {
                self.pointer_pill = None;
            }
            self.delay(90, None, cx);
        }
    }
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.preview {
            self.close(cx);
            self.suppressed = Some(id);
            if let Some(entry) = self.entries.iter().find(|e| e.id == id) {
                entry.focus.focus(window, cx);
            }
        }
    }
    fn remove(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.entries.iter().position(|e| e.kind == Kind::FollowUp) {
            let id = self.entries.remove(index).id;
            self.invalidate();
            if self.preview == Some(id) {
                self.preview = None;
            }
            if self.pointer_pill == Some(id) {
                self.pointer_pill = None;
            }
            self.suppressed = None;
            self.pointer_preview = false;
            cx.notify();
        }
    }
    fn reorder(&mut self, cx: &mut Context<Self>) {
        let indices: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == Kind::FollowUp)
            .map(|(i, _)| i)
            .collect();
        if let (Some(&a), Some(&b)) = (indices.first(), indices.last()) {
            self.entries.swap(a, b);
        }
        self.invalidate();
        self.preview = None;
        self.pointer_pill = None;
        self.pointer_preview = false;
        self.suppressed = None;
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.preview.is_some() {
            self.escape(window, cx);
            cx.stop_propagation();
        }
    }
}
impl Probe for QueueProbe {
    const ID: &'static str = "queue";
    const TITLE: &'static str = "Tau — Native Queue";
    const DESCRIPTION: &'static str =
        "Interactive bounded Markdown queue previews with hover delay and keyboard focus.";
    const LIMITATIONS: &'static str = "Synthetic retained queue IDs; no Pi, external navigation, clear/recover RPC, or persistent windows. Preview links are local-only.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let keyboard = cx.intercept_keystrokes(|event, window, cx| {
            if event.keystroke.key == "tab" {
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
                cx.stop_propagation();
            }
        });
        let mut this = Self {
            p: Palette::new(dark),
            entries: Vec::new(),
            next_id: 1,
            preview: None,
            pointer_pill: None,
            pointer_preview: false,
            suppressed: None,
            epoch: 0,
            timer: None,
            preview_focus: cx.focus_handle(),
            _keyboard: keyboard,
            link: None,
        };
        this.add(Kind::Steering, "Short steer", window, cx);
        this.add(Kind::Steering, "Steer with extra context", window, cx);
        this.add(
            Kind::FollowUp,
            &format!(
                "{LONG}{}",
                "\n\n## Additional context\nA longer note to read and select.\n".repeat(18)
            ),
            window,
            cx,
        );
        this.add(Kind::FollowUp, "Second follow-up", window, cx);
        this
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Check("initial"),
            Step::Hover("pill-1"),
            Step::Check("hover-pending"),
            Step::Wait(179),
            Step::Check("hover-before-deadline"),
            Step::Wait(1),
            Step::Check("hover-open"),
            Step::Hover("preview-surface"),
            Step::Wait(89),
            Step::Check("preview-hover"),
            Step::Wait(41),
            Step::Check("preview-retained"),
            Step::Hover("add"),
            Step::Wait(89),
            Step::Check("exit-pending"),
            Step::Wait(1),
            Step::Check("exit-closed"),
            Step::Key("tab"),
            Step::Key("tab"),
            Step::Key("tab"),
            Step::Key("tab"),
            Step::Check("tab-focus"),
            Step::Key("escape"),
            Step::Check("escape"),
            Step::Wait(250),
            Step::Check("no-reopen"),
            Step::Hover("add"),
            Step::Hover("pill-1"),
            Step::Wait(180),
            Step::Check("reentered"),
            Step::Key("escape"),
            Step::Check("suppressed-immediate"),
            Step::Wait(180),
            Step::Check("suppressed-on-pill"),
            Step::Hover("add"),
            Step::Hover("pill-1"),
            Step::Wait(180),
            Step::Check("reentered-again"),
            Step::Click("pill-3"),
            Step::Check("focus-preview"),
            Step::Pointer {
                phase: "down",
                x: 500.,
                y: 265.,
            },
            Step::Pointer {
                phase: "up",
                x: 500.,
                y: 265.,
            },
            Step::Check("safe-link"),
            Step::Pointer {
                phase: "down",
                x: 310.,
                y: 300.,
            },
            Step::Pointer {
                phase: "move",
                x: 385.,
                y: 300.,
            },
            Step::Pointer {
                phase: "up",
                x: 385.,
                y: 300.,
            },
            Step::Check("selection"),
            Step::Wheel("preview-area", 230.),
            Step::Check("scrolled"),
            Step::Hover("pill-3"),
            Step::Click("remove"),
            Step::Wait(180),
            Step::Check("removed"),
            Step::Click("add"),
            Step::Check("added"),
            Step::Hover("pill-5"),
            Step::Click("reorder"),
            Step::Wait(240),
            Step::Check("reordered"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let shown = probe::visible(window, "preview-surface");
        let ids: Vec<_> = self.entries.iter().map(|e| e.id).collect();
        let follow: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.kind == Kind::FollowUp)
            .map(|e| e.id)
            .collect();
        ensure!(
            self.entries
                .iter()
                .take_while(|e| e.kind == Kind::Steering)
                .count()
                == 2
                && self
                    .entries
                    .iter()
                    .skip(2)
                    .all(|e| e.kind == Kind::FollowUp),
            "steering must precede follow-ups"
        );
        match name {
            "initial" => ensure!(ids == [1, 2, 3, 4] && !shown, "initial queue"),
            "hover-pending" => ensure!(
                !shown && self.pointer_pill == Some(1),
                "hover opened before delay"
            ),
            "hover-before-deadline" => ensure!(
                !shown && self.pointer_pill == Some(1),
                "hover opened before 180 ms"
            ),
            "hover-open" | "preview-hover" | "preview-retained" => {
                ensure!(shown && self.preview == Some(1), "hover preview lost")
            }
            "exit-pending" => ensure!(shown && self.preview == Some(1), "closed before 90 ms"),
            "exit-closed" => ensure!(!shown, "did not close on exit at 90 ms"),
            "tab-focus" => ensure!(
                shown && self.entries[0].focus.is_focused(window),
                "Tab did not focus pill 1; focus: {:?}",
                self.entries
                    .iter()
                    .map(|e| e.focus.is_focused(window))
                    .collect::<Vec<_>>()
            ),
            "escape" | "no-reopen" => ensure!(
                !shown && self.entries[0].focus.is_focused(window),
                "Escape focus/reopen"
            ),
            "reentered" | "reentered-again" => ensure!(
                shown && self.preview == Some(1),
                "hover did not reopen: pointer {:?}, suppressed {:?}, preview {:?}",
                self.pointer_pill,
                self.suppressed,
                self.preview
            ),
            "suppressed-immediate" | "suppressed-on-pill" => ensure!(
                !shown && self.suppressed == Some(1),
                "Escape reopened while pointer stayed on pill: preview {:?}, pointer {:?}, suppressed {:?}, focus {}",
                self.preview,
                self.pointer_pill,
                self.suppressed,
                self.entries[0].focus.is_focused(window)
            ),
            "focus-preview" => ensure!(
                shown && self.preview == Some(3) && self.entries[2].focus.is_focused(window),
                "focus preview not immediate"
            ),
            "safe-link" => ensure!(
                self.link.as_deref() == Some("https://example.invalid/"),
                "safe link not activated locally"
            ),
            "selection" => ensure!(
                !self.entries[2].view.read(cx).selected_text().is_empty(),
                "Markdown selection missing"
            ),
            "scrolled" => ensure!(
                shown
                    && self.entries[2]
                        .view
                        .read(cx)
                        .list_state()
                        .logical_scroll_top()
                        .item_ix
                        > 0,
                "preview did not scroll"
            ),
            "removed" => ensure!(
                !shown && follow == [4] && self.entries.len() == 3,
                "removed item preview persisted"
            ),
            "added" => ensure!(follow == [4, 5] && !shown, "added item ordering"),
            "reordered" => ensure!(
                follow == [5, 4] && !shown,
                "stale timer opened reordered item"
            ),
            _ => anyhow::bail!("unknown check {name}"),
        }
        let bounds = if shown {
            Some(probe::inside(
                window,
                "preview-surface",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        if shown {
            ensure!(
                window.find("preview-surface").bounds().size.height <= px(342.),
                "unbounded preview"
            );
        }
        Ok(
            json!({"ids": ids, "follow_up_ids": follow, "preview": self.preview, "visible": shown, "bounds": bounds, "link": self.link}),
        )
    }
}
impl Render for QueueProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let scene = cx.entity().clone();
        let mut rail = div()
            .flex()
            .gap(px(5.))
            .items_center()
            .overflow_hidden()
            .w_full()
            .h(px(40.));
        for (i, entry) in self.entries.iter().enumerate() {
            if i > 0 && entry.kind == Kind::FollowUp && self.entries[i - 1].kind == Kind::Steering {
                rail = rail.child(div().w(px(1.)).h(px(16.)).bg(rgb(p.border)));
            }
            let id = entry.id;
            let label = if entry.kind == Kind::Steering {
                entry.text.clone()
            } else {
                let ordinal = self.entries[..=i]
                    .iter()
                    .filter(|e| e.kind == Kind::FollowUp)
                    .count();
                format!("{ordinal}  {}", entry.text)
            };
            let label = label
                .chars()
                .take(25)
                .collect::<String>()
                .replace('\n', " ");
            let scene_hover = scene.clone();
            let pill = probe::button(format!("pill-{id}"), label, p)
                .flex_shrink_0()
                .max_w(px(180.))
                .h(px(28.))
                .track_focus(&entry.focus)
                .on_hover(move |hover, _, cx| {
                    scene_hover.update(cx, |this, cx| this.hover_pill(id, *hover, cx));
                })
                .on_click({
                    let scene = scene.clone();
                    move |_, _, cx| {
                        scene.update(cx, |this, cx| this.show(id, cx));
                    }
                });
            if self.preview == Some(id) {
                let style = TextViewStyle::default()
                    .with_foreground(rgb(p.text).into())
                    .with_link(rgb(p.accent).into())
                    .with_code_background(rgb(p.code).into());
                let surface = div()
                    .id("preview-surface")
                    .test_support()
                    .w(px(480.))
                    .max_h(px(340.))
                    .p(px(10.))
                    .rounded(px(7.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.surface))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .track_focus(&self.preview_focus)
                    .on_hover({
                        let scene = scene.clone();
                        move |hover, _, cx| {
                            scene.update(cx, |this, cx| {
                                this.pointer_preview = *hover;
                                if *hover {
                                    this.invalidate();
                                } else {
                                    this.delay(90, None, cx);
                                }
                            });
                        }
                    })
                    .on_key_down(cx.listener(Self::key))
                    .child(div().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(
                        if entry.kind == Kind::Steering {
                            "Steering"
                        } else {
                            "Follow Up"
                        },
                    ))
                    .child(
                        div()
                            .id("preview-area")
                            .test_support()
                            .h(px(290.))
                            .overflow_hidden()
                            .child(
                                TextView::new(&entry.view)
                                    .size_full()
                                    .scrollable(true)
                                    .style(style)
                                    .shared_code_block_highlighter(
                                        highlight::code_block_highlighter(p.canvas == 0x10141c),
                                    )
                                    .on_link_click({
                                        let scene = scene.clone();
                                        move |url, _, _, cx| {
                                            let url = url.to_string();
                                            if url.starts_with("https://") {
                                                scene.update(cx, |this, cx| {
                                                    this.link = Some(url);
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    }),
                            ),
                    );
                rail = rail.child(
                    Popup::new(format!("popup-{id}"), pill)
                        .anchor(Anchor::TopLeft)
                        .offset(px(5.))
                        .content(surface),
                );
            } else {
                rail = rail.child(pill);
            }
        }
        probe::page(p)
            .on_key_down(cx.listener(Self::key))
            .child(div().text_size(px(18.)).child("Queued messages"))
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("Hover for preview · focus for immediate reveal · Escape closes"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(probe::button("add", "Add Follow Up", p).on_click({
                        let scene = scene.clone();
                        move |_, window, cx| {
                            scene.update(cx, |s, cx| {
                                s.add(Kind::FollowUp, "New **Markdown** follow-up", window, cx)
                            });
                        }
                    }))
                    .child(
                        probe::button("remove", "Remove First Follow Up", p).on_click({
                            let scene = scene.clone();
                            move |_, _, cx| {
                                scene.update(cx, |s, cx| s.remove(cx));
                            }
                        }),
                    )
                    .child(probe::button("reorder", "Reorder Follow Ups", p).on_click(
                        move |_, _, cx| {
                            scene.update(cx, |s, cx| s.reorder(cx));
                        },
                    )),
            )
            .child(
                div()
                    .w_full()
                    .border_1()
                    .border_color(rgb(p.border))
                    .p(px(8.))
                    .child(rail),
            )
    }
}
