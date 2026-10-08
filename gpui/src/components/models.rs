use super::support;
use gpui_kit::{
    Anchor, Bounds, BoxShadow, Context, Entity, IntoElement, KeyDownEvent, MouseDownEvent, Pixels,
    Render, Window,
    base::{
        Button, ElementExt, Input, Popup, TestSupportExt,
        input::{InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb, rgba,
};
use std::{cell::Cell, rc::Rc};

gpui_kit::actions!(native_models, [NextModel, PreviousModel]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub id: String,
    pub group: String,
    pub label: String,
    /// Stable UI target for integrations; defaults to the model identity.
    pub element_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Selected(String),
}

impl gpui_kit::EventEmitter<Event> for ModelSelector {}

pub struct ModelSelector {
    editor_style: gpui_kit::base::input::InputEditorStyle,
    options: Vec<ModelOption>,
    dark: bool,
    open: bool,
    selected: String,
    cursor: usize,
    query: String,
    search: Entity<InputState>,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}
impl ModelSelector {
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        super::composer::retheme_input_style(&mut self.editor_style, self.dark, dark);
        self.dark = dark;
        self.search.update(cx, |editor, cx| {
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
        self.search.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
    }
    fn visible(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..self.options.len())
            .filter(|&i| {
                q.is_empty()
                    || self.options[i].id.to_lowercase().contains(&q)
                    || self.options[i].label.to_lowercase().contains(&q)
            })
            .collect()
    }
    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.query.clear();
        cx.notify();
    }
    fn move_cursor(&mut self, down: bool, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let visible = self.visible();
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
        if !self.open {
            return;
        }
        let visible = self.visible();
        match e.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "enter" => {
                if visible.contains(&self.cursor) {
                    self.select(self.cursor, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let trigger_bounds = self.trigger_bounds.clone();
        let visible = self.visible();
        let mut last = String::new();
        let mut rows = Vec::new();
        for ix in visible.iter().copied() {
            let option = &self.options[ix];
            let group = option.group.clone();
            let label = option.label.clone();
            if group != last {
                last = group.clone();
                rows.push(
                    div()
                        .id(format!("heading-{ix}"))
                        .test_support()
                        .h(px(22.))
                        .mr(px(10.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(10.))
                        .text_color(rgb(t.faint))
                        .child(group)
                        .into_any_element(),
                );
            }
            rows.push(
                Button::new(option.element_id.clone())
                    .w_full()
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .relative()
                    .bg(if self.cursor == ix {
                        gpui_kit::Rgba {
                            a: if self.dark { 0.2 } else { 0.1 },
                            ..rgb(if self.dark { 0x475266 } else { 0x6b7d8f })
                        }
                    } else {
                        rgba(0x00000000)
                    })
                    .text_color(rgb(t.text))
                    .text_size(px(11.))
                    .focusable(false)
                    .tab_stop(false)
                    .when(self.options[ix].id == self.selected, |b| {
                        b.child(
                            div()
                                .absolute()
                                .left(px(0.))
                                .top(px(3.))
                                .bottom(px(3.))
                                .w(px(2.))
                                .rounded(px(1.))
                                .bg(rgb(t.accent)),
                        )
                    })
                    .on_mouse_move(cx.listener(move |s, _, _, cx| {
                        if s.cursor != ix {
                            s.cursor = ix;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.select(ix, cx);
                    }))
                    .child(div().w_full().h_full().flex().items_center().child(label))
                    .into_any_element(),
            );
        }
        div()
            .id("model-surface")
            .test_support()
            .w(px(300.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                .blur_radius(px(20.)),
            ])
            .on_mouse_down_out(cx.listener(move |s, event: &MouseDownEvent, _, cx| {
                if s.open
                    && !trigger_bounds
                        .get()
                        .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    s.close(cx);
                }
            }))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|s, _: &NextModel, _, cx| s.move_cursor(true, cx)))
            .on_action(cx.listener(|s, _: &PreviousModel, _, cx| s.move_cursor(false, cx)))
            .child(
                div()
                    .id("model-search")
                    .test_support()
                    .h(px(28.))
                    .px(px(12.))
                    .text_size(px(11.))
                    .line_height(px(13.))
                    .bg(rgb(if self.dark { 0x080b11 } else { 0xebeef0 }))
                    // GPUI's overflow clip is rectangular; mirror the outer surface's corners.
                    .rounded_tl(px(8.))
                    .rounded_tr(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .key_context("NativeModelsSearch")
                            .child(Input::new(&self.search)),
                    ),
            )
            .child(
                div()
                    .id("model-list")
                    .test_support()
                    .px(px(4.))
                    .pb(px(4.))
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .when(visible.is_empty(), |d| {
                        d.child(
                            div()
                                .id("empty")
                                .test_support()
                                .h(px(48.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(10.))
                                .text_color(rgb(t.muted))
                                .child("No matching models"),
                        )
                    })
                    .children(rows),
            )
    }
    pub fn search_editor(&self) -> Entity<InputState> {
        self.search.clone()
    }
    pub fn new(
        options: Vec<ModelOption>,
        selected: impl Into<String>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.bind_keys([
            gpui_kit::KeyBinding::new("down", NextModel, Some("NativeModelsSearch > Input")),
            gpui_kit::KeyBinding::new("up", PreviousModel, Some("NativeModelsSearch > Input")),
        ]);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        search.update(cx, |s, _| s.set_editor_style(support::input_style(dark)));
        cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = input.read(cx).value().to_string();
                let visible = this.visible();
                if !visible.contains(&this.cursor) {
                    this.cursor = visible.first().copied().unwrap_or(0);
                }
                cx.notify();
            }
        })
        .detach();
        Self {
            editor_style: support::input_style(dark),
            options,
            dark,
            open: false,
            selected: selected.into(),
            cursor: 0,
            query: String::new(),
            search,
            trigger_bounds: Rc::new(Cell::new(None)),
        }
    }

    pub fn selected(&self) -> &str {
        &self.selected
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn visible_options(&self) -> Vec<&ModelOption> {
        self.visible()
            .into_iter()
            .map(|i| &self.options[i])
            .collect()
    }
    pub fn cursor(&self) -> Option<&ModelOption> {
        self.open
            .then(|| self.visible().contains(&self.cursor))
            .filter(|v| *v)
            .map(|_| &self.options[self.cursor])
    }
    pub fn set_options(&mut self, options: Vec<ModelOption>, cx: &mut Context<Self>) {
        self.options = options;
        self.cursor = self
            .options
            .iter()
            .position(|o| o.id == self.selected)
            .unwrap_or(0);
        cx.notify();
    }
    pub fn set_selected(&mut self, id: impl Into<String>, cx: &mut Context<Self>) {
        self.selected = id.into();
        cx.notify();
    }
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        if let Some(option) = self.options.get(ix) {
            self.selected = option.id.clone();
            cx.emit(Event::Selected(self.selected.clone()));
            self.close(cx);
        }
    }
}
impl Render for ModelSelector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let trigger = Button::new("trigger")
            .h(px(22.))
            .pl(px(5.))
            .pr(px(24.))
            .relative()
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(if self.open { t.accent } else { t.canvas }))
            .bg(rgb(t.canvas))
            .hover(move |button| button.bg(rgb(t.hover)))
            .text_size(px(10.))
            .text_color(rgb(if self.open { t.text } else { t.muted }))
            .child(
                self.options
                    .iter()
                    .find(|o| o.id == self.selected)
                    .map(|o| o.label.clone())
                    .unwrap_or_default(),
            )
            .child(support::selector_arrow(if self.open {
                t.text
            } else {
                t.muted
            }))
            .on_click(cx.listener(|s, _, w, cx| {
                if s.open {
                    s.close(cx);
                } else {
                    s.open = true;
                    s.cursor = s
                        .options
                        .iter()
                        .position(|o| o.id == s.selected)
                        .unwrap_or(0);
                    s.search.update(cx, |input, cx| {
                        input.set_value("", w, cx);
                        input.focus(w, cx);
                    });
                    cx.notify();
                }
            }));
        let bounds = self.trigger_bounds.clone();
        Popup::new(
            "model-popup",
            div()
                .relative()
                .on_prepaint(move |rect, _, _| bounds.set(Some(rect)))
                .child(trigger),
        )
        .anchor(Anchor::BottomLeft)
        .offset(px(5.))
        .when(self.open, |p| p.content(self.content(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{MouseButton, TestApp, TestAppWindow, test::TestWindowExt};

    fn fixture() -> (TestApp, TestAppWindow<ModelSelector>) {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let options = ["alpha", "beta", "gamma"]
            .into_iter()
            .enumerate()
            .map(|(i, id)| ModelOption {
                id: id.into(),
                group: if i < 2 { "A" } else { "B" }.into(),
                label: format!("Model {id}"),
                element_id: format!("row-{id}"),
            })
            .collect();
        let mut window = app.open_window_with_options(Default::default(), |window, cx| {
            ModelSelector::new(options, "alpha", false, window, cx)
        });
        window.draw();
        (app, window)
    }

    fn click(window: &mut TestAppWindow<ModelSelector>, id: &str) {
        window.draw();
        let position = window.update(|_, window, _| window.find(id.to_owned()).bounds().center());
        window.simulate_click(position, MouseButton::Left);
        window.draw();
    }

    struct InsetHost {
        selector: gpui_kit::Entity<ModelSelector>,
    }

    impl Render for InsetHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().relative().size_full().child(
                div()
                    .absolute()
                    .left(px(220.))
                    .top(px(398.))
                    .child(self.selector.clone()),
            )
        }
    }

    #[test]
    fn popup_tracks_inset_trigger_and_dismisses_outside() {
        let mut app = TestApp::new();
        app.update(|cx| super::super::init(cx, false));
        let mut window = app.open_window_with_options(Default::default(), |window, cx| InsetHost {
            selector: cx.new(|cx| {
                ModelSelector::new(
                    vec![ModelOption {
                        id: "alpha".into(),
                        group: "A".into(),
                        label: "Alpha".into(),
                        element_id: "row-alpha".into(),
                    }],
                    "alpha",
                    false,
                    window,
                    cx,
                )
            }),
        });
        window.draw();
        let trigger = window.update(|_, w, _| w.find("trigger".to_owned()).bounds());
        window.simulate_click(trigger.center(), MouseButton::Left);
        window.draw();
        let surface = window.update(|_, w, _| w.find("model-surface".to_owned()).bounds());
        assert!(trigger.left() >= px(220.));
        assert_eq!(surface.left(), trigger.left());
        assert_eq!(surface.bottom(), trigger.top() - px(5.));
        assert_eq!(
            window.read(|host, cx| host.selector.read(cx).trigger_bounds.get()),
            Some(trigger)
        );
        window.simulate_click(gpui_kit::point(px(20.), px(20.)), MouseButton::Left);
        window.draw();
        assert!(!window.read(|host, cx| host.selector.read(cx).is_open()));
    }

    #[test]
    fn selection_uses_stable_identity_across_options_and_dismissal_preserves_selection() {
        let (_app, mut window) = fixture();
        click(&mut window, "trigger");
        assert!(window.read(|selector, _| selector.is_open()));
        click(&mut window, "model-search");
        assert!(window.read(|selector, _| selector.is_open()));
        click(&mut window, "row-beta");
        assert_eq!(
            window.read(|selector, _| selector.selected().to_owned()),
            "beta"
        );
        assert!(!window.read(|selector, _| selector.is_open()));
        window.update(|selector, _, cx| {
            selector.set_options(
                vec![
                    ModelOption {
                        id: "beta".into(),
                        group: "B".into(),
                        label: "Renamed".into(),
                        element_id: "row-beta".into(),
                    },
                    ModelOption {
                        id: "alpha".into(),
                        group: "A".into(),
                        label: "Other".into(),
                        element_id: "row-alpha".into(),
                    },
                ],
                cx,
            )
        });
        click(&mut window, "trigger");
        assert_eq!(
            window.read(|selector, _| selector.cursor().map(|o| o.id.clone())),
            Some("beta".into())
        );
        window.simulate_click(gpui_kit::point(px(400.), px(400.)), MouseButton::Left);
        window.draw();
        assert!(!window.read(|selector, _| selector.is_open()));
        click(&mut window, "trigger");
        click(&mut window, "trigger");
        assert!(!window.read(|selector, _| selector.is_open()));
        click(&mut window, "trigger");
        window.simulate_keystroke("escape");
        window.draw();
        assert!(!window.read(|selector, _| selector.is_open()));
        assert_eq!(
            window.read(|selector, _| selector.selected().to_owned()),
            "beta"
        );
    }
}
