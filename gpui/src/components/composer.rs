use super::support;
use gpui_kit::{
    Anchor, Bounds, BoxShadow, Context, Entity, FocusHandle, IntoElement, KeyDownEvent,
    MouseDownEvent, Pixels, Render, Subscription, TestSupportExt, Window,
    base::{
        Button as BaseButton, ElementExt, Input, InputBase, Popup, ScrollbarStyles, Textarea,
        Theme,
        input::{InputEvent, InputState, TextareaState},
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
    Stop,
}

impl gpui_kit::EventEmitter<Event> for Composer {}

pub struct Composer {
    models: Vec<Choice>,
    model_heading: String,
    efforts: Vec<Choice>,
    dark: bool,
    editor: Entity<TextareaState>,
    streaming: bool,
    disabled: bool,
    stopped: bool,
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
    _events: Subscription,
    _search_events: Subscription,
}
impl Composer {
    pub fn search_editor(&self) -> Entity<InputState> {
        self.search.clone()
    }
    pub fn editor(&self) -> Entity<TextareaState> {
        self.editor.clone()
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
    pub fn set_status(&mut self, disabled: bool, streaming: bool, cx: &mut Context<Self>) {
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

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        cx.emit(Event::Send(draft));
        cx.notify();
    }
}
impl Composer {
    pub fn new(options: ComposerOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dark = options.dark;
        let disabled = options.disabled;
        let streaming = options.streaming;
        let scrollbar = Theme::global(cx).scrollbar.clone();
        Theme::global_mut(cx).scrollbar =
            scrollbar.with_styles(ScrollbarStyles::default().thumb(|thumb| thumb.radius(px(4.))));
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
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
            e.set_value(&options.draft, window, cx);
            e.set_scroll_sensitivity(0.5);
            e.set_editor_style(gpui_kit::base::input::InputEditorStyle {
                macos_textarea_selection: true,
                unfocused_selection: disabled,
                disabled_opacity: 0.45,
                auto_grow_cursor_margin: Some(px(13.)),
                ..support::input_style(dark)
            });
        });
        let events = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                    this.send(window, cx);
                }
                cx.notify();
            },
        );
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextChoice, Some("NativeComposerSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousChoice, Some("NativeComposerSearch > Input")),
        ]);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        search.update(cx, |s, _| s.set_editor_style(support::input_style(dark)));
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
            dark,
            models: options.models,
            model_heading: options.model_heading,
            efforts: options.efforts,
            editor,
            streaming,
            disabled,
            stopped: false,
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
            .disabled(send_disabled)
            .ml_auto()
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
            .on_click(cx.listener(|s, _, window, cx| s.send(window, cx)))
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
        let model_bounds = self.model_bounds.clone();
        let model =
            Popup::new(
                "model-popup",
                div()
                    .relative()
                    .on_prepaint(move |bounds, _, _| model_bounds.set(Some(bounds)))
                    .child(
                        BaseButton::new("model")
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
            .flex()
            .items_center()
            .gap(px(2.))
            .child(model)
            .child(effort)
            .child(send);
        let input = InputBase::new("editor")
            .w_full()
            .min_w_0()
            .min_h(px(83.))
            .max_h(px(196.))
            .flex()
            .flex_col()
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.45))
            .child(Textarea::new(&self.editor));
        div()
            .w_full()
            .occlude()
            .opacity(if self.disabled { 0.68 } else { 1. })
            .child(input)
            .child(toolbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{MouseButton, TestApp, TestAppWindow, test::TestWindowExt};
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
