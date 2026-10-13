use super::{
    markdown::{self, MarkdownPresentation},
    menus::{Menu, MenuEvent, MenuItem},
    support::{self, ButtonVariant, tokens},
};
use std::{cell::Cell, ops::Range, rc::Rc};
gpui_kit::actions!(extension_dialog, [NextControl, PreviousControl]);
use gpui_kit::{
    BoxShadow, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription,
    TestSupportExt, Window,
    base::{
        AlertDialog, Button, Dialog, DialogBackdrop, DialogPopup, ElementExt, Input, InputBase,
        TextViewState,
        input::{Copy, Cut, InputEvent, InputState, Paste, Textarea, TextareaState},
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

type LinkCallback = Rc<dyn Fn(&str, &mut Window, &mut gpui_kit::App)>;
type OptionRevealCallback =
    Rc<dyn Fn(gpui_kit::Bounds<gpui_kit::Pixels>, &mut Window, &mut gpui_kit::App)>;
type TargetLinkCallback = Rc<
    dyn Fn(
        markdown::LinkTarget,
        Option<FocusHandle>,
        gpui_kit::Point<gpui_kit::Pixels>,
        &mut Window,
        &mut gpui_kit::App,
    ),
>;
type ContextMenuHook = Rc<
    dyn Fn(
        gpui_kit::base::input::NativeMenu,
        gpui_kit::base::input::InputContextMenuCapabilities,
        gpui_kit::Point<gpui_kit::Pixels>,
        &mut Window,
        &mut gpui_kit::App,
    ),
>;

pub struct NativeDialog {
    editor_style: gpui_kit::base::input::InputEditorStyle,
    props: DialogProps,
    editor_mode: bool,
    dark: bool,
    embedded: bool,
    keyboard_submission: bool,
    confirm_shortcuts: bool,
    confirm_focus_pending: bool,
    open: bool,
    busy: bool,
    readonly: bool,
    selected: String,
    cursor: usize,
    option_focus: Vec<FocusHandle>,
    content_scroll: gpui_kit::ScrollHandle,
    reveal_cursor: Rc<Cell<bool>>,
    reveal_option: Option<OptionRevealCallback>,
    input: Entity<InputState>,
    multiline: Entity<TextareaState>,
    title: Entity<MarkdownPresentation>,
    message: Entity<MarkdownPresentation>,
    on_link: LinkCallback,
    on_target_link: Option<TargetLinkCallback>,
    error: String,
    action_label: String,
    dismissible: bool,
    accepted: Option<DialogEvent>,
    context_menu: Entity<Menu>,
    menu_target: Option<(String, Range<usize>)>,
    _menu_events: Subscription,
    focus: FocusHandle,
    action_focus: FocusHandle,
    cancel_focus: FocusHandle,
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
        self.multiline.update(cx, |editor, cx| {
            editor.set_editor_style(self.editor_style.clone());
            cx.notify();
        });
        self.title.update(cx, |p, cx| p.set_theme(dark, cx));
        self.message.update(cx, |p, cx| p.set_theme(dark, cx));
        self.context_menu
            .update(cx, |menu, cx| menu.set_theme(dark, _window, cx));
        cx.notify();
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.editor_style = style.clone();
        self.input.update(cx, |editor, cx| {
            editor.set_editor_style(style.clone());
            cx.notify();
        });
        self.multiline.update(cx, |editor, cx| {
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
        Self::new_with_mode(props, dark, false, window, cx)
    }
    /// Construct a multiline extension input using the actual host window for native focus.
    pub fn new_editor(
        mut props: DialogProps,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        props.kind = DialogKind::ExtensionInput;
        Self::new_with_mode(props, dark, true, window, cx)
    }
    fn new_with_mode(
        props: DialogProps,
        dark: bool,
        editor_mode: bool,
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
                            && !dialog.editor_mode
                            && dialog.keyboard_submission))
                    && !dialog.busy
                    && !dialog.readonly
                {
                    dialog.submit(cx);
                }
            });
        let multiline = cx.new(|cx| {
            TextareaState::new(window, cx)
                .accessibility_label(props.title.clone())
                .rows(6)
                .submit_on_enter(false)
        });
        multiline.update(cx, |e, _| {
            e.set_protect_ime_keys(true);
            e.set_editor_style(support::input_style(dark));
            e.set_editor_paddings(gpui_kit::Edges {
                top: px(6.),
                right: px(8.),
                bottom: px(6.),
                left: px(8.),
            });
        });
        let title_text = cx.new(|cx| {
            TextViewState::markdown_with_extensions(&props.title, markdown::extensions(), cx)
        });
        let message_text = cx.new(|cx| {
            TextViewState::markdown_with_extensions(&props.description, markdown::extensions(), cx)
        });
        let owner = cx.entity().downgrade();
        let title_owner = owner.clone();
        let title = cx.new(|cx| {
            MarkdownPresentation::new(
                title_text,
                dark,
                move |cx| {
                    _ = title_owner.update(cx, |_, cx| cx.notify());
                },
                cx,
            )
        });
        let message_owner = owner.clone();
        let message = cx.new(|cx| {
            MarkdownPresentation::new(
                message_text,
                dark,
                move |cx| {
                    _ = message_owner.update(cx, |_, cx| cx.notify());
                },
                cx,
            )
        });
        let context_menu = cx.new(|cx| Menu::new_context(dark, vec![], cx));
        let hook: ContextMenuHook = Rc::new(move |_, capabilities, position, window, cx| {
            let owner = owner.clone();
            // Base calls this while leasing the editor; resolve its retained state later.
            window.defer(cx, move |window, cx| {
                _ = owner.update(cx, |dialog, cx| {
                    if !dialog.open {
                        return;
                    }
                    let (value, range, origin) = dialog.editing_target(cx);
                    dialog.menu_target = Some((value, range));
                    let items = [
                        (
                            "cut",
                            "Cut",
                            !capabilities.is_editable() || !capabilities.has_selection(),
                        ),
                        ("copy", "Copy", !capabilities.is_copyable()),
                        ("paste", "Paste", !capabilities.is_editable()),
                    ]
                    .into_iter()
                    .map(|(id, label, disabled)| MenuItem {
                        id: id.into(),
                        label: label.into(),
                        element_id: format!("dialog-{id}"),
                        shortcut: None,
                        disabled,
                    })
                    .collect();
                    dialog.context_menu.update(cx, |menu, cx| {
                        menu.set_items(items, cx);
                        menu.open_at(position, Some(origin), window, cx);
                    });
                });
            });
        });
        input.update(cx, |e, _| e.on_context_menu(hook.clone()));
        multiline.update(cx, |e, _| e.on_context_menu(hook));
        let _menu_events = cx.subscribe_in(
            &context_menu,
            window,
            |dialog, _, event: &MenuEvent, window, cx| {
                if let MenuEvent::Selected(id) = event {
                    if !dialog.open {
                        return;
                    }
                    let (value, range, focus) = dialog.editing_target(cx);
                    if dialog.menu_target.take() != Some((value, range)) {
                        return;
                    }
                    match id.as_str() {
                        "copy" => focus.dispatch_action(&Copy, window, cx),
                        "cut" if !dialog.busy && !dialog.readonly => {
                            focus.dispatch_action(&Cut, window, cx)
                        }
                        "paste" if !dialog.busy && !dialog.readonly => {
                            focus.dispatch_action(&Paste, window, cx)
                        }
                        _ => {}
                    }
                }
            },
        );
        cx.bind_keys([
            gpui_kit::KeyBinding::new("tab", NextControl, Some("NativeExtensionEditor > Input")),
            gpui_kit::KeyBinding::new(
                "shift-tab",
                PreviousControl,
                Some("NativeExtensionEditor > Input"),
            ),
        ]);
        let option_focus: Vec<_> = props.options.iter().map(|_| cx.focus_handle()).collect();
        let action_focus = cx.focus_handle();
        if props.kind == DialogKind::ExtensionSelect && !option_focus.is_empty() {
            option_focus[0].focus(window, cx);
        } else if editor_mode {
            multiline.read(cx).focus_handle(cx).focus(window, cx);
        } else if matches!(props.kind, DialogKind::Ssh | DialogKind::ExtensionInput) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        } else if matches!(
            props.kind,
            DialogKind::Feedback | DialogKind::Quit | DialogKind::ExtensionSelect
        ) {
            action_focus.focus(window, cx);
        }
        let action_label = if props.kind == DialogKind::Feedback {
            "Reconnect"
        } else {
            "Quit"
        }
        .into();
        Self {
            option_focus,
            content_scroll: gpui_kit::ScrollHandle::new(),
            reveal_cursor: Rc::new(Cell::new(false)),
            reveal_option: None,
            editor_style: support::input_style(dark),
            props,
            editor_mode,
            dark,
            embedded: false,
            keyboard_submission: false,
            confirm_shortcuts: false,
            confirm_focus_pending: false,
            open: true,
            busy: false,
            readonly: false,
            selected: String::new(),
            cursor: 0,
            input,
            multiline,
            title,
            message,
            on_link: Rc::new(|_, _, _| {}),
            on_target_link: None,
            error: String::new(),
            action_label,
            dismissible: true,
            accepted: None,
            context_menu,
            menu_target: None,
            _menu_events,
            focus: cx.focus_handle(),
            action_focus,
            cancel_focus: cx.focus_handle(),
            _input_events,
        }
    }
    /// The caller owns the canvas; this host must neither consume flex space nor cover it.
    pub fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }
    /// The persistent transcript owns scrolling; only keyboard option navigation requests reveal.
    pub fn set_option_reveal_handler(
        &mut self,
        handler: impl Fn(gpui_kit::Bounds<gpui_kit::Pixels>, &mut Window, &mut gpui_kit::App) + 'static,
    ) {
        self.reveal_option = Some(Rc::new(handler));
    }
    pub fn clear_option_reveal_handler(&mut self) {
        self.reveal_option = None;
    }
    /// Opt into inline keyboard routing and pending-response duplicate protection.
    pub fn keyboard_submission(mut self) -> Self {
        self.keyboard_submission = true;
        self
    }
    /// Confirm modifiers express Yes/No, independently of select navigation.
    pub fn confirm_shortcuts(mut self) -> Self {
        self.confirm_shortcuts = true;
        self.confirm_focus_pending = true;
        self
    }
    pub fn editor(&self) -> Entity<InputState> {
        self.input.clone()
    }
    pub fn focus(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        if !self.open || self.busy || self.readonly {
            return;
        }
        if self.editor_mode {
            self.multiline.read(cx).focus_handle(cx).focus(window, cx);
        } else if matches!(
            self.props.kind,
            DialogKind::Ssh | DialogKind::ExtensionInput
        ) {
            self.input.read(cx).focus_handle(cx).focus(window, cx);
        } else if self.props.kind == DialogKind::ExtensionSelect
            && !self.confirm_shortcuts
            && !self.option_focus.is_empty()
        {
            self.option_focus[self.cursor].focus(window, cx);
        } else {
            self.action_focus.focus(window, cx);
        }
    }
    pub fn multiline_editor(&self) -> Entity<TextareaState> {
        self.multiline.clone()
    }
    pub fn set_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_mode {
            self.multiline
                .update(cx, |e, cx| e.set_value(value, window, cx));
        } else {
            self.input
                .update(cx, |e, cx| e.set_value(value, window, cx));
        }
    }
    pub fn set_error(&mut self, error: impl Into<String>, cx: &mut Context<Self>) {
        let error = error.into();
        if self.error != error {
            self.error = error;
            cx.notify();
        }
    }
    pub fn set_action_label(&mut self, label: impl Into<String>, cx: &mut Context<Self>) {
        self.action_label = label.into();
        cx.notify();
    }
    pub fn set_target_link_handler(
        &mut self,
        handler: impl Fn(markdown::LinkTarget, Option<FocusHandle>, &mut Window, &mut gpui_kit::App)
        + 'static,
    ) {
        self.on_target_link = Some(Rc::new(move |target, origin, _, window, cx| {
            handler(target, origin, window, cx)
        }));
    }
    pub fn set_anchored_link_handler(
        &mut self,
        handler: impl Fn(
            markdown::LinkTarget,
            Option<FocusHandle>,
            gpui_kit::Point<gpui_kit::Pixels>,
            &mut Window,
            &mut gpui_kit::App,
        ) + 'static,
    ) {
        self.on_target_link = Some(Rc::new(handler));
    }
    pub fn set_context_handler(
        &mut self,
        handler: impl Fn(
            markdown::LinkTarget,
            &gpui_kit::MouseDownEvent,
            &mut Window,
            &mut gpui_kit::App,
        ) + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let handler = std::sync::Arc::new(handler);
        let title_handler = handler.clone();
        self.title.update(cx, |p, cx| {
            p.set_context_handler(
                move |target, event, window, cx| title_handler(target, event, window, cx),
                cx,
            )
        });
        self.message.update(cx, |p, cx| {
            p.set_context_handler(
                move |target, event, window, cx| handler(target, event, window, cx),
                cx,
            )
        });
    }
    pub fn set_copy_paths(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.title.update(cx, |p, cx| p.set_copy_paths(enabled, cx));
        self.message
            .update(cx, |p, cx| p.set_copy_paths(enabled, cx));
    }
    pub fn set_markdown_link_handler(
        &mut self,
        handler: impl Fn(&str, &mut Window, &mut gpui_kit::App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.on_link = Rc::new(handler);
        cx.notify();
    }
    pub fn set_dismissible(&mut self, dismissible: bool, cx: &mut Context<Self>) {
        self.dismissible = dismissible;
        cx.notify();
    }
    pub fn accepted_response(&self) -> Option<&str> {
        match &self.accepted {
            Some(DialogEvent::Submitted(value)) => Some(value),
            _ => None,
        }
    }
    pub fn accepted_event(&self) -> Option<&DialogEvent> {
        self.accepted.as_ref()
    }
    fn editing_target(&self, cx: &gpui_kit::App) -> (String, Range<usize>, FocusHandle) {
        if self.editor_mode {
            let e = self.multiline.read(cx);
            (
                e.value().to_string(),
                e.selected_range(),
                e.focus_handle(cx),
            )
        } else {
            let e = self.input.read(cx);
            (
                e.value().to_string(),
                e.selected_range(),
                e.focus_handle(cx),
            )
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
        if self.editor_mode {
            self.multiline.read(cx).value().to_string()
        } else {
            self.input.read(cx).value().to_string()
        }
    }
    pub fn selected(&self) -> &str {
        &self.selected
    }
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy == busy {
            return;
        }
        self.busy = busy;
        if busy {
            self.context_menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
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
        let extension = self.props.kind == DialogKind::ExtensionInput;
        self.input.update(cx, |input, cx| {
            input.set_readonly(readonly, cx);
            if extension {
                input.set_disabled(self.readonly || self.busy, cx);
            }
        });
        self.multiline.update(cx, |input, cx| {
            input.set_readonly(self.readonly || self.busy, cx);
            input.set_disabled(self.readonly || self.busy, cx);
        });
    }
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.open && !self.busy && !self.readonly && self.dismissible {
            let extension = matches!(
                self.props.kind,
                DialogKind::ExtensionInput | DialogKind::ExtensionSelect
            );
            if extension && self.keyboard_submission {
                self.accepted = Some(DialogEvent::Cancelled);
                self.set_busy(true, cx);
            } else {
                self.open = false;
                self.context_menu.update(cx, |menu, cx| menu.dismiss(cx));
            }
            cx.emit(DialogEvent::Cancelled);
            cx.notify();
        }
    }
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.open
            || self.busy
            || self.readonly
            || (self.props.kind == DialogKind::Feedback && self.action_label.is_empty())
            || (self.props.kind == DialogKind::ExtensionSelect
                && !self.confirm_shortcuts
                && self.props.options.is_empty())
        {
            return;
        }
        let value = if self.props.kind == DialogKind::ExtensionSelect {
            self.selected.clone()
        } else {
            self.value(cx)
        };
        if self.editor_mode {
            self.multiline
                .update(cx, |input, _| input.break_undo_transaction());
        } else if matches!(
            self.props.kind,
            DialogKind::Ssh | DialogKind::ExtensionInput
        ) {
            self.input
                .update(cx, |input, _| input.break_undo_transaction());
        }
        self.accepted = Some(DialogEvent::Submitted(value.clone()));
        if matches!(
            self.props.kind,
            DialogKind::Ssh | DialogKind::Quit | DialogKind::Feedback
        ) || self.keyboard_submission
        {
            self.set_busy(true, cx);
        }
        cx.emit(DialogEvent::Submitted(value));
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
            .border_color(if quit {
                rgba(if self.dark { 0x1b1f29bf } else { 0x6b7d8f2e })
            } else {
                rgb(t.border)
            })
            .bg(if quit {
                rgba((t.panel_raised << 8) | 0xf5)
            } else {
                rgb(t.panel_raised)
            })
            .on_key_down(cx.listener(|s, event: &gpui_kit::KeyDownEvent, _, cx| {
                if matches!(s.props.kind, DialogKind::Quit | DialogKind::Feedback)
                    && event.keystroke.key == "enter"
                    && event.keystroke.modifiers.secondary()
                {
                    s.submit(cx);
                    cx.stop_propagation();
                }
            }))
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
                                .child(gpui_kit::Text::new("dialog-title".into(), title.into())),
                        )
                        .when(!description.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(gpui_kit::relative(1.45))
                                    .text_color(rgb(t.muted))
                                    .child(gpui_kit::Text::new(
                                        "dialog-description".into(),
                                        description.into(),
                                    )),
                            )
                        })
                        .when(!self.error.is_empty(), |d| {
                            d.child(
                                div()
                                    .id("dialog-error")
                                    .test_support()
                                    .text_size(px(11.))
                                    .line_height(gpui_kit::relative(1.45))
                                    .text_color(rgb(t.danger))
                                    .child(gpui_kit::Text::new(
                                        "dialog-error-text".into(),
                                        self.error.clone().into(),
                                    )),
                            )
                        }),
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
                            .backdrop(t.panel_raised)
                            .disabled(self.busy || self.readonly)
                            .on_click(move |_, _, cx| {
                                dismiss_scene.update(cx, |s, cx| s.cancel(cx)).ok();
                            }),
                        )
                        .child(
                            support::button(
                                "confirm",
                                self.action_label.clone(),
                                ButtonVariant::Primary,
                                self.dark,
                            )
                            .h(px(28.))
                            .px(px(11.))
                            .backdrop(t.panel_raised)
                            .disabled(self.busy || self.readonly)
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
                            .child(gpui_kit::Text::new("dialog-title".into(), title.into())),
                    )
                    .when(kind == DialogKind::Feedback, |d| {
                        d.child(div().text_size(px(11.)).text_color(rgb(t.muted)).child(
                            gpui_kit::Text::new("dialog-description".into(), description.into()),
                        ))
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
                        .when(!self.action_label.is_empty(), |d| {
                            d.child(
                                support::button_sm(
                                    "reconnect",
                                    self.action_label.clone(),
                                    ButtonVariant::Primary,
                                    self.dark,
                                )
                                .backdrop(t.panel_raised)
                                .disabled(self.busy || self.readonly)
                                .track_focus(&self.action_focus)
                                .on_click(move |_, _, cx| {
                                    action_scene.update(cx, |s, cx| s.submit(cx)).ok();
                                }),
                            )
                        })
                        .when(self.dismissible, |d| {
                            d.child(
                                support::button_sm(
                                    "close",
                                    "Close",
                                    ButtonVariant::Secondary,
                                    self.dark,
                                )
                                .backdrop(t.panel_raised)
                                .disabled(self.busy || self.readonly)
                                .on_click(move |_, _, cx| {
                                    dismiss_scene.update(cx, |s, cx| s.cancel(cx)).ok();
                                }),
                            )
                        }),
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
    fn inline(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(self.dark);
        let select = self.props.kind == DialogKind::ExtensionSelect;
        let owner = cx.entity().downgrade();
        let submit = owner.clone();
        let cancel = owner.clone();
        let title_owner = owner.clone();
        let message_owner = owner.clone();
        let disabled = self.busy || self.readonly;
        let input_focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let multiline_focused = self.multiline.read(cx).focus_handle(cx).is_focused(window);
        let mut box_ = div()
            .id("extension")
            .test_support()
            .role(gpui_kit::accesskit::Role::Dialog)
            .aria_label(self.props.title.clone())
            .when(!self.confirm_shortcuts, |d| {
                d.track_focus(&self.action_focus)
            })
            .on_key_down(
                cx.listener(|s, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if !s.keyboard_submission || s.busy || s.readonly {
                        return;
                    }
                    match event.keystroke.key.as_str() {
                        "escape" => {
                            s.cancel(cx);
                            cx.stop_propagation();
                        }
                        "up" | "down"
                            if s.props.kind == DialogKind::ExtensionSelect
                                && !s.confirm_shortcuts
                                && !s.props.options.is_empty() =>
                        {
                            s.cursor = if event.keystroke.key == "up" {
                                (s.cursor + s.props.options.len() - 1) % s.props.options.len()
                            } else {
                                (s.cursor + 1) % s.props.options.len()
                            };
                            s.reveal_cursor.set(true);
                            s.option_focus[s.cursor].focus(window, cx);
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "enter"
                            if s.confirm_shortcuts
                                && s.props.kind == DialogKind::ExtensionSelect
                                && event.keystroke.modifiers.secondary() =>
                        {
                            s.selected = if event.keystroke.modifiers.shift {
                                "No"
                            } else {
                                "Yes"
                            }
                            .into();
                            s.submit(cx);
                            cx.stop_propagation();
                        }
                        "enter"
                            if s.props.kind == DialogKind::ExtensionSelect
                                && !s.confirm_shortcuts
                                && !s.cancel_focus.is_focused(window) =>
                        {
                            if let Some(option) = s.props.options.get(s.cursor) {
                                s.selected = option.label.clone();
                                s.submit(cx);
                            }
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }),
            )
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
            .font_family(super::font::family())
            .text_size(px(11.))
            .text_color(rgb(t.text))
            .line_height(gpui_kit::relative(1.45))
            .on_action(cx.listener(|s, _: &gpui_kit::base::input::Escape, _, cx| {
                if s.keyboard_submission {
                    s.cancel(cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &gpui_kit::base::input::Enter, _, cx| cx.stop_propagation());
        let content_wheel = self.content_scroll.clone();
        let mut content = div()
            .id("extension-content")
            .test_support()
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex_shrink_0()
            .when(!self.embedded, |content| {
                content
                    .max_h((window.viewport_size().height - px(240.)).max(px(120.)))
                    .overflow_y_scroll()
                    .track_scroll(&self.content_scroll)
                    .on_scroll_wheel(move |_, _, cx| {
                        if content_wheel.max_offset().y > px(0.) {
                            cx.stop_propagation();
                        }
                    })
            })
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .id("extension-title")
                    .test_support()
                    .w_full()
                    .flex_shrink_0()
                    .child(
                        MarkdownPresentation::decorate(&self.title, 13., self.dark, window, cx)
                            .scrollable(false)
                            .text_size(px(13.))
                            .line_height(gpui_kit::relative(1.62))
                            .on_link_activate(move |link, event, window, cx| {
                                let Some(target) = markdown::activation_target(link, event) else {
                                    return;
                                };
                                let Some(owner) = title_owner.upgrade() else {
                                    return;
                                };
                                let (title_target, title_link) = {
                                    let dialog = owner.read(cx);
                                    (dialog.on_target_link.clone(), dialog.on_link.clone())
                                };
                                if let Some(handler) = &title_target {
                                    handler(
                                        target,
                                        window.focused(cx),
                                        event.position(),
                                        window,
                                        cx,
                                    );
                                } else if let markdown::LinkTarget::Web(url) = target {
                                    title_link(&url, window, cx);
                                }
                            }),
                    ),
            )
            .when(!self.props.description.is_empty(), |d| {
                d.child(
                    div()
                        .id("extension-message")
                        .test_support()
                        .w_full()
                        .flex_shrink_0()
                        .child(
                            MarkdownPresentation::decorate(
                                &self.message,
                                13.,
                                self.dark,
                                window,
                                cx,
                            )
                            .scrollable(false)
                            .text_size(px(13.))
                            .line_height(gpui_kit::relative(1.62))
                            .style(
                                markdown::text_style_at_size(self.dark, 13., window)
                                    .with_foreground(rgb(t.muted).into()),
                            )
                            .on_link_activate(
                                move |link, event, window, cx| {
                                    let Some(target) = markdown::activation_target(link, event)
                                    else {
                                        return;
                                    };
                                    let Some(owner) = message_owner.upgrade() else {
                                        return;
                                    };
                                    let (message_target, message_link) = {
                                        let dialog = owner.read(cx);
                                        (dialog.on_target_link.clone(), dialog.on_link.clone())
                                    };
                                    if let Some(handler) = &message_target {
                                        handler(
                                            target,
                                            window.focused(cx),
                                            event.position(),
                                            window,
                                            cx,
                                        );
                                    } else if let markdown::LinkTarget::Web(url) = target {
                                        message_link(&url, window, cx);
                                    }
                                },
                            ),
                        ),
                )
            });
        if select && !self.confirm_shortcuts {
            let mut options = div()
                .id("extension-select-options")
                .test_support()
                .role(gpui_kit::accesskit::Role::ListBox)
                .flex()
                .flex_col()
                .flex_shrink_0()
                .min_h(px(32.));
            for (ix, label) in self.props.options.iter().enumerate() {
                let owner = owner.clone();
                let hover_owner = owner.clone();
                let choice = label.label.clone();
                let scroll = self.content_scroll.clone();
                let reveal = self.reveal_cursor.clone();
                let cursor = self.cursor == ix;
                let embedded = self.embedded;
                let reveal_option = self.reveal_option.clone();
                options = options.child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .flex_shrink_0()
                        // Observe the unpadded row, not a canvas shifted by Button's padding.
                        .on_prepaint(move |bounds, window, cx| {
                            if !cursor || !reveal.get() {
                                return;
                            }
                            if embedded {
                                reveal.set(false);
                                if let Some(handler) = &reveal_option {
                                    handler(bounds, window, cx);
                                }
                                return;
                            }
                            let viewport = scroll.bounds();
                            let oversized = bounds.size.height > viewport.size.height;
                            let delta = if oversized || bounds.top() < viewport.top() {
                                viewport.top() - bounds.top()
                            } else if bounds.bottom() > viewport.bottom() {
                                viewport.bottom() - bounds.bottom()
                            } else {
                                px(0.)
                            };
                            if delta == px(0.) || oversized {
                                reveal.set(false);
                            }
                            if delta != px(0.) {
                                let mut offset = scroll.offset();
                                offset.y += delta;
                                scroll.set_offset(offset);
                                window.request_animation_frame();
                            }
                        })
                        .child(
                            Button::new(label.id.clone())
                                .accessibility_label(label.label.clone())
                                .role(gpui_kit::accesskit::Role::ListBoxOption)
                                .selected(self.cursor == ix)
                                .aria_selected(self.cursor == ix)
                                .disabled(self.busy || self.readonly)
                                .track_focus(&self.option_focus[ix])
                                .w_full()
                                .min_w_0()
                                .flex_shrink_0()
                                .relative()
                                .rounded(px(6.))
                                .px(px(8.))
                                .py(px(5.))
                                .whitespace_normal()
                                .flex()
                                .justify_start()
                                .items_start()
                                .text_size(px(11.))
                                .line_height(gpui_kit::relative(1.45))
                                .cursor_default()
                                .text_color(rgb(t.text))
                                .opacity(if disabled { 0.45 } else { 1. })
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
                                .child(div().w_full().min_w_0().child(label.label.clone()))
                                .on_hover(move |hover, _, cx| {
                                    if *hover {
                                        hover_owner
                                            .update(cx, |s, cx| {
                                                if !s.busy && !s.readonly {
                                                    s.cursor = ix;
                                                    cx.notify();
                                                }
                                            })
                                            .ok();
                                    }
                                })
                                .on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |s, cx| {
                                            if !s.busy && !s.readonly {
                                                s.selected = choice.clone();
                                                s.submit(cx);
                                            }
                                        })
                                        .ok();
                                }),
                        ),
                );
            }
            if self.props.options.is_empty() {
                options = options.child(
                    div()
                        .id("empty-options")
                        .test_support()
                        .px(px(8.))
                        .py(px(5.))
                        .text_color(rgb(t.muted))
                        .child("No options available"),
                );
            }
            content = content.child(options);
        }
        box_ = box_.child(content);
        if self.editor_mode {
            box_ = box_.child(
                InputBase::new("multiline")
                    .role(None)
                    .key_context("NativeExtensionEditor")
                    .w_full()
                    .h(px(120.))
                    .max_h((window.viewport_size().height * 0.35).min(px(300.)))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgb(if multiline_focused && !disabled {
                        t.accent
                    } else {
                        t.border
                    }))
                    .when(!disabled && !multiline_focused, |d| {
                        d.hover(|d| d.border_color(rgb(t.hover_border)))
                    })
                    .bg(rgb(t.canvas))
                    .text_size(px(11.))
                    .line_height(gpui_kit::relative(1.45))
                    .text_color(rgb(t.text))
                    .opacity(if disabled { 0.45 } else { 1. })
                    .capture_action(cx.listener(
                        |s, action: &gpui_kit::base::input::Enter, window, cx| {
                            if !s.editor_mode || !action.secondary || !s.keyboard_submission {
                                return;
                            }
                            use gpui_kit::EntityInputHandler;
                            let marked = s
                                .multiline
                                .update(cx, |e, cx| e.marked_text_range(window, cx).is_some());
                            if !marked {
                                s.submit(cx);
                            }
                            cx.stop_propagation();
                        },
                    ))
                    .on_action(|_: &NextControl, window, cx| window.focus_next(cx))
                    .on_action(|_: &PreviousControl, window, cx| window.focus_prev(cx))
                    .child(Textarea::new(&self.multiline)),
            );
        } else if !select {
            box_ = box_.child(
                div().id("input").test_support().w_full().child(
                    InputBase::new("field")
                        .role(None)
                        .w_full()
                        .h(px(28.))
                        .border_1()
                        .border_color(rgb(if input_focused && !disabled {
                            t.accent
                        } else {
                            t.border
                        }))
                        .when(!disabled && !input_focused, |d| {
                            d.hover(|d| d.border_color(rgb(t.hover_border)))
                        })
                        .bg(rgb(t.canvas))
                        .rounded(px(4.))
                        .px(px(8.))
                        .text_color(rgb(t.text))
                        .opacity(if disabled { 0.45 } else { 1. })
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .child(Input::new(&self.input)),
                ),
            );
        }
        box_.when(!self.error.is_empty(), |d| {
            d.child(
                div()
                    .id("dialog-error")
                    .test_support()
                    .role(gpui_kit::accesskit::Role::Status)
                    .text_size(px(10.))
                    .text_color(rgb(t.danger))
                    .child(self.error.clone()),
            )
        })
        .child(
            div()
                .flex()
                .gap(px(4.))
                .when(self.confirm_shortcuts, |d| {
                    let yes = owner.clone();
                    let no = owner.clone();
                    d.child(
                        support::button_sm("confirm", "Confirm", ButtonVariant::Primary, self.dark)
                            .backdrop(t.panel_raised)
                            .disabled(self.busy || self.readonly)
                            .track_focus(&self.action_focus)
                            .on_click(move |_, _, cx| {
                                _ = yes.update(cx, |s, cx| {
                                    s.selected = "Yes".into();
                                    s.submit(cx);
                                });
                            }),
                    )
                    .child(
                        support::button_sm("reject", "No", ButtonVariant::Secondary, self.dark)
                            .backdrop(t.panel_raised)
                            .disabled(self.busy || self.readonly)
                            .on_click(move |_, _, cx| {
                                _ = no.update(cx, |s, cx| {
                                    s.selected = "No".into();
                                    s.submit(cx);
                                });
                            }),
                    )
                })
                .when(!select, |d| {
                    d.child(
                        support::button_sm("submit", "Submit", ButtonVariant::Primary, self.dark)
                            .backdrop(t.panel_raised)
                            .disabled(self.busy || self.readonly)
                            .on_click(move |_, _, cx| {
                                submit.update(cx, |s, cx| s.submit(cx)).ok();
                            }),
                    )
                })
                .child(
                    support::button_sm("cancel", "Cancel", ButtonVariant::Secondary, self.dark)
                        .backdrop(t.panel_raised)
                        .disabled(self.busy || self.readonly)
                        .track_focus(&self.cancel_focus)
                        .on_click(move |_, _, cx| {
                            cancel.update(cx, |s, cx| s.cancel(cx)).ok();
                        }),
                ),
        )
    }
}
impl Render for NativeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.confirm_focus_pending && self.open && !self.busy && !self.readonly {
            self.confirm_focus_pending = false;
            self.action_focus.focus(window, cx);
        }
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
                page = page.child(self.inline(window, cx));
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
                        .close_on_escape(!self.busy && self.dismissible)
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
        page.when(self.open, |page| page.child(self.context_menu.clone()))
    }
}

#[cfg(test)]
#[path = "dialogs_parity_tests.rs"]
mod parity_tests;

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
    fn native_editor_mode_is_explicit_and_inactive_input_events_cannot_submit() {
        for editor_mode in [false, true] {
            let mut app = gpui_kit::TestApp::new();
            app.update(gpui_kit::init);
            let mut window = app.open_window(|w, cx| {
                let mut p = props(DialogKind::ExtensionInput);
                p.placeholder = if editor_mode { "" } else { "Multiline editor" }.into();
                let mut dialog = if editor_mode {
                    NativeDialog::new_editor(p, false, w, cx)
                } else {
                    NativeDialog::new(p, false, w, cx)
                }
                .embedded()
                .keyboard_submission();
                dialog.set_value("", w, cx);
                dialog
            });
            window.update(|dialog, w, cx| {
                assert_eq!(dialog.editor_mode, editor_mode);
                assert_eq!(dialog.props.kind, DialogKind::ExtensionInput);
                assert_eq!(dialog.value(cx), "");
                assert_eq!(
                    dialog.multiline.read(cx).focus_handle(cx).is_focused(w),
                    editor_mode
                );
                assert_eq!(
                    dialog.input.read(cx).focus_handle(cx).is_focused(w),
                    !editor_mode
                );
                dialog.input.update(cx, |_, cx| {
                    cx.emit(InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    });
                });
            });
            app.run_until_parked();
            assert_eq!(window.read(|dialog, _| dialog.is_busy()), !editor_mode);
            assert_eq!(
                window.read(|dialog, _| dialog.accepted_event().is_some()),
                !editor_mode
            );
            window.update(|_, w, cx| w.blur(cx));
            app.run_until_parked();
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
    fn confirm_shortcuts_ignore_cursor_and_remain_busy_guarded() {
        for shift in [false, true] {
            let mut app = gpui_kit::TestApp::new();
            app.update(gpui_kit::init);
            let mut window = app.open_window(|w, cx| {
                let mut p = props(DialogKind::ExtensionSelect);
                p.options = vec![
                    DialogOption {
                        id: "yes".into(),
                        label: "Yes".into(),
                    },
                    DialogOption {
                        id: "no".into(),
                        label: "No".into(),
                    },
                ];
                NativeDialog::new(p, false, w, cx)
                    .embedded()
                    .keyboard_submission()
                    .confirm_shortcuts()
            });
            let received = Rc::new(RefCell::new(Vec::new()));
            let events = received.clone();
            let _subscription = window.update(|dialog, _, cx| {
                dialog.cursor = usize::from(!shift);
                cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                    events.borrow_mut().push(event.clone())
                })
            });
            window.draw();
            let key = match (cfg!(target_os = "macos"), shift) {
                (true, false) => "cmd-enter",
                (true, true) => "cmd-shift-enter",
                (false, false) => "ctrl-enter",
                (false, true) => "ctrl-shift-enter",
            };
            window.update(|dialog, _, cx| dialog.set_readonly(true, cx));
            window.draw();
            window.simulate_keystroke(key);
            assert!(received.borrow().is_empty());
            window.update(|dialog, _, cx| dialog.set_readonly(false, cx));
            window.draw();
            window.simulate_keystroke(key);
            window.simulate_keystroke(key);
            window.simulate_keystroke("escape");
            assert_eq!(
                *received.borrow(),
                vec![DialogEvent::Submitted(
                    if shift { "No" } else { "Yes" }.into()
                )]
            );
            assert!(window.read(|dialog, _| dialog.is_open() && dialog.is_busy()));
        }
    }

    #[test]
    fn inline_input_ime_composition_still_blocks_secondary_enter() {
        use gpui_kit::EntityInputHandler;
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
            dialog.input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "稍后", Some(2..2), w, cx);
            });
            cx.subscribe(&cx.entity(), move |_, _, event: &DialogEvent, _| {
                events.borrow_mut().push(event.clone())
            })
        });
        window.draw();
        let key = if cfg!(target_os = "macos") {
            "cmd-enter"
        } else {
            "ctrl-enter"
        };
        window.simulate_keystroke(key);
        assert!(received.borrow().is_empty());
        window.update(|dialog, w, cx| {
            dialog.input.update(cx, |input, cx| {
                input.replace_text_in_range(None, "稍后", w, cx)
            });
        });
        window.draw();
        window.simulate_keystroke(key);
        assert_eq!(
            *received.borrow(),
            vec![DialogEvent::Submitted("稍后".into())]
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
