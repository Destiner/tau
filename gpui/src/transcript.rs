use gpui_kit::{
    Context, Entity, FollowMode, IntoElement, ListAlignment, ListOffset, ListState, Render,
    TestSupportExt, Window,
    base::{Button, TextView, TextViewState, TextViewStyle},
    div, list,
    prelude::*,
    px, rgb,
};
use std::{rc::Rc, time::Duration};

const STREAM_LIMIT: usize = 60;
const STREAM_INTERVAL: Duration = Duration::from_millis(200);

struct Message {
    id: u64,
    text: String,
    expanded: bool,
    view: Entity<TextViewState>,
}

pub struct Transcript {
    messages: Vec<Message>,
    list: ListState,
    dark: bool,
    paused: bool,
    next_id: u64,
    large: bool,
    shrink: bool,
    ticks: usize,
    epoch: u64,
    timer: Option<gpui_kit::Task<()>>,
}

fn content(id: u64) -> String {
    match id % 5 {
        0 => format!(
            "### Investigation {id}\nA **rich** paragraph with a [reference](https://example.invalid/) and a second sentence to exercise wrapping at the window edge.\n\n- First finding\n- Second finding"
        ),
        1 => format!("Message {id}: short answer with `inline code` and selectable text."),
        2 => format!(
            "### Tool output {id}\n```rust\nfn main() {{ println!(\"hello\"); }}\n```\nA follow-up explanation after the code block."
        ),
        3 => format!(
            "Message {id}: A longer line that wraps across multiple lines in this narrow transcript viewport. Long text needs real measurement, not a fixed per-message row height. More text makes the wrapping unmistakable."
        ),
        _ => format!("Message {id}: Done."),
    }
}

impl Transcript {
    pub fn new(dark: bool, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            messages: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(160.)),
            dark,
            paused: true,
            next_id: 1,
            large: false,
            shrink: false,
            ticks: 0,
            epoch: 0,
            timer: None,
        };
        this.list.set_follow_mode(FollowMode::Tail);
        for _ in 0..24 {
            this.append(cx);
        }
        this
    }

    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn ticks(&self) -> usize {
        self.ticks
    }
    pub fn last_text_len(&self) -> usize {
        self.messages.last().map_or(0, |m| m.text.len())
    }
    pub fn len(&self) -> usize {
        self.messages.len()
    }
    pub fn following(&self) -> bool {
        self.list.is_following_tail()
    }
    pub fn anchor(&self) -> ListOffset {
        self.list.logical_scroll_top()
    }
    pub fn bounds(&self, ix: usize) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        self.list.bounds_for_item(ix)
    }
    pub fn id(&self, ix: usize) -> u64 {
        self.messages[ix].id
    }
    pub fn expanded(&self, ix: usize) -> bool {
        self.messages[ix].expanded
    }
    pub fn view(&self, ix: usize) -> Entity<TextViewState> {
        self.messages[ix].view.clone()
    }

    pub fn append(&mut self, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let text = content(id);
        let view = cx.new(|cx| TextViewState::markdown(&text, cx));
        let count = self.messages.len();
        self.messages.push(Message {
            id,
            text,
            expanded: true,
            view,
        });
        self.list.splice(count..count, 1);
        // Tail must be established before the next layout measures the new row.
        if self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    pub fn grow(&mut self, ix: usize, cx: &mut Context<Self>) {
        let message = &mut self.messages[ix];
        message.text.push_str("\n\nStreaming **more content** to the same stable message, including a line that wraps at narrower window widths. This changes actual measured height.");
        message
            .view
            .update(cx, |state, cx| state.set_text(&message.text, cx));
        self.list.remeasure_items(ix..ix + 1);
        if self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }

    pub fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.messages[ix].expanded = !self.messages[ix].expanded;
        self.list.remeasure_items(ix..ix + 1);
        cx.notify();
    }

    pub fn inspect_above(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: 5,
            offset_in_item: px(0.),
        });
        cx.notify();
    }
    pub fn older(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: 6,
            offset_in_item: px(12.),
        });
        cx.notify();
    }
    pub fn latest(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        cx.notify();
    }
    pub fn step(&mut self, cx: &mut Context<Self>) {
        self.append(cx);
        self.grow(self.messages.len() - 1, cx);
    }
    fn stop(&mut self) {
        self.epoch += 1;
        self.timer = None;
        self.paused = true;
    }
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let epoch = self.epoch;
        self.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STREAM_INTERVAL).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this.paused || this.epoch != epoch {
                        return;
                    }
                    this.stream_tick(cx);
                    if this.ticks < STREAM_LIMIT {
                        this.schedule(cx);
                    } else {
                        this.stop();
                        cx.notify();
                    }
                });
            }
        }));
    }
    fn stream_tick(&mut self, cx: &mut Context<Self>) {
        if self.ticks.is_multiple_of(5) {
            self.append(cx);
        }
        self.grow(self.messages.len() - 1, cx);
        self.ticks += 1;
    }
    pub fn pause(&mut self, cx: &mut Context<Self>) {
        if self.paused {
            if self.ticks >= STREAM_LIMIT {
                self.ticks = 0;
            }
            self.paused = false;
            self.schedule(cx);
        } else {
            self.stop();
        }
        cx.notify();
    }
    pub fn shrink(&mut self, cx: &mut Context<Self>) {
        self.shrink = !self.shrink;
        cx.notify();
    }
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.stop();
        self.ticks = 0;
        self.shrink = false;
        self.messages.clear();
        self.list.reset(0);
        self.next_id = 1;
        self.large = false;
        self.list.set_follow_mode(FollowMode::Tail);
        for _ in 0..24 {
            self.append(cx);
        }
    }
    pub fn large(&mut self, cx: &mut Context<Self>) {
        self.reset(cx);
        self.large = true;
        for _ in 0..976 {
            self.append(cx);
        }
    }
    pub fn change_above(&mut self, cx: &mut Context<Self>) {
        self.grow(5, cx);
    }
}

impl Render for Transcript {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let text = if dark { 0xcbd1da } else { 0x333b43 };
        let muted = if dark { 0x838c9c } else { 0x65717d };
        let background = if dark { 0x10141c } else { 0xfcfcfc };
        let card = if dark { 0x1b222d } else { 0xf3f5f7 };
        let style = TextViewStyle::default()
            .with_foreground(rgb(text).into())
            .with_muted_foreground(rgb(muted).into());
        let rows: Rc<Vec<_>> = Rc::new(
            self.messages
                .iter()
                .map(|m| (m.id, m.view.clone(), m.expanded))
                .collect(),
        );
        let state = self.list.clone();
        let entity = cx.entity().clone();
        let row_entity = entity.clone();
        let control =
            |id: &'static str,
             label: &'static str,
             action: fn(&mut Transcript, &mut Context<Transcript>)| {
                let entity = entity.clone();
                Button::new(id)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(card))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        entity.update(cx, action);
                    })
            };
        div()
            .size_full()
            .bg(rgb(background))
            .text_color(rgb(text))
            .font_family("Inter")
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(card))
                    .child(control("step", "Step (append + grow)", Transcript::step))
                    .child(control(
                        "stream",
                        if self.paused { "Start" } else { "Pause" },
                        Transcript::pause,
                    ))
                    .child(control("older", "Read Older", Transcript::older))
                    .child(control("latest", "Jump to Latest", Transcript::latest))
                    .child(control(
                        "above",
                        "Grow Above Reader",
                        Transcript::change_above,
                    ))
                    .child(control("large", "Large (1,000)", Transcript::large))
                    .child(control("shrink", "Toggle footer", Transcript::shrink))
                    .child(control("reset", "Reset", Transcript::reset)),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(muted))
                    .child(format!(
                        "Native GPUI / {} messages / {} / {}",
                        self.len(),
                        if self.following() {
                            "following"
                        } else {
                            "reading"
                        },
                        if self.large {
                            "large fixture"
                        } else {
                            "small fixture"
                        }
                    )),
            )
            .child(
                div()
                    .id("transcript-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        list(state, move |ix, _, _| {
                            let (id, view, expanded) = &rows[ix];
                            let entity = row_entity.clone();
                            let id = *id;
                            div()
                                .w_full()
                                .px_4()
                                .py_2()
                                .border_b_1()
                                .border_color(rgb(card))
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .text_sm()
                                        .text_color(rgb(muted))
                                        .child(format!("#{id}"))
                                        .child(
                                            Button::new(format!("collapse-{id}"))
                                                .child(if *expanded {
                                                    "Collapse"
                                                } else {
                                                    "Expand"
                                                })
                                                .on_click(move |_, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        if let Some(ix) = this
                                                            .messages
                                                            .iter()
                                                            .position(|m| m.id == id)
                                                        {
                                                            this.toggle(ix, cx);
                                                        }
                                                    });
                                                }),
                                        ),
                                )
                                .when(*expanded, |row| {
                                    row.child(
                                        TextView::new(view)
                                            .style(style.clone())
                                            .on_link_click(|_, _, _, _| {}),
                                    )
                                })
                                .into_any_element()
                        })
                        .flex_1()
                        .min_h_0(),
                    ),
            )
            .when(self.shrink, |root| {
                root.child(
                    div()
                        .h(px(110.))
                        .flex_shrink_0()
                        .bg(rgb(card))
                        .child("Synthetic footer: reduced transcript viewport"),
                )
            })
    }
}
