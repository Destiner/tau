use super::probe::{self, Palette, Probe, Step};
use anyhow::{Result, ensure};
use gpui_kit::{
    Anchor, App, Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton, Render, Window,
    base::{Button, Popup, TestSupportExt},
    div,
    prelude::*,
    px, rgb,
    test::TestWindowExt,
};
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Dropdown,
    Context,
}

#[derive(Clone, Copy)]
struct Item {
    label: &'static str,
    shortcut: &'static str,
    disabled: bool,
}
const ITEMS: [Item; 3] = [
    Item {
        label: "Open Session",
        shortcut: "⌘O",
        disabled: false,
    },
    Item {
        label: "Rename",
        shortcut: "↩",
        disabled: false,
    },
    Item {
        label: "Delete Session",
        shortcut: "⌘⌫",
        disabled: true,
    },
];

pub struct MenusProbe {
    p: Palette,
    dropdown: bool,
    context: bool,
    edge: bool,
    selection: usize,
    count: [usize; 3],
    trigger: FocusHandle,
    target: FocusHandle,
    dropdown_focus: FocusHandle,
    context_focus: FocusHandle,
    _keys: gpui_kit::Subscription,
}
impl MenusProbe {
    fn focus(&self, layer: Layer) -> &FocusHandle {
        match layer {
            Layer::Dropdown => &self.dropdown_focus,
            Layer::Context => &self.context_focus,
        }
    }
    fn open(&mut self, layer: Layer, window: &mut Window, cx: &mut Context<Self>) {
        match layer {
            Layer::Dropdown => {
                self.dropdown = true;
                self.context = false;
                self.selection = 0;
            }
            Layer::Context => {
                self.context = true;
                self.selection = 0;
            }
        }
        self.focus(layer).focus(window, cx);
        cx.notify();
    }
    fn close(&mut self, layer: Layer, window: &mut Window, cx: &mut Context<Self>) {
        match layer {
            Layer::Dropdown => {
                self.dropdown = false;
                self.context = false;
                self.trigger.focus(window, cx);
            }
            Layer::Context => {
                self.context = false;
                if self.dropdown {
                    self.dropdown_focus.focus(window, cx);
                } else {
                    self.target.focus(window, cx);
                }
            }
        }
        cx.notify();
    }
    fn activate(
        &mut self,
        index: usize,
        layer: Layer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ITEMS[index].disabled {
            return;
        }
        self.count[index] += 1;
        self.close(layer, window, cx);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let layer = if self.context && self.context_focus.is_focused(window) {
            Layer::Context
        } else if self.dropdown && self.dropdown_focus.is_focused(window) {
            Layer::Dropdown
        } else {
            return;
        };
        match event.keystroke.key.as_str() {
            "tab"
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform =>
            {
                let direction = if event.keystroke.modifiers.shift {
                    -1
                } else {
                    1
                };
                self.selection = (1..=ITEMS.len())
                    .map(|n| {
                        (self.selection as isize + direction * n as isize)
                            .rem_euclid(ITEMS.len() as isize) as usize
                    })
                    .find(|&i| !ITEMS[i].disabled)
                    .unwrap_or(self.selection);
                cx.notify();
            }
            "escape" => self.close(layer, window, cx),
            "down" => {
                self.selection = (1..=ITEMS.len())
                    .map(|n| (self.selection + n) % ITEMS.len())
                    .find(|&i| !ITEMS[i].disabled)
                    .unwrap_or(self.selection);
                cx.notify();
            }
            "up" => {
                self.selection = (1..=ITEMS.len())
                    .map(|n| (self.selection + ITEMS.len() - n) % ITEMS.len())
                    .find(|&i| !ITEMS[i].disabled)
                    .unwrap_or(self.selection);
                cx.notify();
            }
            "enter" | "space" => self.activate(self.selection, layer, window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
    fn menu(&self, layer: Layer, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let focus = self.focus(layer).clone();
        let rows = ITEMS
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = self.selection == i && self.focus(layer).is_focused(window);
                let id = match layer {
                    Layer::Dropdown => match i {
                        0 => "drop-open",
                        1 => "drop-rename",
                        _ => "drop-delete",
                    },
                    Layer::Context => match i {
                        0 => "context-open",
                        1 => "context-rename",
                        _ => "context-delete",
                    },
                };
                Button::new(id)
                    .w_full()
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(rgb(if selected { p.hover } else { p.surface }))
                    .text_size(px(12.))
                    .text_color(rgb(if item.disabled { p.muted } else { p.text }))
                    .disabled(item.disabled)
                    .tab_stop(false)
                    .focusable(false)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.activate(i, layer, window, cx)),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if !ITEMS[i].disabled && this.selection != i {
                            this.selection = i;
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, _, window, cx| this.open(Layer::Context, window, cx)),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(item.label)
                            .child(div().text_color(rgb(p.muted)).child(item.shortcut)),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .id(match layer {
                Layer::Dropdown => "dropdown-surface",
                Layer::Context => "context-surface",
            })
            .test_support()
            .w(px(218.))
            .p(px(4.))
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.surface))
            .shadow_lg()
            .flex()
            .flex_col()
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down_out(
                cx.listener(move |this, _, window, cx| this.close(layer, window, cx)),
            )
            .children(rows)
    }
    fn status(&self) -> String {
        format!(
            "Open {}  ·  Rename {}  ·  Delete {}",
            self.count[0], self.count[1], self.count[2]
        )
    }
}
impl Render for MenusProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let trigger = probe::button("menu-trigger", "Session  ▾", p)
            .w(px(218.))
            .track_focus(&self.trigger)
            .on_click(cx.listener(|this, _, window, cx| {
                if this.dropdown {
                    this.close(Layer::Dropdown, window, cx);
                } else {
                    this.open(Layer::Dropdown, window, cx);
                }
            }));
        let dropdown = Popup::new("dropdown-popup", trigger)
            .anchor(if self.edge {
                Anchor::BottomLeft
            } else {
                Anchor::TopLeft
            })
            .offset(px(5.))
            .when(self.dropdown, |popup| {
                popup.content(self.menu(Layer::Dropdown, window, cx))
            });
        let target = probe::button("context-target", "Session row  ·  Right-click", p)
            .w(px(218.))
            .track_focus(&self.target)
            .on_click(cx.listener(|this, _, window, cx| this.open(Layer::Context, window, cx)))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| this.open(Layer::Context, window, cx)),
            );
        let context = Popup::new("context-popup", target)
            .ml(px(if self.edge { 40. } else { 250. }))
            .anchor(Anchor::TopLeft)
            .offset(px(5.))
            .when(self.context, |popup| {
                popup.content(self.menu(Layer::Context, window, cx))
            });
        probe::page(p)
            .child(
                div()
                    .text_size(px(18.))
                    .text_color(rgb(p.text))
                    .child("Menus"),
            )
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("Session actions · native dropdown and context surface"),
            )
            .child(
                probe::button(
                    "edge-toggle",
                    if self.edge {
                        "Edge placement: On"
                    } else {
                        "Edge placement: Off"
                    },
                    p,
                )
                .w(px(218.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edge = !this.edge;
                    cx.notify();
                })),
            )
            .child(div().text_color(rgb(p.muted)).child(self.status()))
            .child(
                probe::button("reset", "Reset", p)
                    .w(px(218.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dropdown = false;
                        this.context = false;
                        this.edge = false;
                        this.selection = 0;
                        this.count = [0; 3];
                        this.trigger.focus(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(self.edge, |d| d.mt(px(460.)).ml(px(580.)))
                    .child(dropdown)
                    .child(context),
            )
    }
}
impl Probe for MenusProbe {
    const ID: &'static str = "menus";
    const TITLE: &'static str = "Tau — Native Menus";
    const DESCRIPTION: &'static str =
        "Dropdown, right-click menu, keyboard navigation, dismissal and edge clamping.";
    const LIMITATIONS: &'static str = "Synthetic session actions only; no Pi, filesystem or clipboard. This is a native interaction probe, not a Vue parity claim.";
    fn new(dark: bool, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if event.keystroke.key == "tab"
                && let Some(scene) = weak.upgrade()
                && (scene.read(cx).dropdown_focus.is_focused(window)
                    || scene.read(cx).context_focus.is_focused(window))
            {
                scene.update(cx, |s, cx| {
                    let direction = if event.keystroke.modifiers.shift {
                        -1
                    } else {
                        1
                    };
                    s.selection = (1..=ITEMS.len())
                        .map(|n| {
                            (s.selection as isize + direction * n as isize)
                                .rem_euclid(ITEMS.len() as isize)
                                as usize
                        })
                        .find(|&i| !ITEMS[i].disabled)
                        .unwrap_or(s.selection);
                    cx.notify();
                });
                cx.stop_propagation();
            }
        });
        Self {
            p: Palette::new(dark),
            dropdown: false,
            context: false,
            edge: false,
            selection: 0,
            count: [0; 3],
            trigger: cx.focus_handle(),
            target: cx.focus_handle(),
            dropdown_focus: cx.focus_handle(),
            context_focus: cx.focus_handle(),
            _keys: keys,
        }
    }
    fn steps() -> Vec<Step> {
        vec![
            Step::Check("initial"),
            Step::Click("menu-trigger"),
            Step::Check("dropdown"),
            Step::Click("drop-delete"),
            Step::Check("disabled"),
            Step::Click("edge-toggle"),
            Step::Check("outside"),
            Step::Click("edge-toggle"),
            Step::Click("menu-trigger"),
            Step::Hover("drop-rename"),
            Step::Check("hover"),
            Step::Key("up"),
            Step::Key("down"),
            Step::Check("navigation"),
            Step::Key("tab"),
            Step::Check("tab-contained"),
            Step::Key("shift-tab"),
            Step::Check("navigation-restored"),
            Step::Key("enter"),
            Step::Check("keyboard-activation"),
            Step::Click("menu-trigger"),
            Step::RightClick("drop-rename"),
            Step::Check("stacked"),
            Step::Key("escape"),
            Step::Check("topmost-escape"),
            Step::Key("escape"),
            Step::Check("restore"),
            Step::RightClick("context-target"),
            Step::Check("context"),
            Step::Click("context-open"),
            Step::Check("pointer-activation"),
            Step::Click("edge-toggle"),
            Step::Click("menu-trigger"),
            Step::Check("edge"),
            Step::Click("reset"),
            Step::Check("reset"),
        ]
    }
    fn check(&self, name: &str, window: &Window, _: &App) -> Result<Value> {
        let drop = probe::visible(window, "dropdown-surface");
        let context = probe::visible(window, "context-surface");
        let focus = if self.context_focus.is_focused(window) {
            "context"
        } else if self.dropdown_focus.is_focused(window) {
            "dropdown"
        } else if self.trigger.is_focused(window) {
            "trigger"
        } else if self.target.is_focused(window) {
            "target"
        } else {
            "other"
        };
        match name {
            "initial" | "reset" => ensure!(
                !drop
                    && !context
                    && self.count == [0; 3]
                    && (name == "initial" || (!self.edge && focus == "trigger")),
                "initial/reset state"
            ),
            "dropdown" => ensure!(
                drop && !context && focus == "dropdown" && self.selection == 0,
                "dropdown state"
            ),
            "disabled" => ensure!(
                drop && self.count == [0; 3] && self.selection == 0,
                "disabled item acted"
            ),
            "outside" => ensure!(
                !drop && !context && self.edge && self.count == [0; 3],
                "outside click did not close"
            ),
            "tab-contained" => ensure!(
                drop && self.selection == 0 && focus == "dropdown" && !context,
                "Tab escaped menu or selected disabled item"
            ),
            "hover" | "navigation" | "navigation-restored" => ensure!(
                drop && self.selection == 1 && focus == "dropdown",
                "highlight selection"
            ),
            "keyboard-activation" => ensure!(
                !drop && self.count == [0, 1, 0] && focus == "trigger",
                "enter did not activate"
            ),
            "stacked" => ensure!(
                drop && context && focus == "context",
                "stack not open: drop={drop} context={context} focus={focus}"
            ),
            "topmost-escape" => ensure!(
                drop && !context && focus == "dropdown",
                "escape dismissed wrong layer: drop={drop} context={context} focus={focus}"
            ),
            "restore" => ensure!(
                !drop && !context && focus == "trigger",
                "focus not restored"
            ),
            "context" => {
                ensure!(
                    context && !drop && focus == "context",
                    "context focus/layer"
                );
                let b = window.find("context-surface").bounds();
                let target = window.find("context-target").bounds();
                ensure!(
                    b.left() >= target.left() - px(1.) && b.top() >= target.bottom() - px(1.),
                    "context menu not anchored near row: {b:?} {target:?}"
                );
            }
            "pointer-activation" => ensure!(
                !context && self.count == [1, 1, 0] && focus == "target",
                "pointer activation"
            ),
            "edge" => {
                ensure!(
                    self.edge && drop && focus == "dropdown",
                    "edge surface not open"
                );
                let bounds = window.find("dropdown-surface").bounds();
                let trigger = window.find("menu-trigger").bounds();
                ensure!(
                    bounds.bottom() <= trigger.top() && bounds.left() >= trigger.left() - px(1.),
                    "popup did not reposition above near-edge trigger: {bounds:?} {trigger:?}"
                );
            }
            _ => anyhow::bail!("unknown check: {name}"),
        }
        let geometry = if drop {
            Some(probe::inside(
                window,
                "dropdown-surface",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else if context {
            Some(probe::inside(
                window,
                "context-surface",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        Ok(
            json!({"dropdown": drop, "context": context, "focus": focus, "selection": self.selection,
            "counts": self.count, "edge": self.edge, "bounds": geometry}),
        )
    }
}
