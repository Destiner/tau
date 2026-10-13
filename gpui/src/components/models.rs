use super::support;
use gpui_kit::{
    Anchor, Bounds, BoxShadow, Context, Entity, EntityInputHandler, FocusHandle, IntoElement,
    KeyDownEvent, MouseDownEvent, Pixels, Render, Window,
    base::{
        Button, Input, Popup, TestSupportExt,
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
    disabled: bool,
    selected: String,
    fallback: String,
    available_width: Pixels,
    cursor: usize,
    query: String,
    search: Entity<InputState>,
    menu_focus: FocusHandle,
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
    fn move_cursor(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open
            || self
                .search
                .update(cx, |s, cx| s.marked_text_range(window, cx).is_some())
        {
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
    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open
            || self
                .search
                .update(cx, |s, cx| s.marked_text_range(window, cx).is_some())
        {
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
    fn content(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let trigger_bounds = self.trigger_bounds.clone();
        let visible = self.visible();
        let height = px(320.).min((window.viewport_size().height - px(120.)).max(px(0.)));
        let searchable = !self.options.is_empty();
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
                        .flex_shrink_0()
                        .mr(px(10.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .text_size(px(10.))
                        .text_color(rgb(t.faint))
                        .child(div().w_full().min_w_0().truncate().child(group))
                        .into_any_element(),
                );
            }
            rows.push(
                Button::new(option.element_id.clone())
                    .w_full()
                    .h(px(28.))
                    .flex_shrink_0()
                    .min_w_0()
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
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .child(div().w_full().truncate().child(label)),
                    )
                    .into_any_element(),
            );
        }
        div()
            .id("model-surface")
            .test_support()
            .w(px(300.).min((window.viewport_size().width - px(18.)).max(px(0.))))
            .max_h(height)
            .flex()
            .flex_col()
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
            .when(!searchable, |d| d.track_focus(&self.menu_focus))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|s, _: &NextModel, w, cx| s.move_cursor(true, w, cx)))
            .on_action(cx.listener(|s, _: &PreviousModel, w, cx| s.move_cursor(false, w, cx)))
            .when(searchable, |surface| {
                surface.child(
                    div()
                        .id("model-search")
                        .test_support()
                        .h(px(28.).min(height))
                        .flex_shrink_0()
                        .overflow_hidden()
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
            })
            .child(
                div()
                    .id("model-list")
                    .test_support()
                    .px(px(4.))
                    .pb(px(4.))
                    .max_h((height - px(if searchable { 28. } else { 0. })).max(px(0.)))
                    .min_h_0()
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
                                .child(if self.options.is_empty() {
                                    "No models available"
                                } else {
                                    "No matching models"
                                }),
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
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .accessibility_label("Search Models")
                .placeholder("Search")
        });
        search.update(cx, |s, _| {
            s.set_protect_ime_keys(true);
            s.set_editor_style(support::input_style(dark));
        });
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
            disabled: false,
            selected: selected.into(),
            fallback: String::new(),
            available_width: px(210.),
            cursor: 0,
            query: String::new(),
            search,
            menu_focus: cx.focus_handle(),
            trigger_bounds: Rc::new(Cell::new(None)),
        }
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        if disabled {
            self.close(cx);
        }
        cx.notify();
    }
    /// Standalone hosts provide their measured main-pane width, not window width.
    pub fn set_available_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        self.available_width = width.max(px(0.));
        cx.notify();
    }
    pub fn set_fallback(&mut self, label: impl Into<String>, cx: &mut Context<Self>) {
        self.fallback = label.into();
        cx.notify();
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
        let cursor_id = if self.open {
            self.options.get(self.cursor).map(|o| o.id.clone())
        } else {
            Some(self.selected.clone())
        };
        self.options = options;
        self.cursor = self
            .options
            .iter()
            .position(|o| cursor_id.as_ref().is_some_and(|id| &o.id == id))
            .filter(|index| self.visible().contains(index))
            .or_else(|| self.visible().into_iter().next())
            .unwrap_or(0);
        cx.notify();
    }
    pub fn set_selected(&mut self, id: impl Into<String>, cx: &mut Context<Self>) {
        self.selected = id.into();
        cx.notify();
    }
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.open || self.disabled {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let open = self.open;
        let pane_width = self.available_width;
        let trigger_bounds = self.trigger_bounds.clone();
        let trigger = Button::new("trigger")
            .disabled(self.disabled)
            .styles(|styles| styles.disabled(|b| b.opacity(0.45)))
            .max_w(px(210.).min(pane_width))
            .min_w_0()
            .h(px(22.))
            .pl(px(5.))
            .pr(px(24.))
            .relative()
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(if self.open { t.accent } else { t.canvas }))
            .bg(rgb(t.canvas))
            .when(self.open, |b| {
                b.shadow(super::composer::retry::glow(t.accent))
            })
            .when(!self.disabled, |b| {
                b.hover(move |button| {
                    button.bg(rgb(t.hover)).border_color(rgb(if open {
                        t.accent
                    } else {
                        t.border
                    }))
                })
            })
            .text_size(px(10.))
            .text_color(rgb(if self.open { t.text } else { t.muted }))
            .child(
                div().min_w_0().truncate().child(
                    self.options
                        .iter()
                        .find(|o| o.id == self.selected)
                        .map(|o| o.label.clone())
                        .unwrap_or_else(|| self.fallback.clone()),
                ),
            )
            .child(
                support::selector_arrow(if self.open { t.text } else { t.muted })
                    .id("model-selector-chevron")
                    .test_support()
                    .opacity(if self.disabled { 0.45 } else { 1. }),
            )
            .on_click(cx.listener(|s, _, w, cx| {
                if s.disabled {
                    return;
                }
                if s.open {
                    s.close(cx);
                } else {
                    s.open = true;
                    s.cursor = s
                        .options
                        .iter()
                        .position(|o| o.id == s.selected)
                        .unwrap_or(0);
                    let searchable = !s.options.is_empty();
                    s.search.update(cx, |input, cx| {
                        input.set_value("", w, cx);
                        if searchable {
                            input.focus(w, cx);
                        }
                    });
                    if !searchable {
                        s.menu_focus.focus(w, cx);
                    }
                    cx.notify();
                }
            }));
        let open_below = self.trigger_bounds.get().is_some_and(|bounds| {
            let above = bounds.top() - px(8.);
            let below = window.viewport_size().height - bounds.bottom() - px(8.);
            above < px(320.) && below > above
        });
        Popup::new(
            "model-popup",
            support::observe_bounds(trigger, move |bounds, _, _| {
                trigger_bounds.set(Some(bounds));
            }),
        )
        .anchor(if open_below {
            Anchor::TopLeft
        } else {
            Anchor::BottomLeft
        })
        .offset(px(5.))
        .when(self.open, |p| p.content(self.content(window, cx)))
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
        window.simulate_mouse_move(position);
        window.draw();
        window.simulate_mouse_down(position, MouseButton::Left);
        window.draw();
        window.simulate_mouse_up(position, MouseButton::Left);
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
    fn long_standalone_trigger_caps_clips_and_disabled_empty_picker_is_inert() {
        let (_app, mut window) = fixture();
        window.update(|s, _, cx| {
            s.options[0].label = "UnbrokenModel".repeat(40);
            s.set_available_width(px(70.), cx);
        });
        window.draw();
        window.update(|_, w, _| assert!(w.find("trigger").bounds().size.width <= px(70.)));
        click(&mut window, "trigger");
        window.update(|s, _, cx| {
            s.set_disabled(true, cx);
            s.select(1, cx);
        });
        assert!(!window.read(|s, _| s.is_open()));
        assert_eq!(window.read(|s, _| s.selected().to_owned()), "alpha");
        click(&mut window, "trigger");
        assert!(!window.read(|s, _| s.is_open()));
        window.update(|s, _, cx| {
            s.set_disabled(false, cx);
            s.set_options(vec![], cx);
            s.set_fallback("Outside Model", cx);
        });
        click(&mut window, "trigger");
        window.update(|_, w, _| {
            assert!(w.try_find("empty").is_some());
            assert!(w.try_find("model-search").is_none());
        });
        window.simulate_keystroke("enter");
        assert_eq!(window.read(|s, _| s.selected().to_owned()), "alpha");
    }

    #[test]
    fn open_catalogue_reorder_retains_cursor_identity_query_and_marked_search() {
        let (_app, mut window) = fixture();
        click(&mut window, "trigger");
        window.simulate_keystroke("down");
        window.update(|s, w, cx| {
            assert_eq!(s.cursor().map(|o| o.id.as_str()), Some("beta"));
            s.search.update(cx, |search, cx| {
                search.replace_and_mark_text_in_range(None, "model", Some(2..4), w, cx);
            });
        });
        window.draw();
        window.update(|s, w, cx| {
            let editor = s.search.clone();
            let before = editor.update(cx, |e, cx| {
                (
                    e.marked_text_range(w, cx),
                    e.selected_text_range(false, w, cx)
                        .map(|x| (x.range, x.reversed)),
                )
            });
            let mut options = s.options.clone();
            options.swap(0, 1);
            s.set_options(options, cx);
            assert_eq!(s.cursor().map(|o| o.id.as_str()), Some("beta"));
            assert_eq!(
                s.query(),
                "",
                "marked text is not a committed Vue v-model query"
            );
            assert_eq!(editor.read(cx).value().as_ref(), "model");
            assert_eq!(s.search, editor);
            let after = editor.update(cx, |e, cx| {
                (
                    e.marked_text_range(w, cx),
                    e.selected_text_range(false, w, cx)
                        .map(|x| (x.range, x.reversed)),
                )
            });
            assert_eq!(after, before);
        });
        for key in ["down", "up", "enter", "escape"] {
            window.simulate_keystroke(key);
        }
        assert!(window.read(|s, _| s.is_open()));
        assert_eq!(window.read(|s, _| s.selected().to_owned()), "alpha");
        window.update(|s, w, cx| {
            s.search.update(cx, |search, cx| {
                search.replace_text_in_range(Some(0..5), "model beta", w, cx);
            });
        });
        window.draw();
        window.update(|s, w, cx| {
            assert_eq!(s.query(), "model beta");
            assert!(
                s.search
                    .update(cx, |search, cx| search.marked_text_range(w, cx).is_none())
            );
            assert_eq!(s.visible().len(), 1);
            assert_eq!(s.cursor().map(|o| o.id.as_str()), Some("beta"));
        });
        window.simulate_keystroke("enter");
        assert_eq!(window.read(|s, _| s.selected().to_owned()), "beta");
        assert!(!window.read(|s, _| s.is_open()));
    }

    #[test]
    fn standalone_search_ime_never_moves_or_selects_a_model() {
        let (_app, mut window) = fixture();
        click(&mut window, "trigger");
        window.update(|s, w, cx| {
            s.search.update(cx, |search, cx| {
                search.replace_and_mark_text_in_range(None, "model", Some(5..5), w, cx)
            });
            let cursor = s.cursor;
            s.move_cursor(true, w, cx);
            assert_eq!(s.cursor, cursor);
        });
        for key in ["down", "up", "enter", "escape"] {
            window.simulate_keystroke(key);
        }
        assert!(window.read(|s, _| s.is_open()));
        assert_eq!(window.read(|s, _| s.selected().to_owned()), "alpha");
    }

    #[test]
    fn production_chevron_geometry_keeps_trigger_hit_area_in_both_themes_and_open_states() {
        let (_app, mut window) = fixture();
        let before = window.update(|_, w, _| w.find("trigger").bounds());
        for dark in [false, true] {
            for open in [false, true] {
                window.update(|selector, w, cx| {
                    selector.set_theme(dark, w, cx);
                    selector.open = open;
                    cx.notify();
                });
                window.draw();
                window.update(|_, w, _| {
                    let trigger = w.find("trigger").bounds();
                    let arrow = w.find("model-selector-chevron").bounds();
                    assert_eq!(trigger, before, "glyph must not change the hit area");
                    assert_eq!(trigger.size.height, px(22.));
                    let extent = px(6. * std::f32::consts::SQRT_2);
                    let tolerance = px(0.5 / w.scale_factor() + 0.02);
                    assert!((arrow.size.width - w.pixel_snap(extent)).abs() < px(0.01));
                    assert!((arrow.size.height - w.pixel_snap(extent)).abs() < px(0.01));
                    assert!((arrow.center().x - trigger.right() + px(12.)).abs() < tolerance);
                    assert!((arrow.center().y - trigger.center().y + px(1.)).abs() < tolerance);
                });
            }
        }
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
