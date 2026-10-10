use super::{
    menus::{Menu, MenuEvent, MenuItem},
    support,
};
use gpui_kit::{
    Anchor, Bounds, BoxShadow, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    MouseDownEvent, Pixels, Render, Subscription, TestSupportExt, Window,
    base::{
        Button as BaseButton, ElementExt, Input, InputBase, Placement, Popup, Positioner,
        ScrollbarStyles, Textarea, Theme, Tooltip,
        input::{Copy, Cut, InputEvent, InputState, Paste, TextareaState},
    },
    div,
    prelude::*,
    px, rgb, rgba,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub element_id: String,
}

#[derive(Clone, Debug)]
pub struct ComposerOptions {
    pub models: Vec<Choice>,
    pub model_heading: String,
    pub efforts: Vec<Choice>,
    pub model: String,
    pub effort: String,
    pub draft: String,
    pub dark: bool,
    pub disabled: bool,
    pub streaming: bool,
}

gpui_kit::actions!(native_composer, [NextChoice, PreviousChoice]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ModelChanged(String),
    EffortChanged(String),
    Send(String),
    FollowUp(String),
    Stop,
}

impl gpui_kit::EventEmitter<Event> for Composer {}

// Base exposes no projected-style getter; adapters must project overrides through the owner.
pub(super) fn retheme_input_style(
    style: &mut gpui_kit::base::input::InputEditorStyle,
    old_dark: bool,
    dark: bool,
) {
    let old = support::input_style(old_dark);
    let new = support::input_style(dark);
    for (current, before, after) in [
        (&mut style.foreground, old.foreground, new.foreground),
        (
            &mut style.muted_foreground,
            old.muted_foreground,
            new.muted_foreground,
        ),
        (&mut style.background, old.background, new.background),
        (&mut style.border, old.border, new.border),
        (&mut style.selection, old.selection, new.selection),
        (&mut style.caret, old.caret, new.caret),
    ] {
        if *current == before {
            *current = after;
        }
    }
}

pub struct Composer {
    hint_hovered: bool,
    hint_visible: bool,
    hint_task: Option<gpui_kit::Task<()>>,
    hint_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    editor_style: gpui_kit::base::input::InputEditorStyle,
    search_style: gpui_kit::base::input::InputEditorStyle,
    models: Vec<Choice>,
    model_heading: String,
    efforts: Vec<Choice>,
    dark: bool,
    editor: Entity<TextareaState>,
    editor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    streaming: bool,
    disabled: bool,
    stopped: bool,
    fill_height: bool,
    selector: Option<&'static str>,
    model: usize,
    effort: usize,
    cursor: usize,
    query: String,
    search: Entity<InputState>,
    effort_focus: FocusHandle,
    suppress_effort_click: bool,
    model_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    effort_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    context_menu: Entity<Menu>,
    menu_target: Option<(String, std::ops::Range<usize>)>,
    _menu_events: Subscription,
    _events: Subscription,
    _search_events: Subscription,
}
impl Composer {
    pub fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        retheme_input_style(&mut self.editor_style, self.dark, dark);
        retheme_input_style(&mut self.search_style, self.dark, dark);
        self.dark = dark;
        self.editor.update(cx, |editor, cx| {
            editor.set_editor_style(self.editor_style.clone());
            cx.notify();
        });
        self.search.update(cx, |editor, cx| {
            editor.set_editor_style(self.search_style.clone());
            cx.notify();
        });
        self.context_menu
            .update(cx, |menu, cx| menu.set_theme(dark, window, cx));
        cx.notify();
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.editor_style = style.clone();
        self.editor.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
    }

    pub fn set_search_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.search_style = style.clone();
        self.search.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
    }

    pub fn context_menu(&self) -> Entity<Menu> {
        self.context_menu.clone()
    }
    pub fn search_editor(&self) -> Entity<InputState> {
        self.search.clone()
    }
    pub fn editor(&self) -> Entity<TextareaState> {
        self.editor.clone()
    }
    fn initial_editor_style(dark: bool, disabled: bool) -> gpui_kit::base::input::InputEditorStyle {
        gpui_kit::base::input::InputEditorStyle {
            macos_textarea_selection: true,
            unfocused_selection: disabled,
            disabled_opacity: 0.45,
            auto_grow_cursor_margin: Some(px(13.)),
            ..support::input_style(dark)
        }
    }

    pub fn tooltip_label(&self, cx: &gpui_kit::App) -> Option<&'static str> {
        self.hint_visible.then(|| self.hint_content(cx).0)
    }

    fn hint_content(&self, cx: &gpui_kit::App) -> (&'static str, &'static str) {
        if self.streaming && self.editor.read(cx).value().trim().is_empty() {
            ("Stop Pi", "⌘.")
        } else if self.streaming {
            ("Queue Steering Message", "Enter")
        } else {
            ("Send Message", "Enter")
        }
    }

    fn hover_hint(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hint_hovered = hovered;
        self.hint_task = None;
        self.hint_visible = false;
        if hovered {
            self.hint_task = Some(cx.spawn_in(window, async move |owner, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(450))
                    .await;
                let _ = owner.update_in(cx, |this, _, cx| {
                    this.hint_visible = this.hint_hovered;
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }
    pub fn open_selector(&self) -> Option<&'static str> {
        self.selector
    }
    pub fn model(&self) -> Option<&Choice> {
        self.models.get(self.model)
    }
    pub fn effort(&self) -> Option<&Choice> {
        self.efforts.get(self.effort)
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn visible_models(&self) -> Vec<&Choice> {
        self.visible_model_indices()
            .into_iter()
            .map(|i| &self.models[i])
            .collect()
    }
    pub fn model_cursor(&self) -> Option<&Choice> {
        (self.selector == Some("model") && self.visible_model_indices().contains(&self.cursor))
            .then(|| &self.models[self.cursor])
    }
    pub fn effort_cursor(&self) -> Option<&Choice> {
        (self.selector == Some("effort"))
            .then(|| self.efforts.get(self.cursor))
            .flatten()
    }
    pub fn set_fill_height(&mut self, fill: bool, cx: &mut Context<Self>) {
        if self.fill_height == fill {
            return;
        }
        self.fill_height = fill;
        self.editor.update(cx, |editor, cx| {
            editor.set_editor_paddings(gpui_kit::Edges {
                top: px(if fill { 16. } else { 4. }),
                right: px(4.),
                bottom: px(3.),
                left: px(4.),
            });
            cx.notify();
        });
        cx.notify();
    }
    pub fn set_status(&mut self, disabled: bool, streaming: bool, cx: &mut Context<Self>) {
        self.context_menu.update(cx, |menu, cx| menu.dismiss(cx));
        self.menu_target = None;
        self.disabled = disabled;
        self.streaming = streaming;
        if !streaming {
            self.stopped = false;
        }
        self.editor.update(cx, |editor, cx| {
            editor.set_disabled(disabled, cx);
            editor.set_readonly(disabled, cx);
        });
        if disabled || streaming {
            self.close(cx);
        }
        cx.notify();
    }
    pub fn set_choices(
        &mut self,
        models: Vec<Choice>,
        efforts: Vec<Choice>,
        cx: &mut Context<Self>,
    ) {
        let model = self.model().map(|choice| choice.id.clone());
        let effort = self.effort().map(|choice| choice.id.clone());
        self.models = models;
        self.efforts = efforts;
        self.model = self
            .models
            .iter()
            .position(|choice| Some(&choice.id) == model.as_ref())
            .unwrap_or(0);
        self.effort = self
            .efforts
            .iter()
            .position(|choice| Some(&choice.id) == effort.as_ref())
            .unwrap_or(0);
        self.close(cx);
    }
    pub fn set_model(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.models.iter().position(|choice| choice.id == id) {
            self.model = ix;
            cx.notify();
        }
    }
    pub fn set_effort(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.efforts.iter().position(|choice| choice.id == id) {
            self.effort = ix;
            cx.notify();
        }
    }

    fn visible_model_indices(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        (0..self.models.len())
            .filter(|&i| {
                query.is_empty()
                    || self.models[i].id.to_lowercase().contains(&query)
                    || self.models[i].label.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.selector = None;
        self.query.clear();
        cx.notify();
    }

    fn choose(&mut self, selector: &str, index: usize, cx: &mut Context<Self>) {
        if self.selector != Some(selector) || self.disabled || self.streaming || self.stopped {
            return;
        }
        if selector == "model" {
            if let Some(option) = self.models.get(index) {
                self.model = index;
                cx.emit(Event::ModelChanged(option.id.clone()));
            }
        } else {
            if let Some(option) = self.efforts.get(index) {
                self.effort = index;
                cx.emit(Event::EffortChanged(option.id.clone()));
            }
        }
        self.close(cx);
    }

    fn move_cursor(&mut self, down: bool, cx: &mut Context<Self>) {
        let visible = match self.selector {
            Some("model") => self.visible_model_indices(),
            Some("effort") => (0..self.efforts.len()).collect(),
            _ => return,
        };
        if visible.is_empty() {
            return;
        }
        let current = visible.iter().position(|&i| i == self.cursor).unwrap_or(0);
        self.cursor = visible[if down {
            (current + 1).min(visible.len() - 1)
        } else {
            current.saturating_sub(1)
        }];
        cx.notify();
    }

    fn key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(selector) = self.selector else {
            return;
        };
        match e.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "enter" | "space" if selector == "effort" || e.keystroke.key == "enter" => {
                if selector == "effort" || self.visible_model_indices().contains(&self.cursor) {
                    self.choose(selector, self.cursor, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn menu(&self, selector: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let is_model = selector == "model";
        let visible = if is_model {
            self.visible_model_indices()
        } else {
            (0..self.efforts.len()).collect()
        };
        let trigger_bounds = if is_model {
            self.model_bounds.clone()
        } else {
            self.effort_bounds.clone()
        };
        let mut menu = div()
            .id(format!("{selector}-menu"))
            .test_support()
            .w(px(if is_model { 300. } else { 70. }))
            .rounded(px(8.))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                .blur_radius(px(20.)),
            ])
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(move |s, event: &MouseDownEvent, _, cx| {
                if s.selector == Some(selector)
                    && !trigger_bounds
                        .get()
                        .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    s.close(cx);
                }
            }))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|s, _: &NextChoice, _, cx| s.move_cursor(true, cx)))
            .on_action(cx.listener(|s, _: &PreviousChoice, _, cx| s.move_cursor(false, cx)));
        if is_model {
            menu = menu.overflow_hidden().child(
                div()
                    .id("model-search")
                    .test_support()
                    .h(px(28.))
                    .px(px(12.))
                    .bg(rgb(if self.dark { 0x080b11 } else { 0xebeef0 }))
                    .rounded_tl(px(8.))
                    .rounded_tr(px(8.))
                    .text_size(px(11.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("model-search-target")
                            .test_support()
                            .w_full()
                            .key_context("NativeComposerSearch")
                            .child(Input::new(&self.search)),
                    ),
            );
        } else {
            menu = menu.border_1().border_color(rgb(t.border));
        }
        let mut list = div()
            .id(format!("{selector}-list"))
            .test_support()
            .px(px(4.))
            .pb(px(4.))
            .when(!is_model, |d| d.pt(px(4.)))
            .flex()
            .flex_col();
        if is_model && !visible.is_empty() {
            list = list.child(
                div()
                    .id("model-heading")
                    .test_support()
                    .h(px(22.))
                    .mr(px(10.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(rgb(t.faint))
                    .child(self.model_heading.clone()),
            );
        }
        if is_model && visible.is_empty() {
            list = list.child(
                div()
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .text_color(rgb(t.muted))
                    .child("No matching models"),
            );
        }
        for ix in visible {
            let label = if is_model {
                self.models[ix].label.clone()
            } else {
                self.efforts[ix].label.clone()
            };
            let selected = if is_model {
                self.model == ix
            } else {
                self.effort == ix
            };
            list = list.child(
                BaseButton::new(if is_model {
                    self.models[ix].element_id.clone()
                } else {
                    self.efforts[ix].element_id.clone()
                })
                .w_full()
                .h(px(if is_model { 28. } else { 24. }))
                .px(px(8.))
                .relative()
                .rounded(px(6.))
                .text_size(px(11.))
                .text_color(rgb(t.text))
                .focusable(false)
                .tab_stop(false)
                .bg(if self.cursor == ix {
                    gpui_kit::Rgba {
                        a: if self.dark { 0.2 } else { 0.1 },
                        ..rgb(if self.dark { 0x475266 } else { 0x6b7d8f })
                    }
                } else {
                    rgba(0x00000000)
                })
                .when(selected, |b| {
                    b.child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(3.))
                            .bottom(px(3.))
                            .w(px(2.))
                            .rounded_tr(px(1.))
                            .rounded_br(px(1.))
                            .bg(rgb(t.accent)),
                    )
                })
                .on_mouse_move(cx.listener(move |s, _, _, cx| {
                    if s.cursor != ix {
                        s.cursor = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |s, _, _, cx| s.choose(selector, ix, cx)))
                .child(div().w_full().h_full().flex().items_center().child(label)),
            );
        }
        menu.child(list)
    }

    fn send(&mut self, follow_up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.stopped {
            return;
        }
        if self.editor.read(cx).value().trim().is_empty() {
            if self.streaming {
                self.stopped = true;
                cx.emit(Event::Stop);
                cx.notify();
            }
            return;
        }
        let draft = self.editor.read(cx).value().to_string();
        self.editor.update(cx, |e, cx| e.set_value("", window, cx));
        cx.emit(if follow_up {
            Event::FollowUp(draft)
        } else {
            Event::Send(draft)
        });
        cx.notify();
    }
}
impl Composer {
    /// Move window-bound routes without replacing editing state or undo history.
    pub fn reattach_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        self.hint_task = None;
        self.hint_hovered = false;
        self.hint_visible = false;
        self.context_menu.update(cx, |menu, cx| menu.dismiss(cx));
        self.menu_target = None;
        self._events = cx.subscribe_in(
            &self.editor,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    shift: false,
                    secondary,
                } = event
                {
                    this.send(*secondary, window, cx);
                }
                cx.notify();
            },
        );
        self._search_events = cx.subscribe(&self.search, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                let visible = this.visible_model_indices();
                if !visible.contains(&this.cursor) {
                    this.cursor = visible.first().copied().unwrap_or(0);
                }
                cx.notify();
            }
        });
        self._menu_events = cx.subscribe_in(
            &self.context_menu,
            window,
            |this, _, event: &MenuEvent, window, cx| {
                this.context_menu_event(event, window, cx);
            },
        );
    }

    fn context_menu_event(
        &mut self,
        event: &MenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.menu_target.take();
        if let MenuEvent::Selected(id) = event {
            let editor = self.editor.read(cx);
            if target != Some((editor.value().to_string(), editor.selected_range())) {
                return;
            }
            let focus = editor.focus_handle(cx);
            match id.as_str() {
                "cut" if editor.is_editable() && !editor.selected_range().is_empty() => {
                    focus.dispatch_action(&Cut, window, cx)
                }
                "copy" if editor.is_copyable() => focus.dispatch_action(&Copy, window, cx),
                "paste" if editor.is_editable() => focus.dispatch_action(&Paste, window, cx),
                _ => {}
            }
        }
    }

    pub fn new(options: ComposerOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dark = options.dark;
        let disabled = options.disabled;
        let streaming = options.streaming;
        let scrollbar = Theme::global(cx).scrollbar.clone();
        Theme::global_mut(cx).scrollbar =
            scrollbar.with_styles(ScrollbarStyles::default().thumb(|thumb| thumb.radius(px(4.))));
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .accessibility_label("Message")
                .submit_on_enter(true)
                .placeholder("Message π")
                .auto_grow(4, 10)
        });
        editor.update(cx, |e, cx| {
            e.set_editor_paddings(gpui_kit::Edges {
                top: px(4.),
                right: px(4.),
                bottom: px(3.),
                left: px(4.),
            });
            e.set_disabled(disabled, cx);
            e.set_readonly(disabled, cx);
            e.set_protect_ime_keys(true);
            e.set_value(&options.draft, window, cx);
            e.set_scroll_sensitivity(0.5);
            e.set_editor_style(Self::initial_editor_style(dark, disabled));
        });
        let events = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    shift: false,
                    secondary,
                } = event
                {
                    this.send(*secondary, window, cx);
                }
                cx.notify();
            },
        );
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextChoice, Some("NativeComposerSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousChoice, Some("NativeComposerSearch > Input")),
        ]);
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .accessibility_label("Search Models")
                .placeholder("Search")
        });
        search.update(cx, |s, _| {
            s.set_editor_style(support::input_style(dark));
            s.set_protect_ime_keys(true);
        });
        let search_events = cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                let visible = this.visible_model_indices();
                if !visible.contains(&this.cursor) {
                    this.cursor = visible.first().copied().unwrap_or(0);
                }
                cx.notify();
            }
        });
        let context_menu = cx.new(|cx| {
            let mut menu = Menu::new_context(dark, vec![], cx);
            menu.set_min_width(px(152.), cx);
            menu
        });
        let owner = cx.entity().downgrade();
        editor.update(cx, |editor, _| {
            editor.on_context_menu(Rc::new(move |_, capabilities, position, window, cx| {
                let owner = owner.clone();
                // Base invokes this hook while leasing the editor entity.
                window.defer(cx, move |window, cx| {
                    _ = owner.update(cx, |this, cx| {
                        this.close(cx);
                        let editor = this.editor.read(cx);
                        this.menu_target =
                            Some((editor.value().to_string(), editor.selected_range()));
                        let origin = editor.focus_handle(cx);
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
                            element_id: format!("composer-{id}"),
                            shortcut: None,
                            disabled,
                        })
                        .collect();
                        this.context_menu.update(cx, |menu, cx| {
                            menu.set_items(items, cx);
                            menu.open_at(position, Some(origin), window, cx);
                        });
                    });
                });
            }));
        });
        let menu_events = cx.subscribe_in(
            &context_menu,
            window,
            |this, _, event: &MenuEvent, window, cx| {
                this.context_menu_event(event, window, cx);
            },
        );
        let model = options
            .models
            .iter()
            .position(|o| o.id == options.model)
            .unwrap_or(0);
        let effort = options
            .efforts
            .iter()
            .position(|o| o.id == options.effort)
            .unwrap_or(0);
        Self {
            hint_hovered: false,
            hint_visible: false,
            hint_task: None,
            hint_bounds: Rc::new(Cell::new(None)),
            editor_style: Self::initial_editor_style(dark, disabled),
            search_style: support::input_style(dark),
            dark,
            models: options.models,
            model_heading: options.model_heading,
            efforts: options.efforts,
            editor,
            editor_bounds: Rc::new(Cell::new(None)),
            streaming,
            disabled,
            stopped: false,
            fill_height: false,
            selector: None,
            model,
            effort,
            cursor: 0,
            query: String::new(),
            search,
            effort_focus: cx.focus_handle(),
            suppress_effort_click: false,
            model_bounds: Rc::new(Cell::new(None)),
            effort_bounds: Rc::new(Cell::new(None)),
            context_menu,
            menu_target: None,
            _menu_events: menu_events,
            _events: events,
            _search_events: search_events,
        }
    }
}
impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let draft = self.editor.read(cx).value().to_string();
        let filled = !draft.trim().is_empty();
        let settings_disabled = self.disabled || self.streaming || self.stopped;
        let model_color = if self.selector == Some("model") {
            t.text
        } else {
            t.muted
        };
        let effort_color = if self.selector == Some("effort") {
            t.text
        } else {
            t.muted
        };
        let send_disabled = self.disabled || self.stopped || (!self.streaming && !filled);
        let icon_name = if self.streaming {
            if filled { "queue" } else { "stop" }
        } else {
            "triangle"
        };
        let send = BaseButton::new("send")
            .accessibility_label(if self.streaming {
                if filled { "Queue" } else { "Stop" }
            } else {
                "Send"
            })
            .disabled(send_disabled)
            .w(px(22.))
            .h(px(22.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(t.canvas))
            .text_color(rgb(if self.streaming && !filled {
                t.danger
            } else {
                t.muted
            }))
            .styles(|styles| styles.disabled(|button| button.opacity(0.45)))
            .on_click(cx.listener(|s, _, window, cx| s.send(false, window, cx)))
            .when(!send_disabled, |button| {
                button.hover(move |s| s.bg(rgb(t.hover)).text_color(rgb(t.text)))
            })
            .child(support::icon(
                icon_name,
                13.,
                if self.streaming && !filled {
                    t.danger
                } else {
                    t.muted
                },
            ));
        let hint_bounds = self.hint_bounds.clone();
        let send = div()
            .id("send-tooltip-trigger")
            .test_support()
            .ml_auto()
            .w(px(22.))
            .h(px(22.))
            .on_prepaint(move |bounds, _, _| hint_bounds.set(Some(bounds)))
            .on_hover(cx.listener(|s, hovered, window, cx| s.hover_hint(*hovered, window, cx)))
            .child(send);
        let hint = self
            .hint_visible
            .then(|| self.hint_bounds.get())
            .flatten()
            .map(|bounds| {
                let (label, shortcut) = self.hint_content(cx);
                gpui_kit::deferred(
                    Positioner::side(bounds)
                        .placement(Placement::Top)
                        .offset(px(6.))
                        .margin(px(6.))
                        .child(
                            Tooltip::new("composer-tooltip")
                                .max_w(px(260.))
                                .px(px(7.))
                                .py(px(3.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded(px(4.))
                                .bg(rgb(t.panel_raised))
                                .text_color(rgb(t.text))
                                .text_size(px(10.))
                                .line_height(gpui_kit::relative(1.25))
                                .shadow(vec![
                                    BoxShadow::new(
                                        px(0.),
                                        px(8.),
                                        rgba(if self.dark { 0x00000073 } else { 0x0000001f })
                                            .into(),
                                    )
                                    .blur_radius(px(10.)),
                                ])
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(label)
                                .child(
                                    div()
                                        .id("composer-tooltip-shortcut")
                                        .test_support()
                                        .flex_shrink_0()
                                        .px(px(4.))
                                        .py(px(1.))
                                        .border_1()
                                        .border_color(gpui_kit::Rgba {
                                            a: 0.35,
                                            ..rgb(t.muted)
                                        })
                                        .rounded(px(3.))
                                        .text_color(rgb(t.muted))
                                        .text_size(px(10.))
                                        .line_height(gpui_kit::relative(1.2))
                                        .child(shortcut),
                                ),
                        ),
                )
                .with_priority(200)
            });
        let model_bounds = self.model_bounds.clone();
        let model =
            Popup::new(
                "model-popup",
                div()
                    .relative()
                    .on_prepaint(move |bounds, _, _| model_bounds.set(Some(bounds)))
                    .child(
                        BaseButton::new("model")
                            .accessibility_label(format!(
                                "Model: {}",
                                self.model().map(|o| o.label.as_str()).unwrap_or_default()
                            ))
                            .disabled(settings_disabled)
                            .h(px(22.))
                            .relative()
                            .pl(px(5.))
                            .pr(px(24.))
                            .border_1()
                            .border_color(rgb(if self.selector == Some("model") {
                                t.accent
                            } else {
                                t.canvas
                            }))
                            .text_color(rgb(model_color))
                            .styles(|styles| styles.disabled(|button| button.opacity(0.45)))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(4.))
                            .when(!settings_disabled, |button| {
                                button.hover(move |s| s.bg(rgb(t.hover)))
                            })
                            .text_size(px(10.))
                            .on_click(cx.listener(|s, _, window, cx| {
                                if !s.disabled && !s.streaming && !s.stopped {
                                    if s.selector == Some("model") {
                                        s.close(cx);
                                    } else {
                                        s.selector = Some("model");
                                        s.cursor = s.model;
                                        s.query.clear();
                                        s.search.update(cx, |input, cx| {
                                            input.set_value("", window, cx);
                                            input.focus(window, cx);
                                        });
                                        cx.notify();
                                    }
                                }
                            }))
                            .child(self.model().map(|o| o.label.clone()).unwrap_or_default())
                            .child(
                                support::selector_arrow(model_color)
                                    .opacity(if settings_disabled { 0.45 } else { 1. }),
                            ),
                    ),
            )
            .anchor(Anchor::BottomLeft)
            .offset(px(5.))
            .when(self.selector == Some("model"), |popup| {
                popup.content(self.menu("model", cx))
            });
        let effort_bounds = self.effort_bounds.clone();
        let effort =
            Popup::new(
                "effort-popup",
                div()
                    .relative()
                    .on_prepaint(move |bounds, _, _| effort_bounds.set(Some(bounds)))
                    .child(
                        BaseButton::new("effort")
                            .accessibility_label(format!(
                                "Effort: {}",
                                self.effort().map(|o| o.label.as_str()).unwrap_or_default()
                            ))
                            .disabled(settings_disabled)
                            .track_focus(&self.effort_focus)
                            .on_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
                                if s.selector != Some("effort") {
                                    return;
                                }
                                match event.keystroke.key.as_str() {
                                    "down" => s.move_cursor(true, cx),
                                    "up" => s.move_cursor(false, cx),
                                    "enter" if s.selector == Some("effort") => {
                                        s.suppress_effort_click = true;
                                        s.key(event, window, cx);
                                        return;
                                    }
                                    _ => {
                                        s.key(event, window, cx);
                                        return;
                                    }
                                }
                                cx.stop_propagation();
                            }))
                            .h(px(22.))
                            .relative()
                            .pl(px(5.))
                            .pr(px(24.))
                            .border_1()
                            .border_color(rgb(if self.selector == Some("effort") {
                                t.accent
                            } else {
                                t.canvas
                            }))
                            .text_color(rgb(effort_color))
                            .styles(|styles| styles.disabled(|button| button.opacity(0.45)))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(4.))
                            .when(!settings_disabled, |button| {
                                button.hover(move |s| s.bg(rgb(t.hover)))
                            })
                            .text_size(px(10.))
                            .on_click(cx.listener(|s, _, window, cx| {
                                if s.suppress_effort_click {
                                    s.suppress_effort_click = false;
                                    return;
                                }
                                if !s.disabled && !s.streaming && !s.stopped {
                                    if s.selector == Some("effort") {
                                        s.close(cx);
                                    } else {
                                        s.selector = Some("effort");
                                        s.cursor = s.effort;
                                        s.effort_focus.focus(window, cx);
                                        cx.notify();
                                    }
                                }
                            }))
                            .child(self.effort().map(|o| o.label.clone()).unwrap_or_default())
                            .child(
                                support::selector_arrow(effort_color)
                                    .opacity(if settings_disabled { 0.45 } else { 1. }),
                            ),
                    ),
            )
            .anchor(Anchor::BottomLeft)
            .offset(px(5.))
            .when(self.selector == Some("effort"), |popup| {
                popup.content(self.menu("effort", cx))
            });
        let toolbar = div()
            .id("footer-gap")
            .test_support()
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(model)
            .child(effort)
            .child(send);
        let editor_bounds = self.editor_bounds.clone();
        let input = InputBase::new("editor")
            .role(None)
            .disabled(self.disabled)
            // Base propagates submit actions; do not let fallback text delivery
            // insert a newline before the deferred PressEnter subscriber sends.
            .on_action(|_: &gpui_kit::base::input::Enter, _, cx| cx.stop_propagation())
            .relative()
            .on_prepaint(move |bounds, _, _| editor_bounds.set(Some(bounds)))
            .on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, window, cx| {
                // Capture runs even when blank chrome consumes the target's mouse down.
                // Only release our own focus; menus and other editors own theirs independently.
                if s.editor_bounds
                    .get()
                    .is_some_and(|bounds| !bounds.contains(&event.position))
                    && s.editor.read(cx).focus_handle(cx).is_focused(window)
                {
                    window.blur(cx);
                }
            }))
            .w_full()
            .min_w_0()
            .when(self.fill_height, |input| input.flex_1().min_h_0())
            .when(!self.fill_height, |input| {
                input.min_h(px(83.)).max_h(px(196.))
            })
            .flex()
            .flex_col()
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.45))
            .child(Textarea::new(&self.editor));
        div()
            .w_full()
            .when(self.fill_height, |composer| {
                composer.h_full().min_h_0().flex().flex_col()
            })
            .occlude()
            .opacity(if self.disabled { 0.68 } else { 1. })
            .child(input)
            .child(toolbar)
            .child(self.context_menu.clone())
            .when_some(hint, |composer, hint| composer.child(hint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Focusable, MouseButton, TestApp, TestAppWindow, test::TestWindowExt};
    use std::{cell::RefCell, rc::Rc};

    fn options(disabled: bool, streaming: bool) -> ComposerOptions {
        let choices = |names: &[&str], prefix: &str| {
            names
                .iter()
                .map(|name| Choice {
                    id: (*name).into(),
                    label: (*name).into(),
                    element_id: format!("{prefix}-{name}"),
                })
                .collect()
        };
        ComposerOptions {
            models: choices(&["alpha", "beta"], "model"),
            model_heading: "Models".into(),
            efforts: choices(&["low", "high"], "effort"),
            model: "alpha".into(),
            effort: "low".into(),
            draft: "keep my draft".into(),
            dark: false,
            disabled,
            streaming,
        }
    }

    fn fixture(disabled: bool, streaming: bool) -> (TestApp, TestAppWindow<Composer>) {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let options = options(disabled, streaming);
        let mut window = app.open_window_with_options(Default::default(), |window, cx| {
            Composer::new(options, window, cx)
        });
        window.draw();
        (app, window)
    }

    fn click(window: &mut TestAppWindow<Composer>, id: &str) {
        window.draw();
        let position = window.update(|_, window, _| window.find(id.to_owned()).bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
    }

    fn right_click_editor(window: &mut TestAppWindow<Composer>) {
        window.draw();
        let position = window.update(|_, window, _| {
            let bounds = window.find("editor").bounds();
            gpui_kit::point(bounds.left() + px(20.), bounds.top() + px(12.))
        });
        window.simulate_click(position, MouseButton::Right);
        window.draw();
    }

    #[test]
    fn tooltip_real_hover_delay_leave_disabled_and_role_transitions() {
        use std::time::Duration;
        for disabled in [false, true] {
            let (app, mut window) = fixture(disabled, false);
            let editor = window.read(|composer, _| composer.editor());
            let position =
                window.update(|_, w, _| w.find("send-tooltip-trigger").bounds().center());
            window.simulate_mouse_move(position);
            window.draw();
            app.advance_clock(Duration::from_millis(449));
            window.draw();
            assert!(window.read(|s, cx| s.tooltip_label(cx).is_none()));
            app.advance_clock(Duration::from_millis(1));
            window.draw();
            assert_eq!(
                window.read(|s, cx| s.tooltip_label(cx)),
                Some("Send Message")
            );
            window.update(|s, _, cx| s.set_status(disabled, true, cx));
            window.draw();
            assert_eq!(
                window.read(|s, cx| s.tooltip_label(cx)),
                Some("Queue Steering Message")
            );
            window.update(|s, w, cx| s.editor.update(cx, |e, cx| e.set_value("", w, cx)));
            window.draw();
            assert_eq!(window.read(|s, cx| s.tooltip_label(cx)), Some("Stop Pi"));
            for dark in [true, false] {
                window.update(|s, w, cx| s.set_theme(dark, w, cx));
                window.draw();
                window.update(|_, w, _| {
                    assert!(w.try_find("composer-tooltip-shortcut").is_some());
                    assert!(w.painted_quads().iter().any(|q| q.background == rgb(support::tokens(dark).panel_raised).into()));
                });
                assert_eq!(window.read(|s, _| s.editor()), editor);
            }
            window.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)));
            window.draw();
            assert!(window.read(|s, cx| s.tooltip_label(cx).is_none()));
            window.simulate_mouse_move(position);
            window.draw();
            app.advance_clock(Duration::from_millis(200));
            window.simulate_mouse_move(gpui_kit::point(px(1.), px(1.)));
            window.draw();
            app.advance_clock(Duration::from_millis(500));
            window.draw();
            assert!(window.read(|s, cx| s.tooltip_label(cx).is_none()));
        }
    }

    #[test]
    fn theme_round_trip_preserves_editor_popup_and_history() {
        let (_app, mut window) = fixture(false, false);
        click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        window.simulate_keystroke("cmd-x");
        window.simulate_keystroke("cmd-z");
        window.draw();
        let editor = window.read(|composer, _| composer.editor.clone());
        let search = window.read(|composer, _| composer.search.clone());
        window.update(|composer, _, cx| {
            let mut style = composer.editor_style.clone();
            style.caret_visible = false;
            style.unfocused_selection = true;
            style.caret_width = Some(px(3.));
            composer.set_editor_style(style, cx);
        });
        window.simulate_keystroke("cmd-a");
        right_click_editor(&mut window);
        let before = window.read(|composer, cx| {
            let state = composer.editor.read(cx);
            (state.value(), state.selected_range(), state.scroll_offset())
        });
        let focus = window.update(|_, window, cx| window.focused(cx));
        for dark in [true, false] {
            window.update(|composer, window, cx| composer.set_theme(dark, window, cx));
            window.draw();
            window.read(|composer, cx| {
                assert_eq!(composer.editor, editor);
                assert_eq!(composer.search, search);
                let state = composer.editor.read(cx);
                assert_eq!(
                    (state.value(), state.selected_range(), state.scroll_offset()),
                    before
                );
                assert!(composer.context_menu.read(cx).is_open());
                assert!(!composer.editor_style.caret_visible);
                assert!(composer.editor_style.unfocused_selection);
                assert_eq!(composer.editor_style.caret_width, Some(px(3.)));
                assert_eq!(
                    composer.editor_style.foreground,
                    rgb(support::tokens(dark).text).into()
                );
                assert_eq!(
                    composer.search_style.selection,
                    support::input_style(dark).selection
                );
            });
            window.update(|_, window, cx| {
                assert_eq!(window.focused(cx), focus);
                assert!(
                    window.painted_quads().iter().any(|quad| {
                        quad.background == rgb(support::tokens(dark).canvas).into()
                    })
                );
            });
        }
        window.update(|composer, _, cx| {
            composer
                .context_menu
                .update(cx, |menu, cx| menu.dismiss(cx))
        });
        click(&mut window, "editor");
        window.simulate_keystroke("cmd-shift-z");
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            ""
        );
        window.simulate_keystroke("cmd-z");
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            before.0.to_string()
        );
    }

    #[test]
    fn context_menu_uses_real_selection_clipboard_and_history() {
        let (_app, mut window) = fixture(false, false);
        click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        let selection = window.read(|composer, cx| composer.editor.read(cx).selected_range());
        assert_eq!(selection, 0..13);
        right_click_editor(&mut window);
        assert!(window.read(|composer, cx| composer.context_menu.read(cx).is_open()));
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).selected_range()),
            selection
        );
        assert!(window.read(|composer, cx| {
            composer
                .context_menu
                .read(cx)
                .items()
                .iter()
                .all(|item| !item.disabled && item.shortcut.is_none())
        }));
        click(&mut window, "composer-copy");
        assert_eq!(
            window.update(|_, _, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "keep my draft"
        );
        right_click_editor(&mut window);
        window.simulate_keystroke("down");
        window.simulate_keystroke("enter");
        window.draw();
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            ""
        );
        window.simulate_keystroke("cmd-z");
        window.draw();
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            "keep my draft"
        );
        window.simulate_keystroke("cmd-shift-z");
        window.draw();
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            ""
        );
        right_click_editor(&mut window);
        assert!(window.read(|composer, cx| composer.context_menu.read(cx).items()[0].disabled));
        click(&mut window, "composer-paste");
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            "keep my draft"
        );
    }

    #[test]
    fn context_menu_empty_clipboard_preserves_selection() {
        let (_app, mut window) = fixture(false, false);
        click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        window.update(|_, _, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(String::new()))
        });
        right_click_editor(&mut window);
        click(&mut window, "composer-paste");
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            "keep my draft"
        );
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).selected_range()),
            0..13
        );
    }

    #[test]
    fn context_menu_readonly_escape_outside_focus_and_stale_draft() {
        let (_app, mut window) = fixture(false, false);
        click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        window.update(|composer, _, cx| {
            composer
                .editor
                .update(cx, |editor, cx| editor.set_readonly(true, cx))
        });
        right_click_editor(&mut window);
        assert_eq!(
            window.read(|composer, cx| composer
                .context_menu
                .read(cx)
                .items()
                .iter()
                .map(|item| item.disabled)
                .collect::<Vec<_>>()),
            vec![true, false, true]
        );
        window.simulate_keystroke("down");
        window.simulate_keystroke("enter");
        assert_eq!(
            window.update(|_, _, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "keep my draft"
        );
        right_click_editor(&mut window);
        window.simulate_keystroke("escape");
        assert!(window.update(|composer, window, cx| {
            composer.editor.read(cx).focus_handle(cx).is_focused(window)
        }));
        right_click_editor(&mut window);
        click(&mut window, "model");
        assert!(!window.read(|composer, cx| composer.context_menu.read(cx).is_open()));
        assert!(window.update(|composer, window, cx| {
            composer.search.read(cx).focus_handle(cx).is_focused(window)
        }));
        window.simulate_keystroke("escape");
        window.update(|composer, _, cx| {
            composer
                .editor
                .update(cx, |editor, cx| editor.set_readonly(false, cx))
        });
        right_click_editor(&mut window);
        window.update(|composer, window, cx| {
            composer
                .editor
                .update(cx, |editor, cx| editor.set_value("replacement", window, cx))
        });
        click(&mut window, "composer-cut");
        assert_eq!(
            window.read(|composer, cx| composer.editor.read(cx).value().to_string()),
            "replacement"
        );
        window.update(|composer, _, cx| composer.set_status(true, false, cx));
        right_click_editor(&mut window);
        assert!(!window.read(|composer, cx| composer.context_menu.read(cx).is_open()));
    }

    struct PointerHost {
        composer: Entity<Composer>,
        other: Entity<InputState>,
    }

    impl Render for PointerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("blank-chrome")
                        .test_support()
                        .h(px(80.))
                        .on_mouse_down(MouseButton::Left, |_, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                        }),
                )
                .child(
                    div()
                        .id("other-input")
                        .test_support()
                        .child(Input::new(&self.other)),
                )
                .child(div().flex_1().min_h_0().child(self.composer.clone()))
        }
    }

    fn pointer_fixture(
        fill: bool,
    ) -> (
        TestApp,
        TestAppWindow<gpui_kit::base::Root>,
        Entity<PointerHost>,
    ) {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let mut host = None;
        let mut window = app.open_window_with_options(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: gpui_kit::size(px(640.), px(650.)),
                })),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| PointerHost {
                    composer: cx.new(|cx| {
                        let mut options = options(false, false);
                        if fill {
                            options.draft.clear();
                        }
                        let mut composer = Composer::new(options, window, cx);
                        composer.set_fill_height(fill, cx);
                        composer
                    }),
                    other: cx.new(|cx| InputState::new(window, cx)),
                });
                host = Some(view.clone());
                gpui_kit::base::Root::new(view, window, cx)
            },
        );
        window.draw();
        (app, window, host.unwrap())
    }

    fn root_click(window: &mut TestAppWindow<gpui_kit::base::Root>, id: &str) {
        window.draw();
        let position = window.update(|_, w, _| w.find(id.to_owned()).bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
    }

    #[test]
    fn outside_capture_blurs_only_editor_and_preserves_draft_selection_history() {
        let (_app, mut window, host) = pointer_fixture(false);
        let (composer, other) = window.read(|_, cx| {
            let host = host.read(cx);
            (host.composer.clone(), host.other.clone())
        });
        let editor = window.read(|_, cx| composer.read(cx).editor());
        root_click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        window.update(|_, _, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("edited draft".into()));
        });
        window.simulate_keystroke("cmd-v");
        window.simulate_keystroke("cmd-a");
        let selection = window.read(|_, cx| editor.read(cx).selected_range());
        for dark in [true, false] {
            window.update(|_, w, cx| composer.update(cx, |s, cx| s.set_theme(dark, w, cx)));
            window.draw();
            root_click(&mut window, "blank-chrome");
            assert!(!window.update(|_, w, cx| editor.read(cx).focus_handle(cx).is_focused(w)));
            window.read(|_, cx| {
                assert_eq!(editor.read(cx).value().as_str(), "edited draft");
                assert_eq!(editor.read(cx).selected_range(), selection);
                assert_eq!(composer.read(cx).editor(), editor);
            });
            window.update(|_, w, cx| editor.read(cx).focus_handle(cx).focus(w, cx));
        }
        root_click(&mut window, "other-input");
        assert!(window.update(|_, w, cx| other.read(cx).focus_handle(cx).is_focused(w)));
        root_click(&mut window, "blank-chrome");
        assert!(window.update(|_, w, cx| other.read(cx).focus_handle(cx).is_focused(w)));
        window.update(|_, w, cx| editor.read(cx).focus_handle(cx).focus(w, cx));
        window.simulate_keystroke("cmd-z");
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "keep my draft"
        );
        window.simulate_keystroke("cmd-shift-z");
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "edited draft"
        );

        root_click(&mut window, "effort");
        assert!(window.update(|_, w, cx| composer.read(cx).effort_focus.is_focused(w)));
        root_click(&mut window, "model");
        assert!(window.update(|_, w, cx| {
            composer
                .read(cx)
                .search
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        }));
        root_click(&mut window, "blank-chrome");
        assert!(window.update(|_, w, cx| {
            composer
                .read(cx)
                .search
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        }));
        root_click(&mut window, "editor");
        let toolbar_blank = window.update(|_, w, _| {
            let bounds = w.find("footer-gap").bounds();
            gpui_kit::point(bounds.right() - px(40.), bounds.center().y)
        });
        window.simulate_click(toolbar_blank, MouseButton::Left);
        assert!(!window.update(|_, w, cx| editor.read(cx).focus_handle(cx).is_focused(w)));
    }

    #[test]
    fn root_replaced_composer_cannot_blur_another_sessions_editor() {
        let (_app, mut window, host) = pointer_fixture(false);
        let first = window.read(|_, cx| host.read(cx).composer.clone());
        let first_editor = window.read(|_, cx| first.read(cx).editor());
        root_click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        let second = window.update(|_, w, cx| {
            let second = cx.new(|cx| Composer::new(options(false, false), w, cx));
            host.update(cx, |host, cx| {
                host.composer = second.clone();
                cx.notify();
            });
            second
        });
        window.draw();
        let second_editor = window.read(|_, cx| second.read(cx).editor());
        root_click(&mut window, "editor");
        assert!(window.update(|_, w, cx| second_editor.read(cx).focus_handle(cx).is_focused(w)));
        window.update(|_, _, cx| {
            host.update(cx, |host, cx| {
                host.composer = first.clone();
                cx.notify();
            })
        });
        window.draw();
        root_click(&mut window, "blank-chrome");
        // The visible composer must not blur an editor belonging to the inactive session.
        assert!(window.update(|_, w, cx| second_editor.read(cx).focus_handle(cx).is_focused(w)));
        window.read(|_, cx| {
            assert_eq!(first_editor.read(cx).selected_range(), 0..13);
            assert_eq!(first_editor.read(cx).value().as_str(), "keep my draft");
        });
        root_click(&mut window, "editor");
        root_click(&mut window, "blank-chrome");
        assert!(!window.update(|_, w, cx| first_editor.read(cx).focus_handle(cx).is_focused(w)));
    }

    #[test]
    fn root_empty_full_height_inside_pointer_keeps_focus_and_places_caret() {
        let (_app, mut window, host) = pointer_fixture(true);
        let composer = window.read(|_, cx| host.read(cx).composer.clone());
        let editor = window.read(|_, cx| composer.read(cx).editor());
        let lower = window.update(|_, w, cx| {
            let bounds = w.find("editor").bounds();
            assert!(bounds.size.height > px(450.));
            assert_eq!(composer.read(cx).editor_bounds.get(), Some(bounds));
            gpui_kit::point(bounds.center().x, bounds.bottom() - px(20.))
        });
        window.simulate_click(lower, MouseButton::Left);
        assert!(window.update(|_, w, cx| editor.read(cx).focus_handle(cx).is_focused(w)));
        window.simulate_input("caret here");
        window.draw();
        window.simulate_click(lower, MouseButton::Left);
        assert!(window.update(|_, w, cx| editor.read(cx).focus_handle(cx).is_focused(w)));
        assert_eq!(
            window.read(|_, cx| editor.read(cx).selected_range()),
            10..10
        );
        root_click(&mut window, "blank-chrome");
        assert!(!window.update(|_, w, cx| editor.read(cx).focus_handle(cx).is_focused(w)));
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "caret here"
        );
    }

    #[test]
    fn root_context_menu_pointer_copy_cut_paste_and_undo_survive_outside_capture() {
        let (_app, mut window, host) = pointer_fixture(false);
        let composer = window.read(|_, cx| host.read(cx).composer.clone());
        let editor = window.read(|_, cx| composer.read(cx).editor());
        root_click(&mut window, "editor");
        window.simulate_keystroke("cmd-a");
        for id in ["composer-copy", "composer-cut", "composer-paste"] {
            window.draw();
            let position = window.update(|_, w, _| {
                let bounds = w.find("editor").bounds();
                gpui_kit::point(bounds.left() + px(20.), bounds.top() + px(12.))
            });
            window.simulate_click(position, MouseButton::Right);
            window.draw();
            assert!(window.read(|_, cx| composer.read(cx).context_menu.read(cx).is_open()));
            root_click(&mut window, id);
            let expected = if id == "composer-cut" {
                ""
            } else {
                "keep my draft"
            };
            assert_eq!(
                window.read(|_, cx| editor.read(cx).value().to_string()),
                expected
            );
        }
        window.simulate_keystroke("cmd-z");
        assert_eq!(window.read(|_, cx| editor.read(cx).value().to_string()), "");
        window.simulate_keystroke("cmd-z");
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "keep my draft"
        );
    }

    struct BottomHost {
        composer: Entity<Composer>,
    }

    impl Render for BottomHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .justify_end()
                .child(self.composer.clone())
        }
    }

    struct FillHeightRoot {
        composer: Entity<Composer>,
    }

    impl Render for FillHeightRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .justify_end()
                .child(self.composer.clone())
        }
    }

    fn fill_height_fixture() -> (TestApp, TestAppWindow<FillHeightRoot>) {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let mut window = app.open_window_with_options(
            gpui_kit::WindowOptions {
                window_bounds: Some(gpui_kit::WindowBounds::Windowed(gpui_kit::Bounds {
                    origin: Default::default(),
                    size: gpui_kit::size(px(640.), px(650.)),
                })),
                ..Default::default()
            },
            |window, cx| FillHeightRoot {
                composer: cx.new(|cx| {
                    let mut options = options(false, false);
                    options.draft = "".into();
                    Composer::new(options, window, cx)
                }),
            },
        );
        window.draw();
        (app, window)
    }

    fn set_fill_height(window: &mut TestAppWindow<FillHeightRoot>, fill: bool) {
        window.update(|root, _, cx| {
            root.composer
                .update(cx, |composer, cx| composer.set_fill_height(fill, cx));
        });
        window.draw();
    }

    fn assert_fill_height_layout(window: &mut TestAppWindow<FillHeightRoot>, fill: bool) {
        window.update(|_, window, _| {
            let editor = window.find("editor").bounds();
            let toolbar = window.find("footer-gap").bounds();
            if fill {
                assert!(editor.size.height > px(500.), "editor viewport: {editor:?}");
                assert_eq!(editor.top(), px(0.));
            } else {
                assert_eq!(editor.size.height, px(83.));
            }
            assert_eq!(editor.bottom(), toolbar.top());
            assert_eq!(toolbar.bottom(), px(650.));
            for id in ["model", "effort", "send"] {
                let control = window.find(id).bounds();
                assert!(control.top() >= toolbar.top());
                assert!(control.bottom() <= toolbar.bottom());
            }
        });
    }

    #[test]
    fn fill_height_uses_real_editor_viewport_and_keeps_controls_at_bottom() {
        let (_app, mut window) = fill_height_fixture();
        let editor = window.read(|root, cx| root.composer.read(cx).editor());
        assert_fill_height_layout(&mut window, false);
        set_fill_height(&mut window, true);
        assert_fill_height_layout(&mut window, true);

        let lower_editor = window.update(|_, window, _| {
            let bounds = window.find("editor").bounds();
            gpui_kit::point(bounds.center().x, bounds.bottom() - px(30.))
        });
        assert!(lower_editor.y > px(500.));
        window.simulate_click(lower_editor, MouseButton::Left);
        assert!(window.update(|_, window, cx| editor.read(cx).focus_handle(cx).is_focused(window)));
        window.simulate_input("typed in the lower editor");
        window.draw();
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "typed in the lower editor"
        );

        for fill in [false, true] {
            set_fill_height(&mut window, fill);
            assert_fill_height_layout(&mut window, fill);
            assert_eq!(
                window.read(|root, cx| root.composer.read(cx).editor().entity_id()),
                editor.entity_id()
            );
            assert_eq!(
                window.read(|_, cx| editor.read(cx).value().to_string()),
                "typed in the lower editor"
            );
        }
        window.simulate_keystroke("cmd-z");
        window.draw();
        assert_eq!(window.read(|_, cx| editor.read(cx).value().to_string()), "");
        window.simulate_keystroke("cmd-shift-z");
        window.draw();
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            "typed in the lower editor"
        );
    }

    #[test]
    fn fill_height_long_draft_scroll_selection_and_history_survive_mode_changes() {
        let (_app, mut window) = fill_height_fixture();
        let editor = window.read(|root, cx| root.composer.read(cx).editor());
        set_fill_height(&mut window, true);
        let position = window.update(|_, window, _| window.find("editor").bounds().center());
        window.simulate_click(position, MouseButton::Left);
        let draft = (0..120)
            .map(|line| format!("Draft line {line}\n"))
            .collect::<String>();
        window.simulate_input(&draft);
        window.draw();
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            draft
        );
        assert_fill_height_layout(&mut window, true);
        assert!(window.read(|_, cx| editor.read(cx).scroll_offset().y < px(-1.)));
        let visible = window.read(|_, cx| editor.read(cx).visible_row_range().unwrap());
        assert!(
            visible.start > 0,
            "typing must scroll the actual editor: {visible:?}"
        );
        assert!(
            visible.len() > 20,
            "full mode must expand the editor's own visible rows: {visible:?}"
        );

        window.simulate_keystroke("shift-left");
        window.simulate_keystroke("shift-left");
        window.draw();
        let selection = window.read(|_, cx| editor.read(cx).selected_range());
        assert_eq!(selection, draft.len() - 2..draft.len());
        for fill in [false, true] {
            set_fill_height(&mut window, fill);
            assert_eq!(
                window.read(|root, cx| root.composer.read(cx).editor().entity_id()),
                editor.entity_id()
            );
            assert_eq!(
                window.read(|_, cx| editor.read(cx).value().to_string()),
                draft
            );
            assert_eq!(
                window.read(|_, cx| editor.read(cx).selected_range()),
                selection
            );
            assert!(
                window.update(|_, window, cx| editor.read(cx).focus_handle(cx).is_focused(window))
            );
            assert!(window.read(|_, cx| editor.read(cx).scroll_offset().y < px(-1.)));
            window.update(|_, window, _| {
                let bounds = window.find("editor").bounds();
                let toolbar = window.find("footer-gap").bounds();
                if fill {
                    assert!(bounds.size.height > px(500.));
                } else {
                    assert!(bounds.size.height <= px(196.));
                }
                assert_eq!(bounds.bottom(), toolbar.top());
                assert_eq!(toolbar.bottom(), px(650.));
            });
        }

        window.simulate_input("X");
        window.draw();
        let replaced = format!("{}X", &draft[..selection.start]);
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            replaced
        );
        window.simulate_keystroke("cmd-z");
        window.draw();
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            draft
        );
        assert_eq!(
            window.read(|_, cx| editor.read(cx).selected_range()),
            selection
        );
        window.simulate_keystroke("cmd-shift-z");
        window.draw();
        assert_eq!(
            window.read(|_, cx| editor.read(cx).value().to_string()),
            replaced
        );
    }

    #[test]
    fn popup_dismissal_exempts_only_its_own_trigger() {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let mut window =
            app.open_window_with_options(Default::default(), |window, cx| BottomHost {
                composer: cx.new(|cx| Composer::new(options(false, false), window, cx)),
            });
        let click = |window: &mut TestAppWindow<BottomHost>, id: &str| {
            window.draw();
            let position =
                window.update(|_, window, _| window.find(id.to_owned()).bounds().center());
            window.simulate_click(position, MouseButton::Left);
            window.draw();
        };
        click(&mut window, "model");
        window.update(|_, w, _| {
            let trigger = w.find("model".to_owned()).bounds();
            let menu = w.find("model-menu".to_owned()).bounds();
            let effort = w.find("effort".to_owned()).bounds();
            assert!(menu.bottom() < trigger.top());
            assert!(!menu.contains(&effort.center()));
        });
        let model_bounds = window.update(|_, w, _| w.find("model".to_owned()).bounds());
        assert_eq!(
            window.read(|h, cx| h.composer.read(cx).model_bounds.get()),
            Some(model_bounds)
        );
        click(&mut window, "model-heading");
        assert_eq!(
            window.read(|h, cx| h.composer.read(cx).open_selector()),
            Some("model")
        );
        click(&mut window, "model");
        assert!(window.read(|h, cx| h.composer.read(cx).open_selector().is_none()));

        click(&mut window, "model");
        click(&mut window, "effort");
        window.update(|_, w, _| {
            let trigger = w.find("effort".to_owned()).bounds();
            assert_eq!(
                w.find("effort-menu".to_owned()).bounds().bottom(),
                trigger.top() - px(5.)
            );
        });
        assert_eq!(
            window.read(|h, cx| h.composer.read(cx).open_selector()),
            Some("effort")
        );
        let effort_bounds = window.update(|_, w, _| w.find("effort".to_owned()).bounds());
        assert_eq!(
            window.read(|h, cx| h.composer.read(cx).effort_bounds.get()),
            Some(effort_bounds)
        );
        click(&mut window, "effort");
        assert!(window.read(|h, cx| h.composer.read(cx).open_selector().is_none()));

        click(&mut window, "effort");
        let blank = window.update(|_, window, _| {
            let footer = window.find("footer-gap".to_owned()).bounds();
            gpui_kit::point(footer.right() - px(36.), footer.center().y)
        });
        window.simulate_click(blank, MouseButton::Left);
        window.draw();
        assert!(window.read(|h, cx| h.composer.read(cx).open_selector().is_none()));
        assert_eq!(
            window.read(|h, cx| h.composer.read(cx).editor().read(cx).value().to_string()),
            "keep my draft"
        );
    }

    #[test]
    fn disabled_controls_are_inert_and_draft_survives_status_changes() {
        let (_app, mut window) = fixture(true, false);
        click(&mut window, "model");
        click(&mut window, "send");
        assert!(window.read(|composer, _| composer.open_selector().is_none()));
        assert_eq!(
            window.read(|composer, cx| composer.editor().read(cx).value().to_string()),
            "keep my draft"
        );
        window.update(|composer, _, cx| composer.set_status(false, true, cx));
        click(&mut window, "model");
        click(&mut window, "effort");
        assert!(window.read(|composer, _| composer.open_selector().is_none()));
        assert_eq!(
            window.read(|composer, cx| composer.editor().read(cx).value().to_string()),
            "keep my draft"
        );
    }

    #[test]
    fn empty_choices_and_removed_selection_are_safe() {
        let (_app, mut window) = fixture(false, false);
        window.update(|composer, _, cx| composer.set_choices(vec![], vec![], cx));
        assert!(
            window.read(|composer, _| composer.model().is_none() && composer.effort().is_none())
        );
        click(&mut window, "model");
        assert!(window.read(|composer, _| composer.model_cursor().is_none()));
        window.update(|composer, _, cx| {
            composer.set_choices(
                vec![Choice {
                    id: "gamma".into(),
                    label: "Gamma".into(),
                    element_id: "model-gamma".into(),
                }],
                vec![Choice {
                    id: "medium".into(),
                    label: "Medium".into(),
                    element_id: "effort-medium".into(),
                }],
                cx,
            );
        });
        assert_eq!(
            window.read(|composer, _| composer.model().map(|c| c.id.clone())),
            Some("gamma".into())
        );
        assert_eq!(
            window.read(|composer, _| composer.effort().map(|c| c.id.clone())),
            Some("medium".into())
        );
    }

    #[test]
    fn stop_ready_and_next_stream_accept_new_messages() {
        let (_app, mut window) = fixture(false, true);
        let received = Rc::new(RefCell::new(Vec::new()));
        let events = received.clone();
        let _subscription = window.update(|composer, window, cx| {
            composer
                .editor
                .update(cx, |editor, cx| editor.set_value("", window, cx));
            cx.subscribe(&cx.entity(), move |_, _, event: &Event, _| {
                events.borrow_mut().push(event.clone());
            })
        });
        click(&mut window, "send");
        assert_eq!(*received.borrow(), vec![Event::Stop]);
        assert!(window.read(|composer, _| composer.is_stopped()));
        window.update(|composer, _, cx| composer.set_status(false, false, cx));
        assert!(!window.read(|composer, _| composer.is_stopped()));
        window.update(|composer, window, cx| {
            composer
                .editor
                .update(cx, |editor, cx| editor.set_value("next", window, cx));
        });
        click(&mut window, "send");
        assert_eq!(
            *received.borrow(),
            vec![Event::Stop, Event::Send("next".into())]
        );
        window.update(|composer, _, cx| composer.set_status(false, true, cx));
        assert!(window.read(|composer, _| composer.is_streaming()));
    }

    #[test]
    fn selector_switch_and_option_reorder_retain_identity_and_draft() {
        let (_app, mut window) = fixture(false, false);
        click(&mut window, "model");
        assert_eq!(
            window.read(|composer, _| composer.open_selector()),
            Some("model")
        );
        click(&mut window, "model-beta");
        assert_eq!(
            window.read(|composer, _| composer.model().map(|o| o.id.clone())),
            Some("beta".into())
        );
        click(&mut window, "effort");
        assert_eq!(
            window.read(|composer, _| composer.open_selector()),
            Some("effort")
        );
        click(&mut window, "effort-high");
        window.update(|composer, _, cx| {
            composer.set_choices(
                vec![Choice {
                    id: "beta".into(),
                    label: "New label".into(),
                    element_id: "model-beta".into(),
                }],
                vec![Choice {
                    id: "high".into(),
                    label: "High".into(),
                    element_id: "effort-high".into(),
                }],
                cx,
            )
        });
        assert_eq!(
            window.read(|composer, _| composer.model().map(|o| o.id.clone())),
            Some("beta".into())
        );
        assert_eq!(
            window.read(|composer, _| composer.effort().map(|o| o.id.clone())),
            Some("high".into())
        );
        assert_eq!(
            window.read(|composer, cx| composer.editor().read(cx).value().to_string()),
            "keep my draft"
        );
        click(&mut window, "model");
        window.simulate_keystroke("escape");
        window.draw();
        assert!(window.read(|composer, _| composer.open_selector().is_none()));
        assert_eq!(
            window.read(|composer, cx| composer.editor().read(cx).value().to_string()),
            "keep my draft"
        );
    }
}
