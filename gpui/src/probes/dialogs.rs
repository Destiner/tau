use super::probe::{self as common, Palette, Probe, Step};
use anyhow::{Result, ensure};
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window,
    base::{
        AlertDialog, Dialog, DialogBackdrop, DialogDescription, DialogPopup, DialogTitle, Input,
        TestSupportExt, Textarea,
        input::{InputState, TextareaState},
    },
    div,
    prelude::*,
    px, rgb,
};
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layer {
    None,
    Edit,
    Quit,
    Nested,
}

pub struct DialogsProbe {
    p: Palette,
    layer: Layer,
    inline: bool,
    busy: bool,
    error: bool,
    attempts: usize,
    trigger: FocusHandle,
    quit_trigger: FocusHandle,
    edit_focus: FocusHandle,
    quit_focus: FocusHandle,
    nested_focus: FocusHandle,
    nested_open_focus: FocusHandle,
    edit_cancel_focus: FocusHandle,
    edit_save_focus: FocusHandle,
    finish_focus: FocusHandle,
    quit_finish_focus: FocusHandle,
    quit_cancel_focus: FocusHandle,
    quit_confirm_focus: FocusHandle,
    nested_close_focus: FocusHandle,
    inline_cancel_focus: FocusHandle,
    inline_focus: FocusHandle,
    input: Entity<InputState>,
    inline_input: Entity<InputState>,
    textarea: Entity<TextareaState>,
    _keys: Subscription,
}
impl DialogsProbe {
    fn open(&mut self, layer: Layer, window: &mut Window, cx: &mut Context<Self>) {
        self.layer = layer;
        self.error = false;
        match layer {
            Layer::Edit => self.input.read(cx).focus_handle(cx).focus(window, cx),
            Layer::Quit => self.quit_cancel_focus.focus(window, cx),
            Layer::Nested => self.nested_close_focus.focus(window, cx),
            Layer::None => self.trigger.focus(window, cx),
        }
        cx.notify();
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let old = self.layer;
        self.layer = if old == Layer::Nested {
            Layer::Edit
        } else {
            Layer::None
        };
        let focus = match old {
            Layer::Nested => &self.nested_open_focus,
            Layer::Quit => &self.quit_trigger,
            _ => &self.trigger,
        };
        focus.focus(window, cx);
        cx.notify();
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = false;
        self.attempts += 1;
        cx.notify();
    }
    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busy {
            return;
        }
        self.busy = false;
        self.error = true;
        match self.layer {
            Layer::Edit => self.edit_save_focus.focus(window, cx),
            Layer::Quit => self.quit_cancel_focus.focus(window, cx),
            _ => {}
        }
        cx.notify();
    }
    fn popup_style(
        &self,
        id: &'static str,
        width: f32,
    ) -> gpui_kit::base::ObservedElement<gpui_kit::Stateful<gpui_kit::Div>> {
        div()
            .id(id)
            .w(px(width))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(self.p.border))
            .bg(rgb(self.p.surface))
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .test_support()
    }
    fn backdrop(&self) -> DialogBackdrop {
        DialogBackdrop::new()
            .absolute()
            .size_full()
            .bg(gpui_kit::rgba(0x00000088))
    }
    fn header(&self, title: &'static str, description: &'static str) -> gpui_kit::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(12.))
            .border_b_1()
            .border_color(rgb(self.p.border))
            .child(
                DialogTitle::new()
                    .text_color(rgb(self.p.text))
                    .text_size(px(14.))
                    .child(title),
            )
            .child(
                DialogDescription::new()
                    .text_color(rgb(self.p.muted))
                    .text_size(px(12.))
                    .child(description),
            )
    }
    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        disabled: bool,
        focus: &FocusHandle,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui_kit::base::Button {
        common::button(id, label, self.p)
            .disabled(disabled)
            .track_focus(focus)
            .on_click(cx.listener(move |this, _, window, cx| f(this, window, cx)))
    }
    fn edit_dialog(&self, cx: &mut Context<Self>) -> Dialog {
        let weak = cx.entity().downgrade();
        let weak_ok = weak.clone();
        Dialog::new(cx)
            .layer(0, self.layer != Layer::Nested)
            .focus_handle(self.edit_focus.clone())
            .close_on_escape(!self.busy && self.layer != Layer::Nested)
            .close_on_backdrop_press(!self.busy && self.layer != Layer::Nested)
            .on_cancel(move |_, window, cx| {
                weak.update(cx, |s, cx| s.close(window, cx)).ok();
                false
            })
            .on_ok(move |_, _, cx| {
                weak_ok.update(cx, |s, cx| s.submit(cx)).ok();
                false
            })
            .backdrop(self.backdrop())
            .popup(
                DialogPopup::new().child(
                    self.popup_style("edit-dialog", 440.)
                        .child(self.header(
                            "Edit Session",
                            "Drafts stay intact after dismissal or failure.",
                        ))
                        .child(
                            div()
                                .p(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child("Session name")
                                .child(
                                    div()
                                        .id("edit-input")
                                        .test_support()
                                        .h(px(32.))
                                        .border_1()
                                        .border_color(rgb(self.p.border))
                                        .bg(rgb(self.p.canvas))
                                        .child(Input::new(&self.input)),
                                )
                                .child("Notes")
                                .child(
                                    div()
                                        .id("edit-textarea")
                                        .test_support()
                                        .h(px(90.))
                                        .border_1()
                                        .border_color(rgb(self.p.border))
                                        .bg(rgb(self.p.canvas))
                                        .child(Textarea::new(&self.textarea)),
                                )
                                .when(self.error, |d| {
                                    d.child(
                                        div()
                                            .id("edit-error")
                                            .test_support()
                                            .text_color(rgb(self.p.danger))
                                            .child("Could not save the session. Try again."),
                                    )
                                })
                                .when(self.busy, |d| {
                                    d.child(div().id("edit-busy").test_support().child("Saving…"))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(7.))
                                .p(px(12.))
                                .border_t_1()
                                .border_color(rgb(self.p.border))
                                .child(self.action(
                                    "nested-open",
                                    "Details…",
                                    self.busy || self.layer == Layer::Nested,
                                    &self.nested_open_focus,
                                    |s, w, c| s.open(Layer::Nested, w, c),
                                    cx,
                                ))
                                .child(self.action(
                                    "edit-cancel",
                                    "Cancel",
                                    self.busy,
                                    &self.edit_cancel_focus,
                                    |s, w, c| s.close(w, c),
                                    cx,
                                ))
                                .child(self.action(
                                    "edit-save",
                                    "Retry / Save",
                                    self.busy,
                                    &self.edit_save_focus,
                                    |s, _, c| s.submit(c),
                                    cx,
                                ))
                                .when(self.busy, |d| {
                                    d.child(self.action(
                                        "fixture-finish",
                                        "Simulate Failure",
                                        false,
                                        &self.finish_focus,
                                        |s, w, c| s.finish(w, c),
                                        cx,
                                    ))
                                }),
                        ),
                ),
            )
    }
    fn quit_dialog(&self, cx: &mut Context<Self>) -> AlertDialog {
        let weak = cx.entity().downgrade();
        let weak_ok = weak.clone();
        AlertDialog::new(cx)
            .focus_handle(self.quit_focus.clone())
            .close_on_escape(!self.busy)
            .on_cancel(move |_, window, cx| {
                weak.update(cx, |s, cx| s.close(window, cx)).ok();
                false
            })
            .on_ok(move |_, _, cx| {
                weak_ok.update(cx, |s, cx| s.submit(cx)).ok();
                false
            })
            .backdrop(self.backdrop())
            .popup(
                DialogPopup::new().child(
                    self.popup_style("quit-dialog", 348.)
                        .child(self.header("Quit Tau?", "Two sessions are still in progress."))
                        .when(self.error, |d| {
                            d.child(
                                div()
                                    .id("quit-error")
                                    .test_support()
                                    .p(px(12.))
                                    .text_color(rgb(self.p.danger))
                                    .child("Could not quit Tau. Try again."),
                            )
                        })
                        .when(self.busy, |d| {
                            d.child(
                                div()
                                    .id("quit-busy")
                                    .test_support()
                                    .p(px(12.))
                                    .child("Quitting…"),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(7.))
                                .p(px(12.))
                                .child(self.action(
                                    "quit-cancel",
                                    "Cancel",
                                    self.busy,
                                    &self.quit_cancel_focus,
                                    |s, w, c| s.close(w, c),
                                    cx,
                                ))
                                .child(self.action(
                                    "quit-confirm",
                                    "Quit",
                                    self.busy,
                                    &self.quit_confirm_focus,
                                    |s, _, c| s.submit(c),
                                    cx,
                                ))
                                .when(self.busy, |d| {
                                    d.child(self.action(
                                        "fixture-finish",
                                        "Simulate Failure",
                                        false,
                                        &self.quit_finish_focus,
                                        |s, w, c| s.finish(w, c),
                                        cx,
                                    ))
                                }),
                        ),
                ),
            )
    }
}
impl Render for DialogsProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.p;
        let mut page = common::page(p)
            .id("dialogs-page")
            .child(div().text_size(px(18.)).child("Dialog surfaces"))
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("Isolated native interactions · no session or runtime"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.action(
                        "open-edit",
                        "Edit Session",
                        self.layer != Layer::None,
                        &self.trigger,
                        |s, w, c| s.open(Layer::Edit, w, c),
                        cx,
                    ))
                    .child(self.action(
                        "open-quit",
                        "Quit Tau",
                        self.layer != Layer::None,
                        &self.quit_trigger,
                        |s, w, c| s.open(Layer::Quit, w, c),
                        cx,
                    ))
                    .child(self.action(
                        "open-inline",
                        "Extension Prompt",
                        self.layer != Layer::None,
                        &self.inline_focus,
                        |s, w, c| {
                            s.inline = !s.inline;
                            if s.inline {
                                s.inline_input.read(c).focus_handle(c).focus(w, c);
                            } else {
                                s.inline_focus.focus(w, c);
                            }
                            c.notify();
                        },
                        cx,
                    )),
            )
            .child(
                div()
                    .text_color(rgb(p.muted))
                    .child("The extension prompt below is inline, not a modal."),
            );
        if self.inline {
            page = page.child(
                div()
                    .id("inline-prompt")
                    .test_support()
                    .w(px(440.))
                    .p(px(12.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.surface))
                    .on_key_down(cx.listener(|s, event: &gpui_kit::KeyDownEvent, w, c| {
                        if event.keystroke.key == "escape" {
                            s.inline = false;
                            s.inline_focus.focus(w, c);
                            c.notify();
                            c.stop_propagation();
                        }
                    }))
                    .child("Extension input requested")
                    .child(
                        div()
                            .id("inline-input")
                            .test_support()
                            .h(px(32.))
                            .bg(rgb(p.canvas))
                            .child(Input::new(&self.inline_input)),
                    )
                    .child(self.action(
                        "inline-cancel",
                        "Cancel Prompt",
                        false,
                        &self.inline_cancel_focus,
                        |s, w, c| {
                            s.inline = false;
                            s.inline_focus.focus(w, c);
                            c.notify();
                        },
                        cx,
                    )),
            );
        }
        if self.layer == Layer::Edit || self.layer == Layer::Nested {
            page = page.child(self.edit_dialog(cx));
        }
        if self.layer == Layer::Quit {
            page = page.child(self.quit_dialog(cx));
        }
        if self.layer == Layer::Nested {
            let weak = cx.entity().downgrade();
            page = page.child(
                Dialog::new(cx)
                    .layer(1, true)
                    .focus_handle(self.nested_focus.clone())
                    .on_cancel(move |_, w, cx| {
                        weak.update(cx, |s, c| s.close(w, c)).ok();
                        false
                    })
                    .backdrop(self.backdrop())
                    .popup(
                        DialogPopup::new().child(
                            self.popup_style("nested-dialog", 300.)
                                .child(self.header("Details", "Escape closes only this layer."))
                                .child(div().p(px(12.)).child(self.action(
                                    "nested-close",
                                    "Done",
                                    false,
                                    &self.nested_close_focus,
                                    |s, w, c| s.close(w, c),
                                    cx,
                                ))),
                        ),
                    ),
            );
        }
        let _ = window;
        page
    }
}
impl Probe for DialogsProbe {
    const ID: &'static str = "dialogs";
    const TITLE: &'static str = "Tau · Dialogs";
    const DESCRIPTION: &'static str = "Base modal, alert, nested layer and inline extension prompt";
    const LIMITATIONS: &'static str = "Local synthetic save/quit failure; no OS quit, Pi, persistence, screen reader or production wiring.";
    fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let textarea = cx.new(|cx| TextareaState::new(window, cx));
        let inline_input = cx.new(|cx| InputState::new(window, cx));
        let p = Palette::new(dark);
        input.update(cx, |s, _| s.set_editor_style(common::text_style(p)));
        textarea.update(cx, |s, _| s.set_editor_style(common::text_style(p)));
        inline_input.update(cx, |s, _| s.set_editor_style(common::text_style(p)));
        let notes_focus = textarea.read(cx).focus_handle(cx).clone();
        let inline_input_focus = inline_input.read(cx).focus_handle(cx).clone();
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(scene) = weak.upgrade() else { return };
            let state = scene.read(cx);
            if state.layer == Layer::Edit && notes_focus.is_focused(window) {
                if event.keystroke.key == "tab"
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform
                    && !event.keystroke.modifiers.function
                {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            } else if state.inline
                && inline_input_focus.is_focused(window)
                && event.keystroke.key == "escape"
            {
                scene.update(cx, |s, cx| {
                    s.inline = false;
                    s.inline_focus.focus(window, cx);
                    cx.notify();
                });
                cx.stop_propagation();
            }
        });
        Self {
            p,
            layer: Layer::None,
            inline: false,
            busy: false,
            error: false,
            attempts: 0,
            trigger: cx.focus_handle(),
            quit_trigger: cx.focus_handle(),
            edit_focus: cx.focus_handle(),
            quit_focus: cx.focus_handle(),
            nested_focus: cx.focus_handle(),
            nested_open_focus: cx.focus_handle(),
            edit_cancel_focus: cx.focus_handle(),
            edit_save_focus: cx.focus_handle(),
            finish_focus: cx.focus_handle(),
            quit_finish_focus: cx.focus_handle(),
            quit_cancel_focus: cx.focus_handle(),
            quit_confirm_focus: cx.focus_handle(),
            nested_close_focus: cx.focus_handle(),
            inline_cancel_focus: cx.focus_handle(),
            inline_focus: cx.focus_handle(),
            input,
            inline_input,
            textarea,
            _keys: keys,
        }
    }
    fn steps() -> Vec<Step> {
        use Step::*;
        vec![
            Check("initial"),
            Click("open-edit"),
            Check("edit-open"),
            Click("edit-input"),
            Type("Keep this draft"),
            Click("edit-textarea"),
            Type("Retained notes"),
            Check("drafts"),
            Click("edit-input"),
            Check("before-tab"),
            Key("tab"),
            Check("tab-contained"),
            Key("tab"),
            Check("textarea-tab"),
            Key("shift-tab"),
            Check("reverse-tab-contained"),
            Key("shift-tab"),
            Check("input-reverse"),
            Click("nested-open"),
            Check("nested-open"),
            Key("tab"),
            Check("nested-cycle"),
            Key("shift-tab"),
            Check("nested-reverse"),
            Key("escape"),
            Check("nested-closed"),
            Key("tab"),
            Check("button-tab"),
            Key("tab"),
            Check("save-tab"),
            Key("tab"),
            Check("wrap-tab"),
            Key("shift-tab"),
            Check("wrap-reverse"),
            Click("edit-save"),
            Check("edit-busy"),
            Key("escape"),
            Check("busy-escape"),
            Click("edit-save"),
            Check("busy-repeat"),
            Click("fixture-finish"),
            Check("edit-error"),
            Click("edit-save"),
            Check("edit-retry"),
            Click("fixture-finish"),
            Check("edit-retry-error"),
            Click("edit-cancel"),
            Check("edit-return"),
            Click("open-edit"),
            Check("drafts-return"),
            Key("escape"),
            Check("edit-escape"),
            Click("open-quit"),
            Check("quit-open"),
            Key("tab"),
            Check("quit-tab"),
            Key("tab"),
            Check("quit-wrap"),
            Key("shift-tab"),
            Check("quit-reverse"),
            Click("quit-confirm"),
            Check("quit-busy"),
            Key("escape"),
            Check("quit-busy-escape"),
            Click("fixture-finish"),
            Check("quit-error"),
            Key("escape"),
            Check("quit-return"),
            Click("open-inline"),
            Check("inline-open"),
            Key("escape"),
            Check("inline-escape"),
        ]
    }
    fn check(&self, name: &str, window: &Window, _cx: &App) -> Result<Value> {
        let focus = |f: &FocusHandle| f.is_focused(window);
        let draft = self.input.read(_cx).value().to_string();
        let notes = self.textarea.read(_cx).value().to_string();
        match name {
            "initial" => ensure!(
                self.layer == Layer::None && common::visible(window, "open-edit"),
                "initial triggers missing"
            ),
            "edit-open"
            | "before-tab"
            | "button-tab"
            | "save-tab"
            | "wrap-tab"
            | "wrap-reverse"
            | "input-reverse"
            | "textarea-tab"
            | "drafts"
            | "tab-contained"
            | "reverse-tab-contained"
            | "drafts-return" => {
                ensure!(
                    self.layer == Layer::Edit && common::visible(window, "edit-dialog"),
                    "edit modal missing"
                );
                if name != "edit-open" {
                    ensure!(
                        draft == "Keep this draft" && notes == "Retained notes",
                        "drafts lost"
                    );
                }
                let expected = match name {
                    "edit-open" | "before-tab" | "input-reverse" | "wrap-tab" | "drafts-return" => {
                        Some(self.input.read(_cx).focus_handle(_cx))
                    }
                    "tab-contained" | "reverse-tab-contained" => {
                        Some(self.textarea.read(_cx).focus_handle(_cx))
                    }
                    "textarea-tab" => Some(self.nested_open_focus.clone()),
                    "button-tab" => Some(self.edit_cancel_focus.clone()),
                    "save-tab" | "wrap-reverse" => Some(self.edit_save_focus.clone()),
                    _ => None,
                };
                if let Some(expected) = expected {
                    ensure!(
                        focus(&expected),
                        "{name}: wrong focus target: {:?}",
                        window.focused(_cx)
                    );
                }
            }
            "nested-open" => ensure!(
                self.layer == Layer::Nested
                    && common::visible(window, "nested-dialog")
                    && focus(&self.nested_close_focus),
                "nested focus/layer missing"
            ),
            "nested-cycle" | "nested-reverse" => ensure!(
                self.layer == Layer::Nested && focus(&self.nested_close_focus),
                "nested focus escaped topmost layer"
            ),
            "nested-closed" => ensure!(
                self.layer == Layer::Edit && focus(&self.nested_open_focus),
                "nested Escape did not return to parent"
            ),
            "edit-busy" | "busy-escape" | "busy-repeat" => ensure!(
                self.layer == Layer::Edit
                    && self.busy
                    && self.attempts == 1
                    && focus(&self.edit_save_focus)
                    && common::visible(window, "edit-busy"),
                "edit busy must veto dismissal/repeat"
            ),
            "edit-error" => ensure!(
                self.error
                    && !self.busy
                    && self.attempts == 1
                    && common::visible(window, "edit-error")
                    && draft == "Keep this draft"
                    && focus(&self.edit_save_focus),
                "local error, focus or draft missing"
            ),
            "edit-retry" => ensure!(
                self.busy
                    && self.attempts == 2
                    && !self.error
                    && common::visible(window, "edit-busy"),
                "retry did not start"
            ),
            "edit-retry-error" => ensure!(
                self.error
                    && !self.busy
                    && self.attempts == 2
                    && common::visible(window, "edit-error")
                    && draft == "Keep this draft",
                "retry error or draft missing"
            ),
            "edit-return" | "edit-escape" => ensure!(
                self.layer == Layer::None && focus(&self.trigger),
                "edit focus not returned"
            ),
            "quit-open" => ensure!(
                self.layer == Layer::Quit
                    && common::visible(window, "quit-dialog")
                    && focus(&self.quit_cancel_focus),
                "alert missing focus"
            ),
            "quit-tab" | "quit-wrap" | "quit-reverse" => ensure!(
                self.layer == Layer::Quit
                    && focus(if name == "quit-wrap" {
                        &self.quit_cancel_focus
                    } else {
                        &self.quit_confirm_focus
                    }),
                "{name}: alert Tab order incorrect: {:?}",
                window.focused(_cx)
            ),
            "quit-busy" | "quit-busy-escape" => ensure!(
                self.layer == Layer::Quit
                    && self.busy
                    && self.attempts == 3
                    && focus(&self.quit_confirm_focus)
                    && common::visible(window, "quit-busy"),
                "quit busy dismissed"
            ),
            "quit-error" => ensure!(
                self.layer == Layer::Quit
                    && self.error
                    && focus(&self.quit_cancel_focus)
                    && common::visible(window, "quit-error"),
                "quit error missing"
            ),
            "quit-return" => ensure!(
                self.layer == Layer::None && focus(&self.quit_trigger),
                "quit focus not returned: layer={:?}, focus={:?}",
                self.layer,
                window.focused(_cx)
            ),
            "inline-open" => ensure!(
                self.inline
                    && self.layer == Layer::None
                    && common::visible(window, "inline-prompt")
                    && self
                        .inline_input
                        .read(_cx)
                        .focus_handle(_cx)
                        .is_focused(window),
                "inline prompt missing"
            ),
            "inline-escape" => ensure!(
                !self.inline && self.layer == Layer::None && focus(&self.inline_focus),
                "inline Escape did not cancel"
            ),
            _ => anyhow::bail!("unknown check: {name}"),
        }
        let geometry = if self.layer == Layer::Nested {
            Some(common::inside(
                window,
                "nested-dialog",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else if self.layer == Layer::Edit {
            Some(common::inside(
                window,
                "edit-dialog",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else if self.layer == Layer::Quit {
            Some(common::inside(
                window,
                "quit-dialog",
                Self::WIDTH as f32,
                Self::HEIGHT as f32,
            )?)
        } else {
            None
        };
        Ok(
            json!({"layer":format!("{:?}",self.layer),"busy":self.busy,"error":self.error,"attempts":self.attempts,"draft":draft,"notes":notes,"modal_bounds":geometry,"trigger_focused":focus(&self.trigger),"input_focused":self.input.read(_cx).focus_handle(_cx).is_focused(window),"edit_focused":focus(&self.edit_focus),"textarea_focused":self.textarea.read(_cx).focus_handle(_cx).is_focused(window),"focused_handle":format!("{:?}",window.focused(_cx))}),
        )
    }
}
