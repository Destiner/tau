use super::probe::{self, Palette, Probe, Step};
use anyhow::{Result, bail, ensure};
use gpui_kit::{
    Anchor, App, Context, Entity, Focusable, IntoElement, KeyDownEvent, Render, ScrollHandle,
    Window,
    base::{
        Button, Input, Popup, TestSupportExt,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};

gpui_kit::actions!(
    models_probe,
    [NextModel, PreviousModel, FirstModel, LastModel]
);

const GROUPS: [&str; 3] = ["Atlas", "Beacon", "Cedar"];
const INITIAL: usize = 2;
const DISABLED: usize = 7;
const LAST: usize = 35;

fn label(ix: usize) -> String {
    format!("{} Model {:02}", GROUPS[ix / 12], ix % 12 + 1)
}
fn model_id(ix: usize) -> String {
    format!("model-{ix}")
}

pub struct ModelsProbe {
    p: Palette,
    composer: Entity<InputState>,
    search: Entity<InputState>,
    scroll: ScrollHandle,
    open: bool,
    selected: usize,
    cursor: Option<usize>,
    query: String,
    fail_next: bool,
    notice: bool,
    attempts: usize,
    _keys: gpui_kit::Subscription,
}
impl ModelsProbe {
    fn matches(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..=LAST)
            .filter(|&ix| {
                q.is_empty() || label(ix).to_lowercase().contains(&q) || model_id(ix).contains(&q)
            })
            .collect()
    }
    fn enabled(&self) -> Vec<usize> {
        self.matches()
            .into_iter()
            .filter(|&ix| ix != DISABLED)
            .collect()
    }
    fn row(&self, ix: usize) -> usize {
        let matches = self.matches();
        let mut row = 0;
        let mut previous = None;
        for item in matches {
            let group = item / 12;
            if previous != Some(group) {
                row += 1;
                previous = Some(group);
            }
            if item == ix {
                return row;
            }
            row += 1;
        }
        row
    }
    fn reveal(&self) {
        if let Some(ix) = self.cursor {
            self.scroll.scroll_to_item(self.row(ix));
        }
    }
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.notice = false;
        self.query.clear();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.cursor = Some(self.selected);
        self.reveal();
        self.search.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.query.clear();
        self.cursor = None;
        // This selector belongs to the composer. The trigger is only a pointer entry point.
        self.composer.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    fn choose(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || ix == DISABLED || !self.matches().contains(&ix) {
            return;
        }
        self.attempts += 1;
        if self.fail_next {
            self.fail_next = false;
            self.notice = true;
        } else {
            self.selected = ix;
            self.notice = false;
        }
        self.close(window, cx);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || !self.search.focus_handle(cx).is_focused(window) {
            return;
        }
        match event.keystroke.key.as_str() {
            "tab"
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform =>
            {
                self.move_cursor(
                    if event.keystroke.modifiers.shift {
                        "up"
                    } else {
                        "down"
                    },
                    cx,
                );
            }
            "escape" => self.close(window, cx),
            "enter" => {
                if let Some(ix) = self.cursor {
                    self.choose(ix, window, cx);
                }
            }
            "down" | "up" | "home" | "end" => self.move_cursor(event.keystroke.key.as_str(), cx),
            _ => return,
        }
        cx.stop_propagation();
    }
    fn move_cursor(&mut self, direction: &str, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let options = self.enabled();
        let position = self
            .cursor
            .and_then(|ix| options.iter().position(|&item| item == ix));
        self.cursor = match direction {
            "home" => options.first().copied(),
            "end" => options.last().copied(),
            "down" => options
                .get(position.map_or(0, |i| (i + 1).min(options.len().saturating_sub(1))))
                .copied(),
            _ => options
                .get(position.map_or(0, |i| i.saturating_sub(1)))
                .copied(),
        };
        self.reveal();
        cx.notify();
    }
    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let mut previous = None;
        let mut rows = Vec::new();
        for ix in self.matches() {
            let group = ix / 12;
            if previous != Some(group) {
                rows.push(
                    div()
                        .h(px(25.))
                        .flex_shrink_0()
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .text_color(rgb(p.muted))
                        .child(GROUPS[group])
                        .into_any_element(),
                );
                previous = Some(group);
            }
            let disabled = ix == DISABLED;
            let active = self.cursor == Some(ix);
            let current = self.selected == ix;
            rows.push(
                Button::new(model_id(ix))
                    .w_full()
                    .h(px(32.))
                    .flex_shrink_0()
                    .px(px(9.))
                    .rounded(px(4.))
                    .bg(rgb(if active { p.hover } else { p.surface }))
                    .text_color(rgb(if disabled { p.muted } else { p.text }))
                    .text_size(px(12.))
                    .disabled(disabled)
                    .focusable(false)
                    .tab_stop(false)
                    .on_click(cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if !disabled && this.cursor != Some(ix) {
                            this.cursor = Some(ix);
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .child(label(ix))
                            .child(if current { "✓" } else { "" }),
                    )
                    .into_any_element(),
            );
        }
        div()
            .id("model-list")
            .test_support()
            .w_full()
            .h(px(232.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .p(px(4.))
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .id("empty-results")
                        .test_support()
                        .h(px(48.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(p.muted))
                        .child("No matching models"),
                )
            })
            .children(rows)
    }
}
impl Render for ModelsProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let trigger = probe::button("model-trigger", format!("{}  ▾", label(self.selected)), p)
            .w(px(236.))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.open {
                    this.close(window, cx);
                } else {
                    this.open(window, cx);
                }
            }));
        let popup = Popup::new("model-popup", trigger)
            .anchor(Anchor::BottomLeft)
            .offset(px(5.))
            .when(self.open, |popup| {
                popup.content(
                    div()
                        .id("model-surface")
                        .test_support()
                        .w(px(300.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(rgb(p.border))
                        .bg(rgb(p.surface))
                        .shadow_lg()
                        .overflow_hidden()
                        .on_key_down(cx.listener(Self::key))
                        .on_action(
                            cx.listener(|this, _: &NextModel, _, cx| this.move_cursor("down", cx)),
                        )
                        .on_action(
                            cx.listener(|this, _: &PreviousModel, _, cx| {
                                this.move_cursor("up", cx)
                            }),
                        )
                        .on_action(
                            cx.listener(|this, _: &FirstModel, _, cx| this.move_cursor("home", cx)),
                        )
                        .on_action(
                            cx.listener(|this, _: &LastModel, _, cx| this.move_cursor("end", cx)),
                        )
                        .on_mouse_down_out(
                            cx.listener(|this, _, window, cx| this.close(window, cx)),
                        )
                        .child(
                            div()
                                .key_context("ModelsSearch")
                                .id("search-box")
                                .test_support()
                                .h(px(38.))
                                .px(px(10.))
                                .flex()
                                .items_center()
                                .bg(rgb(p.canvas))
                                .child(Input::new(&self.search)),
                        )
                        .child(self.list(cx)),
                )
            });
        probe::page(p)
            .child(
                div()
                    .id("page-title")
                    .test_support()
                    .text_size(px(18.))
                    .child("Models"),
            )
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("Local model picker · grouped synthetic catalogue"),
            )
            .child(
                div()
                    .mt(px(320.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(div().text_color(rgb(p.muted)).child("Composer"))
                    .child(
                        div()
                            .id("composer-box")
                            .test_support()
                            .w(px(470.))
                            .h(px(40.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(p.border))
                            .bg(rgb(p.surface))
                            .child(Input::new(&self.composer)),
                    )
                    .child(
                        div().flex().items_center().gap(px(10.)).child(popup).child(
                            probe::button(
                                "fail-next",
                                if self.fail_next {
                                    "Fail Next: On"
                                } else {
                                    "Fail Next: Off"
                                },
                                p,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fail_next = !this.fail_next;
                                cx.notify();
                            })),
                        ),
                    )
                    .when(self.notice, |d| {
                        d.child(
                            div()
                                .id("model-notice")
                                .test_support()
                                .text_color(rgb(p.danger))
                                .child("Model change failed. Choose another model or try again."),
                        )
                    }),
            )
    }
}
impl Probe for ModelsProbe {
    const ID: &'static str = "models";
    const TITLE: &'static str = "Tau — Models";
    const DESCRIPTION: &'static str =
        "Grouped searchable native model picker with composer focus and failure rollback.";
    const LIMITATIONS: &'static str = "Finite local synthetic catalogue and explicit failure fixture; no Pi, SDK, persistence or production wiring.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = Palette::new(dark);
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextModel, Some("ModelsSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousModel, Some("ModelsSearch > Input")),
            gpui_kit::KeyBinding::new("home", FirstModel, Some("ModelsSearch > Input")),
            gpui_kit::KeyBinding::new("end", LastModel, Some("ModelsSearch > Input")),
        ]);
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Message"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search models"));
        for input in [&composer, &search] {
            input.update(cx, |s, _| s.set_editor_style(probe::text_style(p)));
        }
        let subscription = cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                let options = this.enabled();
                if !this.cursor.is_some_and(|ix| options.contains(&ix)) {
                    this.cursor = options.first().copied();
                }
                this.scroll = ScrollHandle::new();
                this.reveal();
                cx.notify();
            }
        });
        subscription.detach();
        let weak = cx.weak_entity();
        let search_focus = search.focus_handle(cx).clone();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key == "tab"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
                && search_focus.is_focused(window)
                && let Some(scene) = weak.upgrade()
                && scene.read(cx).open
            {
                scene.update(cx, |s, cx| {
                    s.move_cursor(
                        if event.keystroke.modifiers.shift {
                            "up"
                        } else {
                            "down"
                        },
                        cx,
                    )
                });
                cx.stop_propagation();
            }
        });
        Self {
            p,
            composer,
            search,
            scroll: ScrollHandle::new(),
            open: false,
            selected: INITIAL,
            cursor: None,
            query: String::new(),
            fail_next: false,
            notice: false,
            attempts: 0,
            _keys: keys,
        }
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Click("composer-box"),
            Step::Type("Keep this draft"),
            Step::Check("initial"),
            Step::Click("model-trigger"),
            Step::Check("selected-visible"),
            Step::Wheel("model-list", 170.),
            Step::Click("model-7"),
            Step::Check("disabled"),
            Step::Key("end"),
            Step::Check("end-scroll"),
            Step::Key("home"),
            Step::Key("down"),
            Step::Key("up"),
            Step::Check("navigation"),
            Step::Key("tab"),
            Step::Check("tab-contained"),
            Step::Key("shift-tab"),
            Step::Check("tab-reversed"),
            Step::Wheel("model-list", 160.),
            Step::Check("wheel"),
            Step::Key("escape"),
            Step::Check("cancel"),
            Step::Click("model-trigger"),
            Step::Type("zzzz-no-match"),
            Step::Check("empty"),
            Step::Key("enter"),
            Step::Check("empty-enter"),
            Step::Key("escape"),
            Step::Click("model-trigger"),
            Step::Type("atlas model 08"),
            Step::Check("disabled-filter"),
            Step::Key("enter"),
            Step::Check("disabled-filter-enter"),
            Step::Key("escape"),
            Step::Click("model-trigger"),
            Step::Type("cedar model 12"),
            Step::Check("filtered"),
            Step::Key("enter"),
            Step::Check("chosen"),
            Step::Click("fail-next"),
            Step::Click("model-trigger"),
            Step::Check("reopened-selected"),
            Step::Key("home"),
            Step::Key("enter"),
            Step::Check("failure"),
            Step::Click("model-trigger"),
            Step::Click("page-title"),
            Step::Check("outside"),
            Step::Click("model-trigger"),
            Step::Key("home"),
            Step::Click("model-0"),
            Step::Check("retry"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let surface = probe::visible(window, "model-surface");
        let draft = self.composer.read(cx).value().to_string();
        let search_focus = self.search.focus_handle(cx).is_focused(window);
        let composer_focus = self.composer.focus_handle(cx).is_focused(window);
        ensure!(
            draft == "Keep this draft",
            "composer draft changed: {draft:?}"
        );
        match name {
            "initial" => ensure!(
                !surface && self.selected == INITIAL && composer_focus,
                "initial focus/value"
            ),
            "selected-visible" => {
                ensure!(
                    surface && search_focus && self.cursor == Some(INITIAL),
                    "open search/cursor"
                );
                self.assert_row(window, INITIAL)?;
            }
            "disabled" => ensure!(
                surface
                    && self.selected == INITIAL
                    && self.cursor == Some(INITIAL)
                    && self.attempts == 0,
                "disabled activated"
            ),
            "reopened-selected" => {
                ensure!(
                    surface && search_focus && self.cursor == Some(LAST),
                    "selected cursor on reopen"
                );
                self.assert_row(window, LAST)?;
            }
            "end-scroll" => {
                ensure!(surface && self.cursor == Some(LAST), "end key");
                self.assert_row(window, LAST)?;
            }
            "navigation" | "tab-reversed" => {
                ensure!(
                    surface && search_focus && self.cursor == Some(0),
                    "home/down/up or Shift-Tab"
                );
                self.assert_row(window, 0)?;
            }
            "tab-contained" => {
                ensure!(
                    surface && search_focus && self.cursor == Some(1),
                    "Tab escaped search"
                );
                self.assert_row(window, 1)?;
            }
            "wheel" => ensure!(
                surface && self.scroll.offset().y < px(0.),
                "wheel did not move list: {:?}",
                self.scroll.offset()
            ),
            "cancel" => ensure!(
                !surface && composer_focus && self.selected == INITIAL && self.attempts == 0,
                "escape selected or lost focus"
            ),
            "empty" | "empty-enter" => ensure!(
                surface
                    && search_focus
                    && self.cursor.is_none()
                    && probe::visible(window, "empty-results")
                    && self.attempts == 0,
                "empty state/enter"
            ),
            "disabled-filter" | "disabled-filter-enter" => ensure!(
                surface
                    && search_focus
                    && self.cursor.is_none()
                    && self.selected == INITIAL
                    && self.attempts == 0
                    && probe::visible(window, "model-7"),
                "disabled-only filtered result became selectable"
            ),
            "filtered" => {
                ensure!(
                    surface
                        && search_focus
                        && self.cursor == Some(LAST)
                        && !probe::visible(window, "model-0"),
                    "filter failed"
                );
                self.assert_row(window, LAST)?;
            }
            "chosen" => ensure!(
                !surface
                    && composer_focus
                    && self.selected == LAST
                    && self.attempts == 1
                    && probe::visible(window, "model-trigger")
                    && window.find("model-trigger").bounds().size.width > px(0.),
                "enter selection"
            ),
            "failure" => ensure!(
                !surface
                    && composer_focus
                    && self.selected == LAST
                    && self.notice
                    && probe::visible(window, "model-notice")
                    && self.attempts == 2,
                "failed choice lost previous value/notice"
            ),
            "outside" => ensure!(
                !surface && composer_focus && self.selected == LAST && self.attempts == 2,
                "outside selected"
            ),
            "retry" => ensure!(
                !surface
                    && composer_focus
                    && self.selected == 0
                    && !self.notice
                    && self.attempts == 3,
                "retry failed"
            ),
            _ => bail!("unknown check {name}"),
        }
        let bounds = if surface {
            Some(probe::inside(
                window,
                "model-surface",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        Ok(
            json!({"open": surface, "selected": self.selected, "cursor": self.cursor, "query": self.query,
            "draft": draft, "search_focus": search_focus, "composer_focus": composer_focus,
            "scroll_y": f32::from(self.scroll.offset().y), "attempts": self.attempts, "notice": self.notice, "bounds": bounds}),
        )
    }
}
impl ModelsProbe {
    fn assert_row(&self, window: &Window, ix: usize) -> Result<()> {
        let row = window.find(model_id(ix)).bounds();
        let list = window.find("model-list").bounds();
        ensure!(
            row.top() >= list.top() - px(1.) && row.bottom() <= list.bottom() + px(1.),
            "selected row {ix} not in scroll viewport: {row:?} {list:?}"
        );
        Ok(())
    }
}
