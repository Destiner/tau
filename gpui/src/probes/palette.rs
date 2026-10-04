use super::probe::{self, Palette, Probe, Step};
use anyhow::{Result, bail, ensure};
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, ScrollHandle,
    Window,
    base::{
        Button, Dialog, DialogBackdrop, DialogPopup, DialogTitle, Input, TestSupportExt,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};

#[derive(Clone, Copy)]
struct Command {
    title: &'static str,
    detail: &'static str,
    shortcut: &'static str,
    group: &'static str,
    disabled: bool,
}
gpui_kit::actions!(palette_probe, [NextRow, PreviousRow, FirstRow, LastRow]);

const COMMANDS: &[Command] = &[
    Command {
        title: "Switch Session",
        detail: "Browse recent sessions",
        shortcut: "⌘O",
        group: "Navigate",
        disabled: false,
    },
    Command {
        title: "New Session",
        detail: "Start a local session",
        shortcut: "⌘N",
        group: "Navigate",
        disabled: false,
    },
    Command {
        title: "Rename Session",
        detail: "Rename current session",
        shortcut: "",
        group: "Actions",
        disabled: false,
    },
    Command {
        title: "Delete Session",
        detail: "Not available in this fixture",
        shortcut: "⌘⌫",
        group: "Actions",
        disabled: true,
    },
    Command {
        title: "Toggle Sidebar",
        detail: "Show or hide navigation",
        shortcut: "⌘\\",
        group: "View",
        disabled: false,
    },
];
const SESSIONS: &[Command] = &[
    Command {
        title: "Alpha",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Bravo",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Charlie",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Delta",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Echo",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Foxtrot",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Golf",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Hotel",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "India",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Juliet",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Kilo",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
    Command {
        title: "Lima",
        detail: "Recent session",
        shortcut: "",
        group: "Recent",
        disabled: false,
    },
];

pub struct PaletteProbe {
    p: Palette,
    open: bool,
    nested: bool,
    selected: usize,
    counts: [usize; 5],
    session_count: usize,
    trigger: FocusHandle,
    dialog_focus: FocusHandle,
    back_focus: FocusHandle,
    close_focus: FocusHandle,
    composer: Entity<InputState>,
    search: Entity<InputState>,
    scroll: ScrollHandle,
    _subscriptions: Vec<gpui_kit::Subscription>,
}
impl PaletteProbe {
    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().to_string()
    }
    fn items(&self) -> &'static [Command] {
        if self.nested { SESSIONS } else { COMMANDS }
    }
    fn filtered(&self, cx: &App) -> Vec<usize> {
        let tokens = self.query(cx).to_lowercase();
        let tokens: Vec<_> = tokens.split_whitespace().collect();
        self.items()
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                tokens.iter().all(|token| {
                    [item.title, item.detail, item.group]
                        .iter()
                        .any(|text| text.to_lowercase().contains(token))
                })
            })
            .map(|(i, _)| i)
            .collect()
    }
    fn select_first(&mut self, cx: &mut Context<Self>) {
        self.selected = self
            .filtered(cx)
            .into_iter()
            .find(|&i| !self.items()[i].disabled)
            .unwrap_or(usize::MAX);
        self.reveal(cx);
    }
    fn reveal(&self, cx: &App) {
        let mut row = 0;
        let mut last = "";
        for i in self.filtered(cx) {
            let item = self.items()[i];
            if item.group != last {
                row += 1;
                last = item.group;
            }
            if i == self.selected {
                self.scroll.scroll_to_item(row);
                break;
            }
            row += 1;
        }
    }
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.nested = false;
        self.search.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        self.select_first(cx);
        cx.notify();
    }
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.nested = false;
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.composer.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.nested {
            return;
        }
        self.nested = false;
        self.search.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        self.select_first(cx);
        cx.notify();
    }
    fn activate(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || self.items().get(i).is_none_or(|item| item.disabled) {
            return;
        }
        if self.nested {
            self.session_count += 1;
            self.dismiss(window, cx);
        } else if i == 0 {
            self.nested = true;
            self.search.update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.focus(window, cx);
            });
            self.select_first(cx);
            cx.notify();
        } else {
            self.counts[i] += 1;
            self.dismiss(window, cx);
        }
    }
    fn move_row(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let choices: Vec<_> = self
            .filtered(cx)
            .into_iter()
            .filter(|&i| !self.items()[i].disabled)
            .collect();
        if !choices.is_empty() {
            let current = choices
                .iter()
                .position(|&i| i == self.selected)
                .unwrap_or(0);
            let next = match key {
                "down" => (current + 1) % choices.len(),
                "up" => (current + choices.len() - 1) % choices.len(),
                "home" => 0,
                _ => choices.len() - 1,
            };
            self.selected = choices[next];
            self.reveal(cx);
            cx.notify();
        }
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if event.keystroke.modifiers.platform && key == "k" {
            if !self.open {
                self.open(window, cx);
            }
        } else if !self.open {
            return;
        } else if key == "escape" {
            if self.nested {
                self.back(window, cx);
            } else {
                self.dismiss(window, cx);
            }
        } else if self.nested
            && ((event.keystroke.modifiers.platform && key == "[")
                || (self.query(cx).is_empty() && (key == "backspace" || key == "delete")))
        {
            self.back(window, cx);
        } else if matches!(key, "down" | "up" | "home" | "end") {
            self.move_row(key, cx);
        } else if key == "enter" {
            self.activate(self.selected, window, cx);
        } else {
            return;
        }
        cx.stop_propagation();
    }
    fn row(&self, i: usize, cx: &mut Context<Self>) -> Button {
        let item = self.items()[i];
        let p = self.p;
        Button::new(format!("row-{i}"))
            .w_full()
            .h(px(44.))
            .flex_shrink_0()
            .px(px(12.))
            .bg(rgb(if self.selected == i {
                p.hover
            } else {
                p.surface
            }))
            .text_color(rgb(if item.disabled { p.muted } else { p.text }))
            .disabled(item.disabled)
            .tab_stop(false)
            .focusable(false)
            .on_click(cx.listener(move |s, _, w, cx| s.activate(i, w, cx)))
            .on_mouse_move(cx.listener(move |s, _, _, cx| {
                if !s.items()[i].disabled && s.selected != i {
                    s.selected = i;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div().flex().flex_col().child(item.title).child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(p.muted))
                                .child(item.detail),
                        ),
                    )
                    .child(div().text_color(rgb(p.muted)).child(item.shortcut)),
            )
    }
}
impl Render for PaletteProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let mut page = probe::page(p)
            .on_key_down(cx.listener(Self::key))
            .child(div().text_size(px(18.)).child("Command Palette"))
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("Local fixture · ⌘K to open"),
            )
            .child(
                probe::button("palette-trigger", "Open Commands  ⌘K", p)
                    .w(px(200.))
                    .track_focus(&self.trigger)
                    .on_click(cx.listener(|s, _, w, cx| s.open(w, cx))),
            )
            .child(
                div().child("Composer draft").child(
                    div()
                        .id("composer")
                        .test_support()
                        .w(px(540.))
                        .h(px(38.))
                        .border_1()
                        .border_color(rgb(p.border))
                        .child(Input::new(&self.composer)),
                ),
            )
            .child(
                div()
                    .id("fixture-counts")
                    .test_support()
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "Commands: {:?} · Sessions: {}",
                        self.counts, self.session_count
                    )),
            );
        if self.open {
            let mut rows = Vec::new();
            let mut last = "";
            for i in self.filtered(cx) {
                let item = self.items()[i];
                if item.group != last {
                    last = item.group;
                    rows.push(
                        div()
                            .h(px(26.))
                            .flex_shrink_0()
                            .px(px(12.))
                            .pt(px(5.))
                            .text_size(px(11.))
                            .text_color(rgb(p.muted))
                            .child(item.group)
                            .into_any_element(),
                    );
                }
                rows.push(self.row(i, cx).into_any_element());
            }
            let weak = cx.entity().downgrade();
            page = page.child(
                Dialog::new(cx)
                    .focus_handle(self.dialog_focus.clone())
                    .close_on_escape(false)
                    .close_on_backdrop_press(true)
                    .on_cancel(move |_, w, cx| {
                        weak.update(cx, |s, cx| s.dismiss(w, cx)).ok();
                        false
                    })
                    .backdrop(
                        DialogBackdrop::new()
                            .absolute()
                            .size_full()
                            .bg(gpui_kit::rgba(0x00000088)),
                    )
                    .popup(
                        DialogPopup::new().child(
                            div()
                                .id("palette-surface")
                                .test_support()
                                .w(px(530.))
                                .rounded(px(9.))
                                .border_1()
                                .border_color(rgb(p.border))
                                .bg(rgb(p.surface))
                                .shadow_lg()
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .on_key_down(cx.listener(Self::key))
                                .on_action(
                                    cx.listener(|s, _: &NextRow, _, cx| s.move_row("down", cx)),
                                )
                                .on_action(
                                    cx.listener(|s, _: &PreviousRow, _, cx| s.move_row("up", cx)),
                                )
                                .on_action(
                                    cx.listener(|s, _: &FirstRow, _, cx| s.move_row("home", cx)),
                                )
                                .on_action(
                                    cx.listener(|s, _: &LastRow, _, cx| s.move_row("end", cx)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .p(px(10.))
                                        .child(if self.nested {
                                            probe::button("palette-back", "‹ Back", p)
                                                .track_focus(&self.back_focus)
                                                .on_click(cx.listener(|s, _, w, cx| s.back(w, cx)))
                                                .into_any_element()
                                        } else {
                                            div()
                                                .text_color(rgb(p.muted))
                                                .child("⌕")
                                                .into_any_element()
                                        })
                                        .child(
                                            div()
                                                .id("palette-search")
                                                .test_support()
                                                .key_context("PaletteSearch")
                                                .flex_1()
                                                .h(px(34.))
                                                .child(Input::new(&self.search))
                                                .into_any_element(),
                                        )
                                        .child(
                                            probe::button("palette-close", "Close", p)
                                                .track_focus(&self.close_focus)
                                                .on_click(
                                                    cx.listener(|s, _, w, cx| s.dismiss(w, cx)),
                                                ),
                                        ),
                                )
                                .child(
                                    DialogTitle::new()
                                        .h(px(26.))
                                        .flex_shrink_0()
                                        .px(px(12.))
                                        .text_color(rgb(p.muted))
                                        .text_size(px(11.))
                                        .child(if self.nested {
                                            "Switch Session"
                                        } else {
                                            "Commands"
                                        }),
                                )
                                .child(
                                    div()
                                        .id("palette-results")
                                        .test_support()
                                        .h(px(244.))
                                        .flex()
                                        .flex_col()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll)
                                        .children(rows)
                                        .when(self.filtered(cx).is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .id("palette-empty")
                                                    .test_support()
                                                    .p(px(16.))
                                                    .text_color(rgb(p.muted))
                                                    .child(if self.query(cx).trim().is_empty() {
                                                        "No commands available"
                                                    } else {
                                                        "No matches"
                                                    }),
                                            )
                                        }),
                                ),
                        ),
                    ),
            );
        }
        page
    }
}
impl Probe for PaletteProbe {
    const ID: &'static str = "palette";
    const TITLE: &'static str = "Tau · Command Palette";
    const DESCRIPTION: &'static str = "Isolated native searchable grouped command palette";
    const LIMITATIONS: &'static str = "Synthetic commands and sessions; no Pi, storage, Vue integration or accessibility parity claim.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = Palette::new(dark);
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextRow, Some("PaletteSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousRow, Some("PaletteSearch > Input")),
            gpui_kit::KeyBinding::new("home", FirstRow, Some("PaletteSearch > Input")),
            gpui_kit::KeyBinding::new("end", LastRow, Some("PaletteSearch > Input")),
        ]);
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Type a draft…"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search commands…"));
        composer.update(cx, |s, _| s.set_editor_style(probe::text_style(p)));
        search.update(cx, |s, _| s.set_editor_style(probe::text_style(p)));
        let subscription = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.select_first(cx);
                cx.notify();
            }
        });
        Self {
            p,
            open: false,
            nested: false,
            selected: 0,
            counts: [0; 5],
            session_count: 0,
            trigger: cx.focus_handle(),
            dialog_focus: cx.focus_handle(),
            back_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            composer,
            search,
            scroll: ScrollHandle::new(),
            _subscriptions: vec![subscription],
        }
    }
    fn steps() -> Vec<Step> {
        use Step::*;
        vec![
            Check("initial"),
            Click("composer"),
            Type("Retained draft"),
            Check("composer-typed"),
            Key("left"),
            Check("composer-left"),
            Key("right"),
            Check("composer-right"),
            Key("home"),
            Check("composer-home"),
            Key("up"),
            Check("composer-up"),
            Key("end"),
            Check("composer-end"),
            Key("down"),
            Check("composer-down"),
            Key("cmd-k"),
            Check("shortcut-open"),
            Type("s"),
            Check("search-insertion"),
            Key("down"),
            Check("search-down"),
            Key("home"),
            Check("search-home"),
            Key("right"),
            Type("x"),
            Check("search-edit"),
            Key("escape"),
            Check("search-dismissed"),
            Key("escape"),
            Check("closed-escape"),
            Key("cmd-k"),
            Check("reopened-clean"),
            Key("tab"),
            Check("tab-close"),
            Key("tab"),
            Check("tab-wrap"),
            Key("shift-tab"),
            Check("reverse-wrap"),
            Click("palette-search"),
            Type("zzzz-no-results"),
            Check("empty"),
            Key("cmd-a"),
            Type("delete"),
            Check("disabled"),
            Click("row-3"),
            Check("disabled-click"),
            Key("cmd-a"),
            Type("switch"),
            Check("filtered"),
            Key("enter"),
            Check("nested"),
            Key("tab"),
            Check("nested-tab-close"),
            Key("tab"),
            Check("nested-tab-back"),
            Key("tab"),
            Check("nested-tab-wrap"),
            Key("end"),
            Check("end-visible"),
            Key("home"),
            Check("home-visible"),
            Key("down"),
            Check("arrow-down"),
            Key("escape"),
            Check("back-escape"),
            Key("escape"),
            Check("dismissed"),
            Click("palette-trigger"),
            Check("trigger-open"),
            Click("row-0"),
            Click("palette-back"),
            Check("back-button"),
            Click("row-1"),
            Check("activated"),
            Key("cmd-k"),
            Click("row-0"),
            Key("end"),
            Check("end-visible-again"),
            Key("enter"),
            Check("session-activated"),
        ]
    }
    fn check(&self, name: &str, window: &Window, cx: &App) -> Result<Value> {
        let query = self.query(cx);
        let draft = self.composer.read(cx).value().to_string();
        let caret = self.composer.read(cx).selected_range();
        let focus = if self.search.read(cx).focus_handle(cx).is_focused(window) {
            "search"
        } else if self.close_focus.is_focused(window) {
            "close"
        } else if self.back_focus.is_focused(window) {
            "back"
        } else if self.composer.read(cx).focus_handle(cx).is_focused(window) {
            "composer"
        } else {
            "other"
        };
        let surface = probe::visible(window, "palette-surface");
        match name {
            "initial" => ensure!(
                !surface && probe::visible(window, "palette-trigger"),
                "initial"
            ),
            "composer-typed" | "composer-left" | "composer-right" | "composer-home"
            | "composer-up" | "composer-end" | "composer-down" => {
                let position = match name {
                    "composer-left" => 13,
                    "composer-home" | "composer-up" => 0,
                    _ => 14,
                };
                ensure!(
                    !surface
                        && focus == "composer"
                        && caret == (position..position)
                        && draft == "Retained draft"
                        && self.selected == 0
                        && query.is_empty(),
                    "composer editing {name}: {caret:?}"
                );
            }
            "shortcut-open" | "trigger-open" | "reopened-clean" => ensure!(
                surface
                    && focus == "search"
                    && !self.nested
                    && query.is_empty()
                    && self.selected == 0
                    && caret == (14..14),
                "open focus/selection/query"
            ),
            "search-insertion" | "search-home" | "search-down" => ensure!(
                surface
                    && focus == "search"
                    && query == "s"
                    && self.search.read(cx).selected_range() == (1..1)
                    && self.selected == if name == "search-down" { 1 } else { 0 },
                "search navigation/editing {name}"
            ),
            "search-edit" => ensure!(
                surface
                    && focus == "search"
                    && query == "sx"
                    && self.search.read(cx).selected_range() == (2..2)
                    && self.selected == usize::MAX,
                "search editing after navigation"
            ),
            "search-dismissed" | "closed-escape" => ensure!(
                !surface
                    && focus == "composer"
                    && query.is_empty()
                    && self.selected == usize::MAX
                    && caret == (14..14),
                "dismiss/closed escape leaked"
            ),
            "tab-close" | "reverse-wrap" => {
                ensure!(surface && focus == "close", "tab cycle to close")
            }
            "tab-wrap" => ensure!(surface && focus == "search", "tab cycle to input"),
            "empty" => ensure!(
                surface && probe::visible(window, "palette-empty") && self.selected == usize::MAX,
                "empty state"
            ),
            "disabled" | "disabled-click" => ensure!(
                surface && self.selected == usize::MAX && self.counts == [0; 5],
                "disabled command"
            ),
            "filtered" => ensure!(self.selected == 0 && self.filtered(cx) == [0], "filter"),
            "nested" | "nested-tab-wrap" => ensure!(
                self.nested && surface && focus == "search" && query.is_empty(),
                "nested focus"
            ),
            "nested-tab-back" => {
                ensure!(
                    self.nested && surface && focus == "back",
                    "nested tab back: {focus}"
                )
            }
            "nested-tab-close" => ensure!(
                self.nested && surface && focus == "close",
                "nested tab close: {focus}"
            ),
            "end-visible" | "end-visible-again" | "home-visible" => {
                ensure!(
                    self.selected
                        == if name.starts_with("end-visible") {
                            11
                        } else {
                            0
                        },
                    "navigation selection"
                );
                let row = window.find(format!("row-{}", self.selected)).bounds();
                let viewport = window.find("palette-results").bounds();
                ensure!(
                    row.top() >= viewport.top() && row.bottom() <= viewport.bottom(),
                    "selected row outside viewport: {row:?} {viewport:?}"
                );
            }
            "arrow-down" => ensure!(
                self.nested && self.selected == 1 && focus == "search",
                "arrow navigation"
            ),
            "back-escape" | "back-button" => ensure!(
                surface && !self.nested && focus == "search" && query.is_empty(),
                "back hierarchy"
            ),
            "dismissed" => ensure!(
                !surface && focus == "composer" && draft == "Retained draft",
                "draft/focus lost"
            ),
            "activated" => ensure!(
                !surface && self.counts[1] == 1 && focus == "composer",
                "command activation"
            ),
            "session-activated" => ensure!(
                !surface && self.session_count == 1 && focus == "composer",
                "session activation"
            ),
            _ => bail!("unknown check {name}"),
        }
        ensure!(draft == "Retained draft" || name == "initial", "draft lost");
        let bounds = if surface {
            Some(probe::inside(
                window,
                "palette-surface",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        Ok(
            json!({"open": surface, "nested": self.nested, "query": query, "selected": self.selected,
            "focus": focus, "draft": draft, "counts": self.counts, "sessions": self.session_count,
            "caret": [caret.start, caret.end], "scroll_y": f32::from(self.scroll.offset().y), "bounds": bounds}),
        )
    }
}
