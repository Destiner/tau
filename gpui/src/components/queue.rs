use super::{
    markdown::{self, MarkdownPresentation},
    support::{self, ButtonVariant},
};
use gpui_kit::{
    Bounds, BoxShadow, Context, FocusHandle, IntoElement, KeyDownEvent, Pixels, Render, Task,
    TestSupportExt, TextRun, Window,
    base::{Button, ElementExt, TextViewState},
    div, font,
    prelude::*,
    px, relative, rgb, rgba,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

struct QueueHint {
    dark: bool,
}
impl Render for QueueHint {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        div()
            .id("queue-clear-tooltip")
            .test_support()
            .px(px(7.))
            .py(px(3.))
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .text_color(rgb(t.text))
            .text_size(px(10.))
            .child("Clear All")
    }
}

fn preview_bottom(host: gpui_kit::Bounds<Pixels>, anchor_top: Pixels) -> Pixels {
    host.bottom() - anchor_top + px(5.)
}

pub struct MessageQueue {
    link_context: Option<std::sync::Arc<markdown::LinkContextHandler>>,
    link_anchor: Option<gpui_kit::Point<Pixels>>,
    id: String,
    dark: bool,
    props: QueueProps,
    details: bool,
    preview: Option<(String, String)>,
    trigger: Option<String>,
    anchor: Option<Bounds<Pixels>>,
    host_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pill_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    hovered: Option<String>,
    preview_hovered: bool,
    pending: usize,
    timer: Option<Task<()>>,
    close_timer: Option<Task<()>>,
    markdown: gpui_kit::Entity<TextViewState>,
    presentation: gpui_kit::Entity<MarkdownPresentation>,
    _presentation_changed: gpui_kit::Subscription,
    chip_focus: Vec<FocusHandle>,
    preview_focus: FocusHandle,
    review_focus: FocusHandle,
    recover_focus: FocusHandle,
    focus_events: Vec<gpui_kit::Subscription>,
    focus_window: Option<gpui_kit::AnyWindowHandle>,
    preview_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    suppress_preview_focus: bool,
}
impl MessageQueue {
    fn show(&mut self, id: String, kind: String, text: String, cx: &mut Context<Self>) {
        self.pending += 1;
        self.timer = None;
        self.close_timer = None;
        self.anchor = self.pill_bounds.borrow().get(&id).copied();
        self.trigger = Some(id);
        self.markdown
            .update(cx, |view, cx| view.set_text(&text, cx));
        self.presentation.update(cx, |p, cx| p.source_changed(cx));
        self.preview_bounds.set(None);
        self.preview = Some((kind, text));
        cx.notify();
    }
    fn show_later(&mut self, id: String, kind: String, text: String, cx: &mut Context<Self>) {
        if self.hovered.as_deref() == Some(id.as_str()) {
            return;
        }
        self.hovered = Some(id.clone());
        self.pending += 1;
        self.timer = None;
        self.close_timer = None;
        if self.trigger.as_deref() == Some(id.as_str()) && self.preview.is_some() {
            return;
        }
        let version = self.pending;
        let bounds = self.pill_bounds.clone();
        self.timer = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |scene, cx| {
                    if scene.pending == version && scene.hovered.as_deref() == Some(id.as_str()) {
                        scene.anchor = bounds.borrow().get(&id).copied();
                        scene.trigger = Some(id);
                        scene
                            .markdown
                            .update(cx, |view, cx| view.set_text(&text, cx));
                        scene.presentation.update(cx, |p, cx| p.source_changed(cx));
                        scene.preview_bounds.set(None);
                        scene.preview = Some((kind, text));
                        cx.notify();
                    }
                });
            }
        }));
    }
    fn leave(&mut self, id: &str, window: &Window, cx: &mut Context<Self>) {
        if self.hovered.as_deref() != Some(id) {
            return;
        }
        // Hover uses the clipped hitbox, not the outer border bounds.
        self.hovered = None;
        if !self.preview_hovered
            && !self.chip_focus.iter().any(|f| f.is_focused(window))
            && !self.preview_focus.contains_focused(window, cx)
        {
            self.leave_preview(cx);
        }
    }
    fn leave_preview(&mut self, cx: &mut Context<Self>) {
        self.pending += 1;
        self.timer = None;
        self.close_timer = None;
        let version = self.pending;
        self.close_timer = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |scene, cx| {
                    if scene.pending == version {
                        scene.hide(cx);
                    }
                });
            }
        }));
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "escape" {
            return;
        }
        if self.preview.is_some() {
            // Returning focus must not reopen the preview just dismissed.
            if let Some(id) = &self.trigger {
                let index = if id.starts_with("steer") {
                    id.strip_prefix("steer-")
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(1)
                        - 1
                } else {
                    self.props.steering.len()
                        + id.strip_prefix("follow-up-")
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(1)
                        - 1
                };
                if let Some(focus) = self.chip_focus.get(index)
                    && !focus.is_focused(window)
                {
                    self.suppress_preview_focus = true;
                    focus.focus(window, cx);
                }
            }
            self.hide(cx);
            cx.stop_propagation();
        } else if self.details {
            self.details = false;
            self.recover_focus.focus(window, cx);
            cx.notify();
            cx.stop_propagation();
        }
    }
    fn hide(&mut self, cx: &mut Context<Self>) {
        self.pending += 1;
        self.timer = None;
        self.close_timer = None;
        self.trigger = None;
        self.hovered = None;
        self.preview_hovered = false;
        self.preview = None;
        cx.notify();
    }
}
#[derive(Clone)]
pub struct QueueProps {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
    pub failed: Vec<String>,
    pub feedback: String,
    pub busy: bool,
    pub can_recover: bool,
}
impl Default for QueueProps {
    fn default() -> Self {
        Self {
            steering: vec![],
            follow_up: vec![],
            failed: vec![],
            feedback: String::new(),
            busy: false,
            can_recover: true,
        }
    }
}

#[derive(Clone, Debug)]
pub enum QueueEvent {
    Clear,
    Dismiss,
    Recover(usize),
    OpenLink(markdown::LinkTarget),
}
impl gpui_kit::EventEmitter<QueueEvent> for MessageQueue {}

impl MessageQueue {
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
    ) {
        self.link_context = Some(std::sync::Arc::new(handler));
    }
    pub fn link_anchor(&self) -> Option<gpui_kit::Point<Pixels>> {
        self.link_anchor
    }
    pub fn set_copy_paths(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.presentation
            .update(cx, |p, cx| p.set_copy_paths(enabled, cx));
    }
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        self.presentation.update(cx, |p, cx| p.set_theme(dark, cx));
        cx.notify();
    }

    pub fn new(
        id: impl Into<String>,
        props: QueueProps,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let chip_focus = (0..props.steering.len() + props.follow_up.len())
            .map(|_| cx.focus_handle().tab_stop(true))
            .collect();
        let markdown =
            cx.new(|cx| TextViewState::markdown_with_extensions("", markdown::extensions(), cx));
        let owner = cx.entity().downgrade();
        let presentation = cx.new(|cx| {
            MarkdownPresentation::new(
                markdown.clone(),
                dark,
                move |cx| {
                    let _ = owner.update(cx, |_, cx| cx.notify());
                },
                cx,
            )
        });
        let changed = cx.observe(&presentation, |_, _, cx| cx.notify());
        Self {
            id: id.into(),
            dark,
            props,
            details: false,
            preview: None,
            link_context: None,
            link_anchor: None,
            trigger: None,
            anchor: None,
            host_bounds: Rc::new(Cell::new(None)),
            pill_bounds: Rc::new(RefCell::new(HashMap::new())),
            hovered: None,
            preview_hovered: false,
            pending: 0,
            timer: None,
            close_timer: None,
            markdown,
            presentation,
            _presentation_changed: changed,
            chip_focus,
            preview_focus: cx.focus_handle(),
            review_focus: cx.focus_handle(),
            recover_focus: cx.focus_handle(),
            focus_events: vec![],
            focus_window: None,
            preview_bounds: Rc::new(Cell::new(None)),
            suppress_preview_focus: false,
        }
    }
    pub fn props(&self) -> &QueueProps {
        &self.props
    }
    pub fn set_props(&mut self, props: QueueProps, cx: &mut Context<Self>) {
        if self.props.steering != props.steering || self.props.follow_up != props.follow_up {
            self.hide(cx);
            self.pill_bounds.borrow_mut().clear();
            self.focus_events.clear();
            self.chip_focus = (0..props.steering.len() + props.follow_up.len())
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect();
        }
        if props.failed.is_empty() {
            self.details = false;
        }
        self.props = props;
        cx.notify();
    }
    pub fn preview(&self) -> Option<&(String, String)> {
        self.preview.as_ref()
    }
    pub fn trigger(&self) -> Option<&str> {
        self.trigger.as_deref()
    }
    pub fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.props.failed.is_empty() {
            return;
        }
        self.hide(cx);
        self.details = true;
        self.review_focus.focus(window, cx);
        cx.notify();
    }
    pub fn details_open(&self) -> bool {
        self.details
    }
    pub fn focus_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.chip_focus.get(self.props.steering.len()) {
            focus.focus(window, cx);
            self.show(
                "follow-up".into(),
                "Follow Up".into(),
                self.props.follow_up[0].clone(),
                cx,
            );
        }
    }
    pub fn focused_follow_up(&self, window: &Window) -> bool {
        self.chip_focus
            .get(self.props.steering.len())
            .is_some_and(|focus| focus.is_focused(window))
    }
}
impl Render for MessageQueue {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        if self.focus_window != Some(window.window_handle()) {
            self.focus_events.clear();
            self.focus_window = Some(window.window_handle());
        }
        if self.focus_events.is_empty() {
            for (index, text) in self
                .props
                .steering
                .iter()
                .chain(&self.props.follow_up)
                .cloned()
                .enumerate()
            {
                let steering = index < self.props.steering.len();
                let ordinal = if steering {
                    index + 1
                } else {
                    index - self.props.steering.len() + 1
                };
                let base = if steering { "steer" } else { "follow-up" };
                let id = if ordinal == 1 {
                    base.into()
                } else {
                    format!("{base}-{ordinal}")
                };
                let kind = if steering { "Steering" } else { "Follow Up" }.to_owned();
                let focus = self.chip_focus[index].clone();
                self.focus_events
                    .push(cx.on_focus(&focus, window, move |s, _, cx| {
                        if s.suppress_preview_focus {
                            s.suppress_preview_focus = false;
                        } else {
                            s.show(id.clone(), kind.clone(), text.clone(), cx);
                        }
                    }));
                self.focus_events
                    .push(cx.on_blur(&focus, window, |s, window, cx| {
                        if !s.chip_focus.iter().any(|f| f.is_focused(window))
                            && !s.preview_focus.contains_focused(window, cx)
                        {
                            s.leave_preview(cx);
                        }
                    }));
            }
            self.focus_events
                .push(cx.on_focus_in(&self.preview_focus, window, |s, _, _cx| {
                    s.pending += 1;
                    s.close_timer = None;
                }));
            self.focus_events.push(cx.on_focus_out(
                &self.preview_focus,
                window,
                |s, _, window, cx| {
                    if !s.chip_focus.iter().any(|f| f.is_focused(window)) {
                        s.leave_preview(cx);
                    }
                },
            ));
        }
        let chip_max_width = if self.props.steering.len() + self.props.follow_up.len() == 1 {
            let text = self
                .props
                .steering
                .first()
                .or_else(|| self.props.follow_up.first())
                .unwrap();
            let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let text_width = window
                .text_system()
                .layout_line(
                    &label,
                    px(11.),
                    &[TextRun {
                        len: label.len(),
                        font: font(super::font::family()),
                        color: rgb(t.muted).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                )
                .width;
            ((text_width
                + px(12.
                    + if self.props.follow_up.is_empty() {
                        0.
                    } else {
                        22.
                    }))
                * 0.75)
                .min(px(180.))
        } else {
            px(180.)
        };
        let mut rail = div()
            .id("rail")
            .test_support()
            .flex()
            .gap(px(4.))
            .min_w(px(0.))
            .max_w_full()
            .overflow_x_scroll();
        let all = self
            .props
            .steering
            .iter()
            .map(|text| ("Steering", text))
            .chain(self.props.follow_up.iter().map(|text| ("Follow Up", text)))
            .enumerate();
        for (index, (kind, text)) in all {
            if index == self.props.steering.len()
                && !self.props.steering.is_empty()
                && !self.props.follow_up.is_empty()
            {
                rail = rail.child(
                    div()
                        .id("divider")
                        .test_support()
                        .self_center()
                        .h(px(14.))
                        .border_l_1()
                        .border_color(rgb(t.border)),
                );
            }
            let kind = kind.to_owned();
            let text = text.to_owned();
            let name = if kind == "Steering" {
                if index == 0 {
                    "steer".into()
                } else {
                    format!("steer-{}", index + 1)
                }
            } else if index == self.props.steering.len() {
                "follow-up".into()
            } else {
                format!("follow-up-{}", index - self.props.steering.len() + 1)
            };
            let ordinal = if kind == "Follow Up" {
                Some(index - self.props.steering.len() + 1)
            } else {
                None
            };
            let pill_bounds = self.pill_bounds.clone();
            let preview_visible = self.preview.is_some();
            let entity_id = cx.entity_id();
            rail = rail.child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .on_prepaint({
                        let name = name.clone();
                        move |bounds, _, cx| {
                            let changed =
                                pill_bounds.borrow_mut().insert(name, bounds) != Some(bounds);
                            if changed && preview_visible {
                                cx.notify(entity_id);
                            }
                        }
                    })
                    .child(
                        Button::new(name.clone())
                            .track_focus(&self.chip_focus[index])
                            .on_key_down(cx.listener(Self::key))
                            .h(px(22.))
                            .max_w(chip_max_width)
                            .overflow_hidden()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .px(px(5.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(if self.chip_focus[index].is_focused(window) {
                                t.accent
                            } else {
                                t.border
                            }))
                            .bg(rgb(t.panel))
                            .when(self.props.busy, |chip| chip.opacity(0.55))
                            .text_size(px(11.))
                            .hover(move |s| s.bg(rgb(t.hover)))
                            .on_hover(cx.listener({
                                let id = name.clone();
                                let kind = kind.clone();
                                let text = text.clone();
                                move |scene, hovered, window, cx| {
                                    if *hovered {
                                        scene.show_later(
                                            id.clone(),
                                            kind.clone(),
                                            text.clone(),
                                            cx,
                                        );
                                    } else {
                                        scene.leave(&id, window, cx);
                                    }
                                }
                            }))
                            .on_click(cx.listener({
                                let id = name.clone();
                                let kind = kind.clone();
                                let text = text.clone();
                                move |scene, _, _, cx| {
                                    scene.show(id.clone(), kind.clone(), text.clone(), cx);
                                }
                            }))
                            .when_some(ordinal, |chip, n| {
                                chip.child(
                                    div()
                                        .id(format!("ordinal-{n}"))
                                        .test_support()
                                        .min_w(px(14.))
                                        .when(n < 10, |badge| badge.w(px(14.)))
                                        .h(px(14.))
                                        .px(px(2.))
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(4.))
                                        .bg(rgb(t.hover))
                                        .text_size(px(10.))
                                        .text_color(rgb(t.muted))
                                        .child(n.to_string()),
                                )
                            })
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_decoration_none()
                                    .text_color(rgb(t.muted))
                                    .child(text.split_whitespace().collect::<Vec<_>>().join(" ")),
                            ),
                    ),
            );
        }
        let mut bar = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .p(px(4.))
            .text_size(px(11.));
        if !self.props.steering.is_empty() || !self.props.follow_up.is_empty() {
            let dark = self.dark;
            bar = bar.child(
                div()
                    .id("clear-tooltip-trigger")
                    .test_support()
                    .flex_shrink_0()
                    .tooltip_show_delay(Duration::from_millis(450))
                    .tooltip(move |_, cx| cx.new(|_| QueueHint { dark }).into())
                    .child(
                        Button::new("clear")
                            .accessibility_label("Clear All")
                            .disabled(self.props.busy)
                            .flex_shrink_0()
                            .size(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .when(!self.props.busy, |button| {
                                button.hover(move |s| s.bg(rgb(t.hover)))
                            })
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(QueueEvent::Clear);
                            }))
                            .child(
                                support::icon("cross", 12., t.muted)
                                    .when(self.props.busy, |icon| icon.opacity(0.45)),
                            ),
                    ),
            );
        }
        bar = bar.child(rail);
        if !self.props.feedback.is_empty() {
            bar = bar.child(
                div()
                    .id("feedback")
                    .test_support()
                    .min_w(px(0.))
                    .max_w((window.viewport_size().width * 0.35).min(px(220.)))
                    .truncate()
                    .text_color(rgb(t.muted))
                    .text_size(px(10.))
                    .child(self.props.feedback.clone()),
            );
        }
        if !self.props.feedback.is_empty() && !self.props.busy {
            bar = bar.child(
                Button::new("dismiss")
                    .accessibility_label("Dismiss queue status")
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .opacity(0.65)
                    .hover(|button| button.opacity(1.))
                    .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            cx.emit(QueueEvent::Dismiss);
                        }
                    }))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(QueueEvent::Dismiss);
                    }))
                    .child(support::icon("cross", 15., t.muted)),
            );
        }
        if !self.props.failed.is_empty() {
            bar = bar.child(
                Button::new("recover")
                    .track_focus(&self.recover_focus)
                    .flex_shrink_0()
                    .accessibility_label(format!("Review Unsent ({})", self.props.failed.len()))
                    .px(px(4.))
                    .py(px(2.))
                    .text_size(px(10.))
                    .text_color(rgb(t.muted))
                    .hover(move |button| button.bg(rgb(t.hover)))
                    .on_click(cx.listener(|s, _, window, cx| {
                        s.details = !s.details;
                        if s.details {
                            s.review_focus.focus(window, cx);
                        }
                        cx.notify();
                    }))
                    .child(format!("Review Unsent ({})", self.props.failed.len())),
            );
        }
        if self.details {
            let cap = (window.viewport_size().height * 0.36).min(px(260.));
            let below = self.host_bounds.get().is_some_and(|bounds| {
                bounds.top() < cap && window.viewport_size().height - bounds.bottom() > bounds.top()
            });
            let mut review = div()
                .id("review-panel")
                .test_support()
                .track_focus(&self.review_focus)
                .on_key_down(cx.listener(Self::key))
                .max_h(cap)
                .overflow_y_scroll()
                .occlude()
                .absolute()
                .when(below, |panel| panel.top(relative(1.)))
                .when(!below, |panel| panel.bottom(relative(1.)))
                .right(px(4.))
                .left(px(4.))
                .p(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.panel_raised))
                .flex()
                .flex_col()
                .shadow(vec![
                    BoxShadow::new(
                        px(0.),
                        px(4.),
                        rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                    )
                    .blur_radius(px(14.)),
                ])
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .child("Unsent messages"),
                );
            for (i, text) in self.props.failed.iter().enumerate() {
                review = review.child(
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(8.))
                        .px(px(6.))
                        .py(px(4.))
                        .text_color(rgb(t.muted))
                        .flex_shrink_0()
                        .child(div().min_w(px(0.)).flex_1().child(text.clone()))
                        .child(
                            support::button_sm(
                                if i == 0 {
                                    "restore".into()
                                } else {
                                    format!("restore-{i}")
                                },
                                "Restore Draft",
                                ButtonVariant::Ghost,
                                self.dark,
                            )
                            .backdrop(t.panel_raised)
                            .flex_shrink_0()
                            .disabled(!self.props.can_recover)
                            .on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
                                let k = &event.keystroke;
                                if k.key == "enter"
                                    && (k.modifiers.platform || k.modifiers.control)
                                    && !k.modifiers.alt
                                    && !k.modifiers.shift
                                    && !event.is_held
                                    && s.props.can_recover
                                {
                                    cx.stop_propagation();
                                    cx.emit(QueueEvent::Recover(i));
                                }
                            }))
                            .on_click(cx.listener(
                                move |s, _, _, cx| {
                                    if s.props.can_recover {
                                        cx.emit(QueueEvent::Recover(i));
                                    }
                                },
                            )),
                        ),
                );
            }
            if !self.props.can_recover {
                review = review.child(
                    div()
                        .id("recovery-hint")
                        .test_support()
                        .mx(px(6.))
                        .my(px(4.))
                        .text_color(rgb(t.muted))
                        .text_size(px(11.))
                        .child("Clear the composer to restore a draft."),
                );
            }
            review = review.child(div().flex().child(
                support::button_sm("close", "Close", ButtonVariant::Ghost, self.dark).on_click(
                    cx.listener(|s, _, window, cx| {
                        s.details = false;
                        s.recover_focus.focus(window, cx);
                        cx.notify();
                    }),
                ),
            ));
            bar = bar.child(gpui_kit::deferred(review).priority(100));
        }
        let host_bounds = self.host_bounds.clone();
        let preview_visible = self.preview.is_some();
        let entity_id = cx.entity_id();
        let mut root = div()
            .id(self.id.clone())
            .test_support()
            .relative()
            .w_full()
            .on_key_down(cx.listener(Self::key))
            .child(bar);
        if let Some((kind, _)) = &self.preview {
            let anchor = self
                .trigger
                .as_ref()
                .and_then(|id| self.pill_bounds.borrow().get(id).copied())
                .or(self.anchor);
            let host = self.host_bounds.get();
            let position = anchor.zip(host);
            let viewport = window.viewport_size();
            let max_width = (viewport.width - px(16.)).max(px(0.)).min(px(500.));
            let measured = self.preview_bounds.get();
            let width = measured.map_or(max_width, |b| b.size.width.min(max_width));
            let x = position.map_or(px(0.), |(b, host)| {
                b.left().min(viewport.width - width - px(8.)).max(px(8.)) - host.left()
            });
            let cap = (viewport.height * 0.45).min(px(340.));
            let above = anchor.map_or(cap, |b| b.top() - px(13.));
            let below = anchor.map_or(cap, |b| viewport.height - b.bottom() - px(13.));
            let open_below = measured.map_or(cap, |b| b.size.height) > above && below > above;
            let max_height = cap.min(if open_below { below } else { above }).max(px(24.));
            let bottom = position.map_or(px(0.), |(b, host)| preview_bottom(host, b.top()));
            let preview_bounds = self.preview_bounds.clone();
            root = root.child(support::observe_bounds(
                div()
                    .id("preview")
                    .test_support()
                    .track_focus(&self.preview_focus)
                    .on_key_down(cx.listener(Self::key))
                    .absolute()
                    .when(!open_below, |d| d.bottom(bottom))
                    .when(open_below, |d| {
                        d.top(position.map_or(px(0.), |(b, host)| b.bottom() - host.top() + px(5.)))
                    })
                    .left(x)
                    .on_hover(cx.listener(|scene, hovered, window, cx| {
                        scene.preview_hovered = *hovered;
                        if *hovered {
                            scene.pending += 1;
                            scene.timer = None;
                            scene.close_timer = None;
                        } else if scene.hovered.is_none()
                            && !scene.chip_focus.iter().any(|f| f.is_focused(window))
                            && !scene.preview_focus.contains_focused(window, cx)
                        {
                            scene.leave_preview(cx);
                        }
                    }))
                    .max_w(max_width)
                    .max_h(max_height)
                    .overflow_y_scroll()
                    .p(px(9.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.panel_raised))
                    .shadow(vec![
                        BoxShadow::new(
                            px(0.),
                            px(5.),
                            rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                        )
                        .blur_radius(px(16.)),
                    ])
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .text_size(px(11.))
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child(kind.clone()),
                    )
                    .child(
                        div().line_height(relative(1.62)).child(
                            MarkdownPresentation::decorate(
                                &self.presentation,
                                11.,
                                self.dark,
                                window,
                                cx,
                            )
                            .on_link_activate({
                                let entity = cx.entity().downgrade();
                                let preview = self.preview.clone();
                                move |link, event, _, cx| {
                                    let Some(target) = markdown::activation_target(link, event)
                                    else {
                                        return;
                                    };
                                    let _ = entity.update(cx, |queue, cx| {
                                        if queue.preview == preview {
                                            queue.link_anchor = Some(event.position());
                                            cx.emit(QueueEvent::OpenLink(target));
                                        }
                                    });
                                }
                            })
                            .on_link_context_menu({
                                let entity = cx.entity().downgrade();
                                let preview = self.preview.clone();
                                move |raw, event, window, cx| {
                                    let Some(target) = markdown::link_target(raw) else {
                                        return;
                                    };
                                    let handler = entity.upgrade().and_then(|entity| {
                                        let queue = entity.read(cx);
                                        (queue.preview == preview)
                                            .then(|| queue.link_context.clone())
                                            .flatten()
                                    });
                                    if let Some(handler) = handler {
                                        handler(target, event, window, cx);
                                    }
                                }
                            }),
                        ),
                    ),
                move |bounds, _, cx| {
                    if preview_bounds.replace(Some(bounds)) != Some(bounds) {
                        cx.notify(entity_id);
                    }
                },
            ));
        }
        support::observe_bounds(root, move |bounds, _, cx| {
            let changed = host_bounds.replace(Some(bounds)) != Some(bounds);
            if changed && preview_visible {
                cx.notify(entity_id);
            }
        })
    }
}

#[cfg(test)]
#[path = "queue_parity_tests.rs"]
mod parity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        TestApp, TestAppWindow, WindowBounds, WindowOptions, point, size, test::TestWindowExt,
    };

    struct QueueHost {
        queue: gpui_kit::Entity<MessageQueue>,
        left: Pixels,
        top: Pixels,
    }
    impl Render for QueueHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .absolute()
                    .left(self.left)
                    .top(self.top)
                    .w(px(640.))
                    .child(self.queue.clone()),
            )
        }
    }
    fn fixture_at(left: Pixels, top: Pixels) -> (TestApp, TestAppWindow<QueueHost>) {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window_with_options(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(px(900.), px(700.)),
                })),
                ..Default::default()
            },
            |_, cx| QueueHost {
                queue: cx.new(|cx| {
                    MessageQueue::new(
                        "queue-host",
                        QueueProps {
                            steering: vec!["Steer this turn".into()],
                            follow_up: vec![
                                "First follow up".into(),
                                "Second follow up".into(),
                                "Third follow up".into(),
                            ],
                            can_recover: true,
                            ..Default::default()
                        },
                        false,
                        cx,
                    )
                }),
                left,
                top,
            },
        );
        window.draw();
        (app, window)
    }
    fn fixture() -> (TestApp, TestAppWindow<QueueHost>) {
        fixture_at(px(130.), px(300.))
    }
    fn settle(app: &mut TestApp, window: &mut TestAppWindow<QueueHost>, ms: u64) {
        app.advance_clock(Duration::from_millis(ms));
        window.draw();
        app.run_until_parked();
    }
    #[test]
    fn clear_activation_tracks_busy_enabled_busy_again() {
        let (_app, mut window) = fixture();
        let clears = Rc::new(std::cell::Cell::new(0));
        let _subscription = window.update(|host, _, cx| {
            let clears = clears.clone();
            cx.subscribe(&host.queue, move |_, _, event: &QueueEvent, _| {
                if matches!(event, QueueEvent::Clear) {
                    clears.set(clears.get() + 1);
                }
            })
        });
        let geometry = window.update(|_, window, _| {
            ["queue-host", "clear", "steer", "follow-up", "follow-up-3"]
                .map(|id| window.find(id).bounds())
        });
        for busy in [true, false, true, false] {
            window.update(|host, _, cx| {
                host.queue.update(cx, |queue, cx| {
                    let mut props = queue.props().clone();
                    props.busy = busy;
                    queue.set_props(props, cx);
                });
            });
            window.draw();
            let center = window.update(|_, window, _| {
                assert!(window.try_find("feedback").is_none());
                assert_eq!(
                    ["queue-host", "clear", "steer", "follow-up", "follow-up-3"]
                        .map(|id| window.find(id).bounds()),
                    geometry,
                    "pending must not shift the queue"
                );
                window.find("clear").bounds().center()
            });
            window.simulate_mouse_move(center);
            let before = clears.get();
            for _ in 0..3 {
                window.simulate_click(center, gpui_kit::MouseButton::Left);
            }
            assert_eq!(clears.get() - before, 3 * usize::from(!busy));
            window.update(|host, _, cx| {
                let props = host.queue.read(cx).props();
                assert_eq!(props.steering, vec!["Steer this turn"]);
                assert_eq!(props.follow_up.len(), 3);
            });
        }
    }

    #[test]
    fn pending_items_remain_inspectable_and_failure_feedback_is_not_replaced() {
        let (_, mut window) = fixture();
        window.update(|host, _, cx| {
            host.queue.update(cx, |queue, cx| {
                let mut props = queue.props().clone();
                props.busy = true;
                props.feedback = "Could not clear messages. Try again.".into();
                props.failed = vec!["Keep this unsent draft".into()];
                queue.set_props(props, cx);
            });
        });
        window.draw();
        let chip = window.update(|_, window, _| {
            assert!(window.find("feedback").visible());
            assert!(window.find("recover").visible());
            assert_eq!(window.find("clear").disabled(), Some(true));
            window.find("follow-up").bounds()
        });
        window.simulate_mouse_move(chip.center());
        window.simulate_click(chip.center(), gpui_kit::MouseButton::Left);
        window.draw();
        window.read(|host, cx| {
            let queue = host.queue.read(cx);
            assert_eq!(
                queue.preview(),
                Some(&("Follow Up".into(), "First follow up".into()))
            );
            assert_eq!(
                queue.props().feedback,
                "Could not clear messages. Try again."
            );
            assert_eq!(queue.props().failed, vec!["Keep this unsent draft"]);
        });
        window.update(|host, _, cx| {
            host.queue.update(cx, |queue, cx| {
                let mut props = queue.props().clone();
                props.busy = false;
                queue.set_props(props, cx);
            });
        });
        window.draw();
        window.update(|_, window, _| {
            assert!(window.find("feedback").visible());
            assert!(window.find("dismiss").visible());
            assert_ne!(window.find("clear").disabled(), Some(true));
        });
    }

    #[test]
    fn slow_exit_through_each_chip_border_closes_preview() {
        for id in ["steer", "follow-up", "follow-up-2", "follow-up-3"] {
            for edge in 0..4 {
                let (mut app, mut window) = fixture();
                let pill = window.update(|_, window, _| window.find(id.to_owned()).bounds());
                window.simulate_mouse_move(pill.center());
                settle(&mut app, &mut window, 200);
                assert!(window.read(|host, cx| host.queue.read(cx).preview().is_some()));
                settle(&mut app, &mut window, 400);
                assert!(window.read(|host, cx| host.queue.read(cx).preview().is_some()));
                for step in 0..=24 {
                    let distance = px(2. - step as f32 * 0.125);
                    let point = match edge {
                        0 => point(pill.left() + distance, pill.center().y),
                        1 => point(pill.right() - distance, pill.center().y),
                        2 => point(pill.center().x, pill.top() + distance),
                        _ => point(pill.center().x, pill.bottom() - distance),
                    };
                    window.simulate_mouse_move(point);
                    settle(&mut app, &mut window, 10);
                }
                settle(&mut app, &mut window, 100);
                assert!(
                    window.read(|host, cx| host.queue.read(cx).preview().is_none()),
                    "{id} edge {edge}"
                );
            }
        }
    }
    #[test]
    fn preview_handoff_survives_listener_order_and_closes_after_exit() {
        for (left, top) in [
            (px(130.), px(300.)),
            (px(20.), px(450.)),
            (px(210.), px(220.)),
        ] {
            for cross_gap in [false, true] {
                let (mut app, mut window) = fixture_at(left, top);
                let pill = window.update(|_, window, _| window.find("follow-up").bounds());
                window.simulate_mouse_move(pill.center());
                settle(&mut app, &mut window, 200);
                let preview = window.update(|_, window, _| window.find("preview").bounds());
                window.update(|_, window, _| {
                    let viewport = window.viewport_size();
                    assert_eq!(pill.top() - preview.bottom(), px(5.));
                    assert!(preview.top() >= px(0.));
                    assert!(preview.left() >= px(0.));
                    assert!(preview.right() <= viewport.width);
                });
                if cross_gap {
                    window.simulate_mouse_move(point(pill.center().x, pill.top() - px(2.)));
                    settle(&mut app, &mut window, 30);
                }
                window.simulate_mouse_move(preview.center());
                settle(&mut app, &mut window, 300);
                assert!(window.read(|host, cx| host.queue.read(cx).preview().is_some()));
                window.simulate_mouse_move(point(preview.right() - px(0.5), preview.center().y));
                settle(&mut app, &mut window, 20);
                window.simulate_mouse_move(point(preview.right() + px(0.125), preview.center().y));
                settle(&mut app, &mut window, 100);
                assert!(window.read(|host, cx| host.queue.read(cx).preview().is_none()));
            }
        }
    }
    #[test]
    fn focused_follow_up_escape_dismisses_only_preview() {
        let (_, mut window) = fixture();
        window.update(|host, window, cx| {
            host.queue
                .update(cx, |queue, cx| queue.focus_follow_up(window, cx))
        });
        window.draw();
        assert!(window.update(|host, window, cx| host.queue.read(cx).focused_follow_up(window)));
        assert!(window.read(|host, cx| host.queue.read(cx).preview().is_some()));
        window.simulate_keystroke("escape");
        window.draw();
        assert!(window.read(|host, cx| host.queue.read(cx).preview().is_none()));
        assert!(window.update(|host, window, cx| host.queue.read(cx).focused_follow_up(window)));
    }
    #[test]
    fn rendered_preview_keeps_gap_when_frame_and_viewport_differ() {
        let (_app, mut window) = fixture();
        window.update(|host, window, cx| {
            window.resize(size(px(900.), px(732.)));
            assert_eq!(window.viewport_size().height, px(700.));
            host.queue.update(cx, |queue, cx| {
                queue.show("follow-up".into(), "Follow Up".into(), "Preview".into(), cx)
            });
        });
        window.draw();
        window.update(|_, window, _| {
            let pill = window.find("follow-up").bounds();
            let preview = window.find("preview").bounds();
            assert!((f32::from(pill.top() - preview.bottom()) - 5.).abs() < 0.01);
        });
    }
}
