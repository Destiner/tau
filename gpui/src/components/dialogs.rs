use super::support::{self, ButtonVariant, tokens};
use gpui_kit::{
    BoxShadow, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription,
    TestSupportExt, Window,
    base::{
        AlertDialog, Dialog, DialogBackdrop, DialogPopup, Input, InputBase,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb, rgba,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Ssh,
    Feedback,
    Quit,
    ExtensionInput,
    ExtensionSelect,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogEvent {
    Submitted(String),
    Cancelled,
}
pub struct DialogOption {
    pub id: String,
    pub label: String,
}
pub struct DialogProps {
    pub kind: DialogKind,
    pub title: String,
    pub description: String,
    pub placeholder: String,
    pub options: Vec<DialogOption>,
}

pub struct NativeDialog {
    props: DialogProps,
    dark: bool,
    open: bool,
    busy: bool,
    readonly: bool,
    selected: String,
    cursor: usize,
    input: Entity<InputState>,
    focus: FocusHandle,
    action_focus: FocusHandle,
    _input_events: Subscription,
}
impl gpui_kit::EventEmitter<DialogEvent> for NativeDialog {}
impl NativeDialog {
    pub fn new(
        props: DialogProps,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(props.placeholder.clone()));
        input.update(cx, |state, _| {
            state.set_editor_style(support::input_style(dark))
        });
        let _input_events =
            cx.subscribe_in(&input, window, |dialog, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && dialog.props.kind == DialogKind::Ssh
                    && !dialog.busy
                    && !dialog.readonly
                {
                    dialog.submit(cx);
                }
            });
        let action_focus = cx.focus_handle();
        if matches!(props.kind, DialogKind::Ssh | DialogKind::ExtensionInput) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        } else if matches!(props.kind, DialogKind::Feedback | DialogKind::Quit) {
            action_focus.focus(window, cx);
        }
        Self {
            props,
            dark,
            open: true,
            busy: false,
            readonly: false,
            selected: String::new(),
            cursor: 0,
            input,
            focus: cx.focus_handle(),
            action_focus,
            _input_events,
        }
    }
    pub fn editor(&self) -> Entity<InputState> {
        self.input.clone()
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }
    pub fn value(&self, cx: &gpui_kit::App) -> String {
        self.input.read(cx).value().to_string()
    }
    pub fn selected(&self) -> &str {
        &self.selected
    }
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.sync_input_readonly(cx);
        cx.notify();
    }
    pub fn set_readonly(&mut self, readonly: bool, cx: &mut Context<Self>) {
        self.readonly = readonly;
        self.sync_input_readonly(cx);
        cx.notify();
    }
    fn sync_input_readonly(&mut self, cx: &mut Context<Self>) {
        let readonly = self.readonly || (self.props.kind == DialogKind::Ssh && self.busy);
        self.input
            .update(cx, |input, cx| input.set_readonly(readonly, cx));
    }
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.open = false;
            cx.emit(DialogEvent::Cancelled);
            cx.notify();
        }
    }
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.readonly {
            return;
        }
        let value = if self.props.kind == DialogKind::ExtensionSelect {
            self.selected.clone()
        } else {
            self.input.read(cx).value().to_string()
        };
        cx.emit(DialogEvent::Submitted(value));
        if self.props.kind == DialogKind::Ssh {
            self.set_busy(true, cx);
        }
        cx.notify();
    }
    fn backdrop(&self) -> DialogBackdrop {
        DialogBackdrop::new().absolute().size_full().bg(rgba(
            if self.props.kind == DialogKind::Quit {
                if self.dark { 0x0000004a } else { 0x00000012 }
            } else {
                tokens(self.dark).scrim
            },
        ))
    }
    fn popup(
        &self,
        viewport: gpui_kit::Size<gpui_kit::Pixels>,
        cx: &mut Context<Self>,
    ) -> DialogPopup {
        let t = tokens(self.dark);
        let kind = self.props.kind;
        let quit = kind == DialogKind::Quit;
        let title = self.props.title.clone();
        let description = self.props.description.clone();
        let scene = cx.entity().downgrade();
        let action_scene = scene.clone();
        let dismiss_scene = scene.clone();
        let panel = div()
            .id("dialog")
            .test_support()
            .w(px(if quit { 348. } else { 440. }))
            .max_w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(if quit { 18. } else { 14. }),
                    rgba(if self.dark { 0x00000099 } else { 0x00000033 }).into(),
                )
                .blur_radius(px(if quit { 42. } else { 40. })),
            ])
            .overflow_hidden()
            .flex()
            .flex_col();
        let panel = if quit {
            panel
                .child(
                    div()
                        .px(px(20.))
                        .pt(px(18.))
                        .pb(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .line_height(px(15.))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(t.muted))
                                .child(description),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .px(px(16.))
                        .pb(px(14.))
                        .child(
                            support::button(
                                "cancel",
                                "Cancel",
                                ButtonVariant::Secondary,
                                self.dark,
                            )
                            .h(px(28.))
                            .px(px(11.))
                            .on_click(move |_, _, cx| {
                                dismiss_scene.update(cx, |s, cx| s.cancel(cx)).ok();
                            }),
                        )
                        .child(
                            support::button("confirm", "Quit", ButtonVariant::Primary, self.dark)
                                .h(px(28.))
                                .px(px(11.))
                                .track_focus(&self.action_focus)
                                .on_click(move |_, _, cx| {
                                    action_scene.update(cx, |s, cx| s.submit(cx)).ok();
                                }),
                        ),
                )
        } else {
            let panel = panel.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .line_height(px(15.))
                            .child(title),
                    )
                    .when(kind == DialogKind::Feedback, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(t.muted))
                                .child(description),
                        )
                    }),
            );
            if kind == DialogKind::Ssh {
                panel.child(
                    div().p(px(12.)).child(
                        div().id("input").test_support().w_full().child(
                            InputBase::new("field")
                                .w_full()
                                .h(px(28.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.accent))
                                .bg(rgb(t.canvas))
                                .px(px(8.))
                                // The system monospace baseline sits 1px lower than WebKit here.
                                .pb(px(2.))
                                .flex()
                                .items_center()
                                .font_family(support::mono())
                                .text_size(px(11.))
                                .text_color(rgb(if self.busy { t.muted } else { t.text }))
                                .child(Input::new(&self.input)),
                        ),
                    ),
                )
            } else {
                panel.child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(0.))
                        .px(px(12.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(rgb(t.border))
                        .child(
                            support::button_sm(
                                "reconnect",
                                "Reconnect",
                                ButtonVariant::Primary,
                                self.dark,
                            )
                            .track_focus(&self.action_focus)
                            .on_click(move |_, _, cx| {
                                action_scene.update(cx, |s, cx| s.submit(cx)).ok();
                            }),
                        )
                        .child(
                            support::button_sm(
                                "close",
                                "Close",
                                ButtonVariant::Secondary,
                                self.dark,
                            )
                            .on_click(move |_, _, cx| {
                                dismiss_scene.update(cx, |s, cx| s.cancel(cx)).ok();
                            }),
                        ),
                )
            }
        };
        DialogPopup::new()
            .absolute()
            .top_0()
            .left_0()
            .w(viewport.width)
            .h(viewport.height)
            .child(
                div()
                    .absolute()
                    .top(px(if quit { 112. } else { 68. }))
                    .left_0()
                    .right_0()
                    .px(px(18.))
                    .flex()
                    .justify_center()
                    .child(panel),
            )
    }
    fn inline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(self.dark);
        let select = self.props.kind == DialogKind::ExtensionSelect;
        let owner = cx.entity().downgrade();
        let submit = owner.clone();
        let cancel = owner.clone();
        let mut box_ = div()
            .id("extension")
            .test_support()
            .w_full()
            .px(px(10.))
            .py(px(9.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(gpui_kit::relative(1.62))
                    .child(self.props.title.clone()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(gpui_kit::relative(1.62))
                    .child(self.props.description.clone()),
            );
        if select {
            let mut options = div().flex().flex_col().min_h(px(32.));
            for (ix, label) in self.props.options.iter().enumerate() {
                let owner = owner.clone();
                let hover_owner = owner.clone();
                let choice = label.label.clone();
                options = options.child(
                    div()
                        .id(label.id.clone())
                        .test_support()
                        .w_full()
                        .relative()
                        .rounded(px(6.))
                        .px(px(8.))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .line_height(gpui_kit::relative(1.45))
                        .cursor_pointer()
                        .when(self.cursor == ix, |s| {
                            s.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top(px(3.))
                                    .bottom(px(3.))
                                    .w(px(2.))
                                    .bg(rgb(t.accent)),
                            )
                        })
                        .child(label.label.clone())
                        .on_hover(move |hover, _, cx| {
                            if *hover {
                                hover_owner
                                    .update(cx, |s, cx| {
                                        s.cursor = ix;
                                        cx.notify();
                                    })
                                    .ok();
                            }
                        })
                        .on_click(move |_, _, cx| {
                            owner
                                .update(cx, |s, cx| {
                                    s.selected = choice.clone();
                                    s.submit(cx);
                                })
                                .ok();
                        }),
                );
            }
            box_ = box_.child(options);
        } else {
            box_ = box_.child(
                div().id("input").test_support().w_full().child(
                    InputBase::new("field")
                        .w_full()
                        .h(px(28.))
                        .border_1()
                        .border_color(rgb(t.accent))
                        .bg(rgb(t.canvas))
                        .rounded(px(4.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .child(Input::new(&self.input)),
                ),
            );
        }
        box_.child(
            div()
                .flex()
                .gap(px(4.))
                .when(!select, |d| {
                    d.child(
                        support::button_sm("submit", "Submit", ButtonVariant::Primary, self.dark)
                            .on_click(move |_, _, cx| {
                                submit.update(cx, |s, cx| s.submit(cx)).ok();
                            }),
                    )
                })
                .child(
                    support::button_sm("cancel", "Cancel", ButtonVariant::Secondary, self.dark)
                        .on_click(move |_, _, cx| {
                            cancel.update(cx, |s, cx| s.cancel(cx)).ok();
                        }),
                ),
        )
    }
}
impl Render for NativeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inline = matches!(
            self.props.kind,
            DialogKind::ExtensionInput | DialogKind::ExtensionSelect
        );
        let mut page = if inline {
            div().w_full()
        } else {
            support::root(self.dark)
        };
        if inline {
            if self.open {
                page = page.child(self.inline(cx));
            }
        } else if self.open {
            let weak = cx.entity().downgrade();
            let viewport = window.viewport_size();
            if self.props.kind == DialogKind::Quit {
                page = page.child(
                    AlertDialog::new(cx)
                        .focus_handle(self.focus.clone())
                        .on_cancel(move |_, _, cx| {
                            weak.update(cx, |s, cx| s.cancel(cx)).ok();
                            false
                        })
                        .backdrop(self.backdrop())
                        .popup(self.popup(viewport, cx)),
                );
            } else {
                page = page.child(
                    Dialog::new(cx)
                        .focus_handle(self.focus.clone())
                        .close_on_escape(!self.busy)
                        .close_on_backdrop_press(false)
                        .on_cancel(move |_, _, cx| {
                            weak.update(cx, |s, cx| s.cancel(cx)).ok();
                            false
                        })
                        .backdrop(self.backdrop())
                        .popup(self.popup(viewport, cx)),
                );
            }
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    fn props(kind: DialogKind) -> DialogProps {
        DialogProps {
            kind,
            title: "Title".into(),
            description: "Description".into(),
            placeholder: "Type here".into(),
            options: vec![
                DialogOption {
                    id: "first".into(),
                    label: "First".into(),
                },
                DialogOption {
                    id: "second".into(),
                    label: "Second".into(),
                },
            ],
        }
    }
    #[test]
    fn ssh_busy_and_readonly_block_cancel_and_duplicate_submit() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app
            .open_window(|window, cx| NativeDialog::new(props(DialogKind::Ssh), false, window, cx));
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|dialog, window, cx| {
            dialog
                .input
                .update(cx, |input, cx| input.set_value("ssh host", window, cx));
            cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.update(|dialog, _, cx| {
            dialog.submit(cx);
            assert!(dialog.is_busy() && !dialog.is_readonly());
            assert!(dialog.input.read(cx).presentation().is_readonly());
            dialog.cancel(cx);
            assert!(dialog.is_open());
            dialog.submit(cx);
        });
        assert_eq!(
            *received.borrow(),
            vec![DialogEvent::Submitted("ssh host".into())]
        );
        window.draw();
        window.simulate_keystroke("escape");
        assert!(window.read(|dialog, _| dialog.is_open()));
        window.update(|dialog, window, cx| {
            dialog.set_busy(false, cx);
            assert_eq!(dialog.value(cx), "ssh host");
            assert!(!dialog.input.read(cx).presentation().is_readonly());
            dialog
                .input
                .update(cx, |input, cx| input.set_value("ssh retry", window, cx));
            dialog.submit(cx);
            dialog.submit(cx);
        });
        assert_eq!(
            *received.borrow(),
            vec![
                DialogEvent::Submitted("ssh host".into()),
                DialogEvent::Submitted("ssh retry".into())
            ]
        );
        window.update(|dialog, _, cx| {
            dialog.set_busy(false, cx);
            dialog.cancel(cx);
            assert!(!dialog.is_open());
        });
        assert_eq!(received.borrow().last(), Some(&DialogEvent::Cancelled));
    }
    #[test]
    fn caller_readonly_survives_busy_transition() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app
            .open_window(|window, cx| NativeDialog::new(props(DialogKind::Ssh), false, window, cx));
        window.update(|dialog, _, cx| {
            dialog.set_readonly(true, cx);
            dialog.set_busy(true, cx);
            dialog.set_busy(false, cx);
            assert!(dialog.is_readonly());
            assert!(dialog.input.read(cx).presentation().is_readonly());
            dialog.set_readonly(false, cx);
            assert!(!dialog.input.read(cx).presentation().is_readonly());
        });
    }
    #[test]
    fn popup_centers_on_resize_and_backdrop_respects_busy_state() {
        use gpui_kit::{MouseButton, point, size, test::TestWindowExt};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app
            .open_window(|window, cx| NativeDialog::new(props(DialogKind::Ssh), false, window, cx));
        for (width, height) in [(900., 700.), (620., 510.)] {
            window.simulate_resize(size(px(width), px(height)));
            window.update(|_, window, cx| window.bounds_changed(cx));
            window.draw();
            let bounds = window.update(|_, window, _| window.find("dialog").bounds());
            assert!(
                (f32::from(bounds.left()) - (width - 440.) / 2.).abs() < 1.,
                "{width}x{height}: {bounds:?}"
            );
            assert_eq!(f32::from(bounds.top()), 68.);
            window.simulate_click(point(px(5.), px(5.)), MouseButton::Left);
            assert!(window.read(|dialog, _| dialog.is_open()));
        }
        window.update(|dialog, _, cx| dialog.set_busy(true, cx));
        window.draw();
        window.simulate_keystroke("escape");
        assert!(window.read(|dialog, _| dialog.is_open()));
    }
    #[test]
    fn extension_select_exposes_value_without_ssh_busy_state() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|window, cx| {
            NativeDialog::new(props(DialogKind::ExtensionSelect), false, window, cx)
        });
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        window.update(|dialog, _, cx| {
            dialog.selected = "Second".into();
            dialog.submit(cx);
            assert!(!dialog.is_busy());
        });
        assert_eq!(
            *received.borrow(),
            vec![DialogEvent::Submitted("Second".into())]
        );
    }
}
