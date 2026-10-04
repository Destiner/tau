use anyhow::{Result, bail, ensure};
use gpui_kit::{
    App, Context, Entity, FollowMode, IntoElement, ListAlignment, ListOffset, ListState, Render,
    TestSupportExt, Window,
    base::{Button, TextView, TextViewState, TextViewStyle},
    div, list,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use super::probe::{Palette, Probe, Step, button, inside, page, visible};

const LONG_OUTPUT: &str = "line 01: collecting diagnostics\nline 02: alpha beta gamma delta\nline 03: selectable output remains here\nline 04: work continues\nline 05: completed stage\nline 06: detailed result\nline 07: more output\nline 08: final summary\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Success,
    Error,
}
impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Success => "Success",
            Self::Error => "Failed",
        }
    }
}

struct Activity {
    id: u64,
    name: &'static str,
    argument: String,
    arguments: String,
    result: String,
    status: Status,
    expanded: bool,
    view: Entity<TextViewState>,
}

pub struct ActivityProbe {
    palette: Palette,
    rows: Vec<Activity>,
    list: ListState,
    next_id: u64,
    collapsed_height: Cell<Option<f32>>,
    reader_top: Cell<Option<f32>>,
    selected_before_exit: RefCell<String>,
}

impl ActivityProbe {
    fn add(
        &mut self,
        name: &'static str,
        argument: String,
        arguments: String,
        result: String,
        status: Status,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let source = Self::details(&arguments, &result, status);
        let view = cx.new(|cx| TextViewState::markdown(&source, cx));
        let ix = self.rows.len();
        self.rows.push(Activity {
            id,
            name,
            argument,
            arguments,
            result,
            status,
            expanded: false,
            view,
        });
        let following = self.list.is_following_tail();
        self.list.splice(ix..ix, 1);
        if following {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    fn details(args: &str, result: &str, status: Status) -> String {
        let mut text = format!("Arguments\n```text\n{args}\n```");
        if !result.is_empty() {
            text.push_str(&format!(
                "\n\n{}\n```text\n{result}\n```",
                if status == Status::Error {
                    "Error"
                } else {
                    "Result"
                }
            ));
        }
        text
    }
    fn reset(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.list.reset(0);
        self.list.set_follow_mode(FollowMode::Tail);
        self.next_id = 1;
        self.add(
            "bash",
            "rg ActivityRow src".into(),
            "{\"command\":\"rg ActivityRow src\"}".into(),
            String::new(),
            Status::Running,
            cx,
        );
        self.add(
            "read",
            "src/components/ActivityRow.vue".into(),
            "{\"path\":\"src/components/ActivityRow.vue\"}".into(),
            LONG_OUTPUT.into(),
            Status::Success,
            cx,
        );
        self.add(
            "bash",
            "check unavailable path".into(),
            "{\"command\":\"check unavailable path\"}".into(),
            "The requested path is unavailable. Check the path and try again.".into(),
            Status::Error,
            cx,
        );
        self.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    }
    fn toggle(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(ix) = self.rows.iter().position(|r| r.id == id) {
            self.rows[ix].expanded = !self.rows[ix].expanded;
            self.list.remeasure_items(ix..ix + 1);
            cx.notify();
        }
    }
    fn set_status(&mut self, status: Status, cx: &mut Context<Self>) {
        let row = &mut self.rows[0];
        row.status = status;
        row.result = match status {
            Status::Running => String::new(),
            Status::Success => {
                "3 matching files\nActivityRow.vue\nTranscriptView.vue\nUiCollapsible.vue".into()
            }
            Status::Error => "Search failed. Check the query and try again.".into(),
        };
        let source = Self::details(&row.arguments, &row.result, status);
        row.view.update(cx, |view, cx| view.set_text(&source, cx));
        self.list.remeasure_items(0..1);
        if self.list.is_following_tail() {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    fn append(&mut self, cx: &mut Context<Self>) {
        let row = &mut self.rows[1];
        row.result.push_str("\nline 09: appended while reading\nline 10: selection should remain attached to this row");
        let source = Self::details(&row.arguments, &row.result, row.status);
        row.view.update(cx, |view, cx| view.set_text(&source, cx));
        self.list.remeasure_items(1..2);
        self.add(
            "read",
            format!("synthetic item {}", self.next_id),
            "{\"fixture\":true}".into(),
            "Append created this result.".into(),
            Status::Success,
            cx,
        );
    }
    fn many(&mut self, cx: &mut Context<Self>) {
        for _ in 0..300 {
            self.add(
                "read",
                format!("synthetic item {}", self.next_id),
                "{\"fixture\":true}".into(),
                "Measured virtual row.".into(),
                Status::Success,
                cx,
            );
        }
    }
    fn jump(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: px(0.),
        });
        cx.notify();
    }
    fn latest(&mut self, cx: &mut Context<Self>) {
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        cx.notify();
    }
}

impl Probe for ActivityProbe {
    const ID: &'static str = "activity";
    const TITLE: &'static str = "Activity Rows · Native GPUI";
    const DESCRIPTION: &'static str = "Measured virtual activity rows with stable IDs, status transitions and selectable bounded details";
    const LIMITATIONS: &'static str = "Synthetic UI-only tool records; no Pi, process execution, filesystem, networking, persistent window, or production integration. TextView renders read-only details; row list and detail scrolling are separate.";
    fn new(dark: bool, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            palette: Palette::new(dark),
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(96.)),
            next_id: 1,
            collapsed_height: Cell::new(None),
            reader_top: Cell::new(None),
            selected_before_exit: RefCell::new(String::new()),
        };
        this.list.set_follow_mode(FollowMode::Tail);
        this.reset(cx);
        this
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Check("initial"),
            Step::Click("row-2"),
            Step::Check("expanded"),
            Step::Pointer {
                phase: "down",
                x: 55.,
                y: 355.,
            },
            Step::Pointer {
                phase: "move",
                x: 240.,
                y: 355.,
            },
            Step::Pointer {
                phase: "up",
                x: 240.,
                y: 355.,
            },
            Step::Check("selected"),
            Step::Wheel("detail-2", 180.),
            Step::Check("detail-scrolled"),
            Step::Click("row-2"),
            Step::Check("collapsed"),
            Step::Click("row-2"),
            Step::Check("reexpanded"),
            Step::Key("space"),
            Step::Check("keyboard-collapsed"),
            Step::Key("space"),
            Step::Check("keyboard-expanded"),
            Step::Click("complete"),
            Step::Check("completed"),
            Step::Click("fail"),
            Step::Check("failed-local"),
            Step::Click("running"),
            Step::Check("running-again"),
            Step::Click("many"),
            Step::Check("many-tail"),
            Step::Click("first"),
            Step::Check("returned"),
            Step::Click("append"),
            Step::Check("appended-reader"),
            Step::Click("row-1"),
            Step::Check("independent"),
            Step::Click("far"),
            Step::Check("far-away"),
            Step::Click("first"),
            Step::Check("reentry"),
            Step::Pointer {
                phase: "down",
                x: 55.,
                y: 430.,
            },
            Step::Pointer {
                phase: "move",
                x: 230.,
                y: 430.,
            },
            Step::Pointer {
                phase: "up",
                x: 230.,
                y: 430.,
            },
            Step::Check("reentry-selected"),
            Step::Wheel("activity-list", 17000.),
            Step::Check("wheel-away"),
            Step::Wheel("activity-list", -17000.),
            Step::Check("wheel-return"),
            Step::Click("latest"),
            Step::Check("latest"),
            Step::Click("reset"),
            Step::Check("reset"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let viewport = inside(
            window,
            "activity-list",
            Self::WIDTH as f32,
            Self::HEIGHT as f32,
        )?;
        let first = &self.rows[0];
        let second = &self.rows[1];
        let third = &self.rows[2];
        let rendered = second.view.read(cx).rendered_text().as_str().to_string();
        let anchor = self.list.logical_scroll_top().item_ix;
        let row_height = self
            .list
            .bounds_for_item(1)
            .map(|b| f32::from(b.size.height));
        let selected = second.view.read(cx).selected_text().to_string();
        let first_top = self.list.bounds_for_item(0).map(|b| f32::from(b.top()));
        let tail_gap = self.list.bounds_for_item(self.rows.len() - 1).map(|b| {
            f32::from(window.find("activity-list").bounds().bottom()) - f32::from(b.bottom())
        });
        if name == "initial" {
            self.collapsed_height.set(row_height);
        }
        if name == "returned" {
            self.reader_top.set(first_top);
        }
        if name == "reentry-selected" {
            *self.selected_before_exit.borrow_mut() = selected.clone();
        }
        match name {
            "initial" | "reset" => ensure!(
                self.rows.len() == 3
                    && !first.expanded
                    && !second.expanded
                    && !third.expanded
                    && first.status == Status::Running
                    && visible(window, "row-1")
                    && visible(window, "row-2")
                    && row_height.is_some_and(|h| h > 30. && h < 100.),
                "initial rows incorrect"
            ),
            "expanded" | "reexpanded" => ensure!(
                second.expanded
                    && visible(window, "detail-2")
                    && rendered.contains("selectable output")
                    && !first.expanded
                    && !third.expanded
                    && row_height.zip(self.collapsed_height.get()).is_some_and(|(now, before)| now >= before + 180.)
                    && inside(window, "detail-2", Self::WIDTH as f32, Self::HEIGHT as f32)?["height"].as_f64().is_some_and(|h| (189. ..=191.).contains(&h)),
                "second row detail did not expand measured row"
            ),
            "selected" => ensure!(
                !second.view.read(cx).selected_text().is_empty()
                    && second.view.read(cx).selected_source_range().is_some(),
                "pointer did not select detail text"
            ),
            "detail-scrolled" => ensure!(
                second.expanded
                    && second
                        .view
                        .read(cx)
                        .list_state()
                        .logical_scroll_top()
                        .item_ix
                        > 0,
                "bounded detail did not scroll"
            ),
            "keyboard-collapsed" => ensure!(
                !second.expanded && !visible(window, "detail-2"),
                "Space did not collapse focused header"
            ),
            "keyboard-expanded" => ensure!(
                second.expanded && visible(window, "detail-2"),
                "Space did not expand focused header"
            ),
            "collapsed" => ensure!(
                !second.expanded
                    && !visible(window, "detail-2")
                    && rendered.contains("selectable output")
                    && row_height.zip(self.collapsed_height.get()).is_some_and(|(now, before)| (now - before).abs() <= 2.),
                "collapse did not restore measured row height"
            ),
            "completed" => ensure!(
                first.status == Status::Success
                    && first
                        .view
                        .read(cx)
                        .rendered_text()
                        .as_str()
                        .contains("3 matching files")
                    && second.expanded,
                "completion missing"
            ),
            "failed-local" => ensure!(
                first.status == Status::Error
                    && first
                        .view
                        .read(cx)
                        .rendered_text()
                        .as_str()
                        .contains("Check the query")
                    && third.status == Status::Error
                    && second.status == Status::Success,
                "error leaked across rows"
            ),
            "running-again" => ensure!(
                first.status == Status::Running
                    && first.result.is_empty()
                    && third.status == Status::Error,
                "running transition failed"
            ),
            "appended-reader" => ensure!(
                self.rows.len() == 304
                    && second.expanded
                    && rendered.contains("line 10")
                    && anchor < 4
                    && !self.list.is_following_tail()
                    && first_top.zip(self.reader_top.get()).is_some_and(|(now, before)| (now - before).abs() <= 1.),
                "reader moved or content lost"
            ),
            "many-tail" => ensure!(
                self.rows.len() == 303 && self.list.is_following_tail() && anchor > 250
                    && tail_gap.is_some_and(|g| g.abs() <= 1.),
                "large fixture not virtualized at tail"
            ),
            "returned" | "reentry" => ensure!(
                anchor == 0
                    && second.expanded
                    && (name == "returned" || rendered.contains("line 10"))
                    && visible(window, "detail-2"),
                "stable row state lost after leaving viewport"
            ),
            "independent" => ensure!(
                first.expanded && second.expanded && !third.expanded && visible(window, "detail-1"),
                "different IDs shared disclosure state"
            ),
            "far-away" => ensure!(
                anchor >= 200 && !visible(window, "detail-2") && !visible(window, "row-1"),
                "offscreen rows still mounted"
            ),
            "reentry-selected" => ensure!(
                !second.view.read(cx).selected_text().is_empty(),
                "reentry detail not selectable"
            ),
            "wheel-away" => ensure!(
                anchor > 100 && !visible(window, "detail-2"),
                "wheel did not virtualize selected row"
            ),
            "wheel-return" => ensure!(
                anchor == 0
                    && second.expanded
                    && visible(window, "detail-2")
                    && !selected.is_empty()
                    && selected == *self.selected_before_exit.borrow(),
                "wheel reentry lost expansion or changed selection"
            ),
            "latest" => ensure!(
                self.list.is_following_tail() && anchor > 250
                    && tail_gap.is_some_and(|g| g.abs() <= 1.),
                "not following tail"
            ),
            _ => bail!("unknown check {name}"),
        }
        Ok(
            json!({"rows": self.rows.len(), "anchor": anchor, "following": self.list.is_following_tail(), "expanded_ids": self.rows.iter().filter(|r| r.expanded).map(|r| r.id).collect::<Vec<_>>(), "first_status": first.status.label(), "second_rendered_bytes": rendered.len(), "second_selected": selected, "second_row_height": row_height, "first_top": first_top, "tail_gap": tail_gap, "viewport": viewport}),
        )
    }
}

impl Render for ActivityProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let scene = cx.entity().clone();
        let rows = Rc::new(
            self.rows
                .iter()
                .map(|r| {
                    (
                        r.id,
                        r.name,
                        r.argument.clone(),
                        r.status,
                        r.expanded,
                        r.view.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        );
        let state = self.list.clone();
        let action =
            |id: &'static str, label: &'static str, f: fn(&mut Self, &mut Context<Self>)| {
                let scene = scene.clone();
                button(id, label, p).on_click(move |_, _, cx| scene.update(cx, f))
            };
        let row_scene = scene.clone();
        page(p)
            .child(
                div()
                    .text_size(px(18.))
                    .child("Activity / synthetic tool calls"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(7.))
                    .child(action("running", "Running", |s, cx| {
                        s.set_status(Status::Running, cx)
                    }))
                    .child(action("complete", "Complete", |s, cx| {
                        s.set_status(Status::Success, cx)
                    }))
                    .child(action("fail", "Fail", |s, cx| {
                        s.set_status(Status::Error, cx)
                    }))
                    .child(action("append", "Append", Self::append))
                    .child(action("many", "Load Many", Self::many))
                    .child(action("first", "First Rows", |s, cx| s.jump(0, cx)))
                    .child(action("far", "Far Rows", |s, cx| s.jump(240, cx)))
                    .child(action("latest", "Latest", Self::latest))
                    .child(action("reset", "Reset", Self::reset)),
            )
            .child(div().text_color(rgb(p.muted)).child(format!(
                "{} rows · {} · click a header or focus it and press Space/Return",
                self.rows.len(),
                if self.list.is_following_tail() {
                    "following"
                } else {
                    "reading"
                }
            )))
            .child(
                div()
                    .id("activity-list")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .border_1()
                    .border_color(rgb(p.border))
                    .child(
                        list(state, move |ix, _, _| {
                            let (id, name, argument, status, expanded, view) = &rows[ix];
                            let id = *id;
                            let scene = row_scene.clone();
                            let status_color = if *status == Status::Error {
                                p.danger
                            } else if *status == Status::Running {
                                p.accent
                            } else {
                                p.muted
                            };
                            div()
                                .w_full()
                                .px(px(12.))
                                .py(px(5.))
                                .border_b_1()
                                .border_color(rgb(p.border))
                                .child(
                                    Button::new(format!("row-{id}"))
                                        .w_full()
                                        .px(px(9.))
                                        .py(px(6.))
                                        .rounded(px(4.))
                                        .bg(rgb(p.surface))
                                        .text_color(rgb(p.text))
                                        .hover(move |s| s.bg(rgb(p.hover)))
                                        .focus_visible(move |s| {
                                            s.border_1().border_color(rgb(p.accent))
                                        })
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(if *expanded { "▾" } else { "▸" })
                                                .child(div().text_color(rgb(p.muted)).child(*name))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .child(argument.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(rgb(status_color))
                                                        .child(status.label()),
                                                ),
                                        )
                                        .on_click(move |_, _, cx| {
                                            scene.update(cx, |s, cx| s.toggle(id, cx))
                                        }),
                                )
                                .when(*expanded, |row| {
                                    row.child(
                                        div()
                                            .id(format!("detail-{id}"))
                                            .test_support()
                                            .h(px(190.))
                                            .max_h(px(190.))
                                            .overflow_hidden()
                                            .p(px(8.))
                                            .bg(rgb(p.code))
                                            .child(
                                                TextView::new(view)
                                                    .scrollable(true)
                                                    .size_full()
                                                    .style(
                                                        TextViewStyle::default()
                                                            .with_foreground(rgb(p.text).into())
                                                            .with_muted_foreground(
                                                                rgb(p.muted).into(),
                                                            )
                                                            .with_code_background(
                                                                rgb(p.surface).into(),
                                                            ),
                                                    ),
                                            ),
                                    )
                                })
                                .into_any_element()
                        })
                        .flex_1()
                        .min_h_0(),
                    ),
            )
    }
}
