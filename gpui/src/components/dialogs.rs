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
    editor_style: gpui_kit::base::input::InputEditorStyle,
    props: DialogProps,
    dark: bool,
    embedded: bool,
    keyboard_submission: bool,
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
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        super::composer::retheme_input_style(&mut self.editor_style, self.dark, dark);
        self.dark = dark;
        self.input.update(cx, |editor, cx| {
            editor.set_editor_style(self.editor_style.clone());
            cx.notify();
        });
        cx.notify();
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.editor_style = style.clone();
        self.input.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
    }
    pub fn new(
        props: DialogProps,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .accessibility_label(props.title.clone())
                .placeholder(props.placeholder.clone())
        });
        input.update(cx, |state, _| {
            state.set_protect_ime_keys(true);
            state.set_editor_style(support::input_style(dark));
        });
        let _input_events =
            cx.subscribe_in(&input, window, |dialog, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && (dialog.props.kind == DialogKind::Ssh
                        || (dialog.props.kind == DialogKind::ExtensionInput
                            && dialog.keyboard_submission))
                    && !dialog.busy
                    && !dialog.readonly
                {
                    dialog.submit(cx);
                }
            });
        let action_focus = cx.focus_handle();
        if matches!(props.kind, DialogKind::Ssh | DialogKind::ExtensionInput) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        } else if matches!(
            props.kind,
            DialogKind::Feedback | DialogKind::Quit | DialogKind::ExtensionSelect
        ) {
            action_focus.focus(window, cx);
        }
        Self {
            editor_style: support::input_style(dark),
            props,
            dark,
            embedded: false,
            keyboard_submission: false,
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
    /// The caller owns the canvas; this host must neither consume flex space nor cover it.
    pub fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }
    /// Opt into inline keyboard routing and pending-response duplicate protection.
    pub fn keyboard_submission(mut self) -> Self {
        self.keyboard_submission = true;
        self
    }
    pub fn editor(&self) -> Entity<InputState> {
        self.input.clone()
    }
    pub fn focus(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        if !self.open || self.busy {
            return;
        }
        if matches!(
            self.props.kind,
            DialogKind::Ssh | DialogKind::ExtensionInput
        ) {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            self.action_focus.focus(window, cx);
        }
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
        let readonly = self.readonly
            || ((self.props.kind == DialogKind::Ssh || self.keyboard_submission) && self.busy);
        self.input
            .update(cx, |input, cx| input.set_readonly(readonly, cx));
    }
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.open && !self.busy {
            self.open = false;
            cx.emit(DialogEvent::Cancelled);
            cx.notify();
        }
    }
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.open || self.busy || self.readonly {
            return;
        }
        let value = if self.props.kind == DialogKind::ExtensionSelect {
            self.selected.clone()
        } else {
            self.input.read(cx).value().to_string()
        };
        cx.emit(DialogEvent::Submitted(value));
        if self.props.kind == DialogKind::Ssh || self.keyboard_submission {
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
                                .role(None)
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
        if kind == DialogKind::Ssh {
            // Only the panel may occlude the backdrop's mouse-down handler.
            let width = (viewport.width - px(36.)).max(px(0.)).min(px(440.));
            return DialogPopup::new()
                .absolute()
                .top(px(68.))
                .left((viewport.width - width) / 2.)
                .w(width)
                .child(panel);
        }
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
            .track_focus(&self.action_focus)
            .on_key_down(cx.listener(|s, event: &gpui_kit::KeyDownEvent, _, cx| {
                if !s.keyboard_submission || s.busy {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "escape" => {
                        s.cancel(cx);
                        cx.stop_propagation();
                    }
                    "up" | "down"
                        if s.props.kind == DialogKind::ExtensionSelect
                            && !s.props.options.is_empty() =>
                    {
                        s.cursor = if event.keystroke.key == "up" {
                            s.cursor.saturating_sub(1)
                        } else {
                            (s.cursor + 1).min(s.props.options.len() - 1)
                        };
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "enter" if s.props.kind == DialogKind::ExtensionSelect => {
                        if let Some(option) = s.props.options.get(s.cursor) {
                            s.selected = option.label.clone();
                            s.submit(cx);
                        }
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
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
                        .role(None)
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
        } else if self.embedded {
            support::root(self.dark)
                .absolute()
                .top_0()
                .left_0()
                .bg(rgba(0x00000000))
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
                        .close_on_backdrop_press(self.props.kind == DialogKind::Ssh && !self.busy)
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
    fn inline_input_enter_submits_once_and_escape_cancels_after_retry() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|w, cx| {
            NativeDialog::new(props(DialogKind::ExtensionInput), false, w, cx)
                .embedded()
                .keyboard_submission()
        });
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|dialog, w, cx| {
            dialog
                .input
                .update(cx, |input, cx| input.set_value("reply café", w, cx));
            cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                events.borrow_mut().push(event.clone())
            })
        });
        window.draw();
        window.simulate_keystroke("enter");
        window.simulate_keystroke("enter");
        assert_eq!(
            *received.borrow(),
            vec![DialogEvent::Submitted("reply café".into())]
        );
        window.update(|dialog, _, cx| dialog.set_busy(false, cx));
        window.draw();
        window.simulate_keystroke("escape");
        assert_eq!(received.borrow().last(), Some(&DialogEvent::Cancelled));
    }
    #[test]
    fn inline_select_owns_arrow_and_enter_routing() {
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window(|w, cx| {
            NativeDialog::new(props(DialogKind::ExtensionSelect), false, w, cx)
                .embedded()
                .keyboard_submission()
        });
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|_, _, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                events.borrow_mut().push(event.clone())
            })
        });
        window.draw();
        window.simulate_keystroke("down");
        window.simulate_keystroke("enter");
        assert_eq!(
            *received.borrow(),
            vec![DialogEvent::Submitted("Second".into())]
        );
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
    struct PointerHost {
        dialog: Entity<NativeDialog>,
        downs: usize,
        clicks: usize,
    }

    impl Render for PointerHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            support::root(false)
                .child(
                    div()
                        .id("underlay")
                        .size_full()
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(|s, _, _, _| s.downs += 1),
                        )
                        .on_click(cx.listener(|s, _, _, _| s.clicks += 1)),
                )
                .child(self.dialog.clone())
        }
    }

    struct Fixture {
        window: gpui_kit::TestAppWindow<gpui_kit::base::Root>,
        host: Entity<PointerHost>,
        dialog: Entity<NativeDialog>,
        events: Rc<RefCell<Vec<DialogEvent>>>,
        _subscription: Subscription,
        _app: gpui_kit::TestApp,
    }

    impl Fixture {
        fn new(kind: DialogKind) -> Self {
            let mut app = gpui_kit::TestApp::new();
            app.update(gpui_kit::init);
            let mut entities = None;
            let mut window = app.open_window(|window, cx| {
                let dialog =
                    cx.new(|cx| NativeDialog::new(props(kind), false, window, cx).embedded());
                let host = cx.new(|_| PointerHost {
                    dialog: dialog.clone(),
                    downs: 0,
                    clicks: 0,
                });
                entities = Some((host.clone(), dialog));
                gpui_kit::base::Root::new(host, window, cx)
            });
            let (host, dialog) = entities.unwrap();
            let events = Rc::new(RefCell::new(Vec::new()));
            let received = events.clone();
            let subscription = window.update(|_, _, cx| {
                dialog.update(cx, |_, cx| {
                    cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                        received.borrow_mut().push(event.clone());
                    })
                })
            });
            window.simulate_resize(gpui_kit::size(px(900.), px(700.)));
            window.update(|_, window, cx| window.bounds_changed(cx));
            window.draw();
            Self {
                _app: app,
                window,
                host,
                dialog,
                events,
                _subscription: subscription,
            }
        }

        fn open(&mut self) -> bool {
            self.window
                .update(|_, _, cx| self.dialog.read(cx).is_open())
        }

        fn busy(&mut self, busy: bool) {
            self.window
                .update(|_, _, cx| self.dialog.update(cx, |s, cx| s.set_busy(busy, cx)));
            self.window.draw();
        }

        fn assert_no_clickthrough(&mut self) {
            self.window.update(|_, _, cx| {
                let host = self.host.read(cx);
                assert_eq!((host.downs, host.clicks), (0, 0));
            });
        }
    }

    #[test]
    fn ssh_popup_bounds_follow_panel_on_resize() {
        use gpui_kit::{size, test::TestWindowExt};
        let mut f = Fixture::new(DialogKind::Ssh);
        for (width, height) in [(900., 700.), (620., 510.), (360., 400.)] {
            f.window.simulate_resize(size(px(width), px(height)));
            f.window.update(|_, window, cx| window.bounds_changed(cx));
            f.window.draw();
            let panel = f
                .window
                .update(|_, window, _| window.find("dialog").bounds());
            let expected_width = 440_f32.min(width - 36.);
            assert!((f32::from(panel.left()) - (width - expected_width) / 2.).abs() < 1.);
            assert!((f32::from(panel.size.width) - expected_width).abs() < 1.);
            assert_eq!(f32::from(panel.top()), 68.);
        }
    }

    #[test]
    fn ssh_idle_outside_dismisses_on_down_once_on_every_side() {
        use gpui_kit::{MouseButton, point, test::TestWindowExt};
        for side in 0..4 {
            let mut f = Fixture::new(DialogKind::Ssh);
            let b = f.window.update(|_, w, _| w.find("dialog").bounds());
            let outside = match side {
                0 => point(b.left() - px(1.), b.center().y),
                1 => point(b.right() + px(1.), b.center().y),
                2 => point(b.center().x, b.top() - px(1.)),
                _ => point(b.center().x, b.bottom() + px(1.)),
            };
            f.window.simulate_mouse_down(outside, MouseButton::Left);
            assert!(!f.open(), "side {side} must dismiss on down, not release");
            f.window.draw();
            f.window.simulate_mouse_up(outside, MouseButton::Left);
            f.window.draw();
            assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
            f.assert_no_clickthrough();
            // A stale callback must not emit a second cancellation.
            f.window
                .update(|_, _, cx| f.dialog.update(cx, |s, cx| s.cancel(cx)));
            assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
        }
    }

    #[test]
    fn ssh_inside_clicks_and_inside_to_outside_release_do_not_dismiss() {
        use gpui_kit::{MouseButton, point, test::TestWindowExt};
        let mut f = Fixture::new(DialogKind::Ssh);
        let b = f.window.update(|_, w, _| w.find("dialog").bounds());
        let field = f.window.update(|_, w, _| w.find("input").bounds().center());
        let header = point(b.center().x, b.top() + px(15.));
        for inside in [header, field] {
            f.window.simulate_click(inside, MouseButton::Left);
            assert!(f.open());
            f.window.simulate_mouse_down(inside, MouseButton::Left);
            f.window
                .simulate_mouse_move(point(b.left() - px(1.), inside.y));
            f.window
                .simulate_mouse_up(point(b.left() - px(1.), inside.y), MouseButton::Left);
            assert!(f.open());
        }
        f.window.simulate_click(field, MouseButton::Left);
        f.window.simulate_keystroke("x");
        f.window
            .update(|_, _, cx| assert_eq!(f.dialog.read(cx).value(cx), "x"));
        assert!(f.events.borrow().is_empty());
        f.assert_no_clickthrough();
        f.window.simulate_keystroke("escape");
        assert!(!f.open());
        assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
    }

    #[test]
    fn ssh_busy_outside_and_escape_stay_open_then_idle_cancel_works() {
        use gpui_kit::{MouseButton, point, test::TestWindowExt};
        for escape in [false, true] {
            let mut f = Fixture::new(DialogKind::Ssh);
            f.busy(true);
            let b = f.window.update(|_, w, _| w.find("dialog").bounds());
            for outside in [
                point(b.left() - px(1.), b.center().y),
                point(b.right() + px(1.), b.center().y),
                point(b.center().x, b.top() - px(1.)),
                point(b.center().x, b.bottom() + px(1.)),
            ] {
                f.window.simulate_click(outside, MouseButton::Left);
                assert!(f.open());
            }
            f.window.simulate_keystroke("escape");
            assert!(f.open());
            assert!(f.events.borrow().is_empty());
            f.assert_no_clickthrough();
            f.busy(false);
            if escape {
                f.window.simulate_keystroke("escape");
            } else {
                f.window
                    .simulate_click(point(b.left() - px(1.), b.center().y), MouseButton::Left);
            }
            assert!(!f.open());
            assert_eq!(*f.events.borrow(), vec![DialogEvent::Cancelled]);
            f.assert_no_clickthrough();
        }
    }

    #[test]
    fn feedback_and_quit_keep_their_outside_dismissal_policy() {
        use gpui_kit::{MouseButton, point};
        for kind in [DialogKind::Feedback, DialogKind::Quit] {
            let mut f = Fixture::new(kind);
            f.window
                .simulate_click(point(px(5.), px(5.)), MouseButton::Left);
            assert!(f.open());
            assert!(f.events.borrow().is_empty());
            f.assert_no_clickthrough();
        }
    }
    #[test]
    fn ordinary_dialog_editor_accessibility_focus_and_readonly_guard() {
        use gpui_kit::accesskit::{Action, ActionRequest, Role, TreeId};
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window =
            app.open_window(|w, cx| NativeDialog::new(props(DialogKind::Ssh), false, w, cx));
        window.activate_accessibility();
        for readonly in [false, true, false] {
            window.update(|s, w, cx| {
                s.input
                    .update(cx, |s, cx| s.set_value("host sentinel", w, cx));
                s.set_readonly(readonly, cx);
            });
            window.draw();
            let tree = window.accessibility_tree().unwrap();
            let fields: Vec<_> = tree
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::TextInput)
                .collect();
            assert_eq!(fields.len(), 1);
            let (id, n) = fields[0];
            assert_eq!(n.label(), Some("Title"));
            assert_eq!(n.is_read_only(), readonly);
            assert!(n.supports_action(Action::Focus));
            let id = *id;
            let bounds = n.bounds().unwrap();
            let dialog_id = tree
                .nodes
                .iter()
                .find(|(_, n)| n.role() == Role::Dialog)
                .unwrap()
                .0;
            let at = gpui_kit::point(
                px(((bounds.x0 + bounds.x1) / 2.) as f32),
                px(((bounds.y0 + bounds.y1) / 2.) as f32),
            );
            // Genuine pointer interaction first; then move away through the platform action.
            window.simulate_click(at, gpui_kit::MouseButton::Left);
            window.dispatch_accessibility_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: dialog_id,
                data: None,
            });
            window.update(|s, w, cx| {
                assert!(!s.input.read(cx).focus_handle(cx).is_focused(w));
                assert!(gpui_kit::base::active_focus_trap(w, cx).is_some());
            });
            window.dispatch_accessibility_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            });
            window.update(|s, w, cx| assert!(s.input.read(cx).focus_handle(cx).is_focused(w)));
            window.draw();
            assert_eq!(window.accessibility_tree().unwrap().focus, id);
            window.simulate_input("X");
            window.read(|s, cx| {
                let value = s.input.read(cx).value();
                if readonly {
                    assert_eq!(value.as_ref(), "host sentinel");
                } else {
                    assert_ne!(value.as_ref(), "host sentinel");
                    assert!(value.contains('X'));
                }
            });
            if readonly {
                window.simulate_keystroke("right");
                window.read(|s, cx| assert!(s.input.read(cx).selected_range().start > 0));
            }
            window.simulate_keystroke("backspace");
            window.read(|s, cx| assert_eq!(s.input.read(cx).value().as_ref(), "host sentinel"));
        }
        drop(window);
        app.update(|cx| cx.shutdown());
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
