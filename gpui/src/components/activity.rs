use super::{
    markdown::{self, MarkdownPresentation},
    support,
};
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, ScrollHandle, TestSupportExt,
    Window,
    base::{
        Button, ElementExt, InputBase, SelectionFormat, TextSelectionDomain,
        TextSelectionDomainRange, TextViewState, Textarea, input::TextareaState,
    },
    div,
    prelude::*,
    px, rgb,
};

pub struct Header<'a> {
    pub name: &'a str,
    pub argument: &'a str,
    pub open: bool,
    pub hovered: bool,
    pub expandable: bool,
    pub running: bool,
    pub failed: bool,
}

/// Callers own disclosure state and attach their own activation callback.
pub fn tool_header(
    id: impl Into<gpui_kit::ElementId>,
    header: Header<'_>,
    focus: &FocusHandle,
    window: &Window,
    dark: bool,
) -> Button {
    let Header {
        name,
        argument,
        open,
        hovered,
        expandable,
        running,
        failed,
    } = header;
    let t = support::tokens(dark);
    Button::new(id)
        .disabled(!expandable)
        .track_focus(focus)
        .w_full()
        .min_h(px(18.5))
        .flex()
        .items_center()
        .gap(px(8.))
        .group("activity-header")
        .px(px(6.))
        .py(px(3.))
        .rounded(px(4.))
        .hover(move |s| s.bg(rgb(t.hover)))
        .focus_visible(move |s| s.bg(rgb(t.hover)))
        .child(
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .font_family("Menlo")
                .text_size(px(10.))
                .line_height(gpui_kit::relative(1.25))
                .text_color(rgb(if expandable && hovered {
                    t.text
                } else {
                    t.muted
                }))
                .child(name.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .font_family("Menlo")
                .text_size(px(10.))
                .line_height(gpui_kit::relative(1.25))
                .text_color(rgb(t.faint))
                .child(argument.split_whitespace().collect::<Vec<_>>().join(" ")),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(12.))
                        .flex()
                        .justify_center()
                        .child(if expandable {
                            support::icon(
                                if open { "chevron-up" } else { "chevron-down" },
                                11.,
                                t.faint,
                            )
                            .opacity(
                                if focus.is_focused(window) && window.last_input_was_keyboard() {
                                    1.
                                } else {
                                    0.
                                },
                            )
                            .group_hover("activity-header", |s| s.opacity(1.))
                            .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .when(running || failed, |end| {
                    end.child(
                        div()
                            .w(px(if failed { 10. } else { 11. }))
                            .h(px(11.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if running {
                                div()
                                    .size(px(5.))
                                    .rounded_full()
                                    .bg(rgb(t.muted))
                                    .into_any_element()
                            } else {
                                support::icon("cross", 10., t.danger).into_any_element()
                            }),
                    )
                }),
        )
}

pub fn tool_details(
    id: impl Into<gpui_kit::ElementId>,
    arguments: &str,
    result: &str,
    failed: bool,
    dark: bool,
) -> impl IntoElement {
    tool_details_with_scroll(id, arguments, result, failed, dark, px(455.), None, None)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Details rendering shares caller-owned scroll and raw editor state"
)]
fn tool_details_with_scroll(
    id: impl Into<gpui_kit::ElementId>,
    arguments: &str,
    result: &str,
    failed: bool,
    dark: bool,
    max_height: gpui_kit::Pixels,
    scroll: Option<&ScrollHandle>,
    raw: Option<&[Entity<TextareaState>; 2]>,
) -> gpui_kit::AnyElement {
    let t = support::tokens(dark);
    let scroll = scroll.cloned();
    let viewport_height = std::rc::Rc::new(std::cell::Cell::new(px(0.)));
    let field_heights = std::rc::Rc::new(std::cell::Cell::new([px(0.); 2]));
    let measured_viewport = viewport_height.clone();
    let mut details = div()
        .id(id)
        .test_support()
        .ml(px(14.))
        .mt(px(4.))
        .mb(px(2.))
        .pl(px(10.))
        .border_l_1()
        .border_color(rgb(t.border))
        .flex()
        .flex_col()
        .gap(px(8.))
        .max_h(max_height)
        .overflow_y_scroll()
        .when_some(scroll.as_ref(), |details, scroll| {
            details.track_scroll(scroll)
        })
        .on_prepaint(move |bounds, _, _| measured_viewport.set(bounds.size.height));
    for (index, (label, value)) in [
        ("Arguments", arguments),
        (if failed { "Error" } else { "Result" }, result),
    ]
    .into_iter()
    .enumerate()
    {
        if value.is_empty() {
            continue;
        }
        let measured_fields = field_heights.clone();
        details = details.child(
            div()
                .on_prepaint(move |bounds, _, _| {
                    let mut heights = measured_fields.get();
                    heights[index] = bounds.size.height;
                    measured_fields.set(heights);
                })
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .font_family("Menlo")
                        .text_size(px(10.))
                        .line_height(gpui_kit::relative(1.25))
                        .text_color(rgb(t.faint))
                        .child(label),
                )
                .child(
                    div()
                        .id(format!("{label}-body"))
                        .test_support()
                        .min_w(px(0.))
                        .font_family(support::mono())
                        .text_size(px(10.))
                        .line_height(gpui_kit::relative(1.45))
                        .text_color(rgb(if failed && label == "Error" {
                            t.danger
                        } else {
                            t.muted
                        }))
                        .when_some(raw, |body, raw| {
                            body.child(
                                InputBase::new(format!("{label}-raw"))
                                    .role(None)
                                    .w_full()
                                    .min_w(px(0.))
                                    .child(Textarea::new(&raw[index])),
                            )
                        })
                        .when(raw.is_none(), |body| body.child(value.to_owned())),
                ),
        );
    }
    // Overflow owns the gesture even at an edge; fitting details pass it to the transcript.
    // Bubble after descendants and the built-in scroller, without replacing the retained handle.
    div()
        .child(details)
        .on_scroll_wheel(move |_, _, cx| {
            let overflowing = if let Some(scroll) = &scroll {
                scroll.max_offset().y > px(0.)
            } else {
                // Standalone details keep GPUI's element-owned offset rather than a fresh handle.
                let heights = field_heights.get();
                let gap = if heights.iter().all(|height| *height > px(0.)) {
                    px(8.)
                } else {
                    px(0.)
                };
                heights[0] + heights[1] + gap > viewport_height.get()
            };
            if overflowing {
                cx.stop_propagation();
            }
        })
        .into_any_element()
}

#[derive(Clone, Debug)]
pub enum ActivityContent {
    Tool {
        name: String,
        argument: String,
        arguments: String,
        result: String,
        running: bool,
        failed: bool,
    },
    Thinking {
        text: String,
    },
    Skill {
        name: String,
        prompt: String,
        text: String,
    },
}

#[derive(Clone, Debug)]
pub enum ActivityEvent {
    OpenLink(String),
    LinkRequested {
        target: markdown::LinkTarget,
        origin: Option<FocusHandle>,
        anchor: gpui_kit::Point<gpui_kit::Pixels>,
    },
    LinkContextRequested {
        target: markdown::LinkTarget,
        event: gpui_kit::MouseDownEvent,
    },
}

/// Text, selection, Markdown affordances and reader offsets belong to the source row.
/// The transcript may evict ActivityRow while retaining this entity.
pub struct ActivitySource {
    link_revision: u64,
    prose: Vec<Entity<TextViewState>>,
    presentations: Vec<Entity<MarkdownPresentation>>,
    prose_sources: Vec<String>,
    prose_scroll: Vec<ScrollHandle>,
    raw: [Option<Entity<TextareaState>>; 2],
    subscriptions: Vec<gpui_kit::Subscription>,
    selection: TextSelectionDomain,
}
impl ActivitySource {
    pub fn set_copy_paths(&mut self, enabled: bool, cx: &mut Context<Self>) {
        for presentation in &self.presentations {
            presentation.update(cx, |p, cx| p.set_copy_paths(enabled, cx));
        }
    }
    fn new(content: &ActivityContent, dark: bool, cx: &mut Context<Self>) -> Self {
        let owner = cx.entity().downgrade();
        let copy_owner = owner.clone();
        let selection = TextSelectionDomain::new(
            move |key, cx| {
                let owner = owner.upgrade()?;
                ((key as usize) < owner.read(cx).prose_sources.len()).then_some(key)
            },
            move |range, cx| {
                copy_owner
                    .upgrade()
                    .map(|owner| owner.read(cx).copy_prose(range))
                    .unwrap_or_default()
            },
        );
        let mut state = Self {
            link_revision: 0,
            selection,
            prose: vec![],
            presentations: vec![],
            prose_sources: vec![],
            prose_scroll: vec![],
            raw: [None, None],
            subscriptions: vec![],
        };
        state.set_content(content, dark, cx);
        state
    }
    pub fn set_content(&mut self, content: &ActivityContent, dark: bool, cx: &mut Context<Self>) {
        if let ActivityContent::Tool {
            arguments, result, ..
        } = content
        {
            for (index, value) in [arguments, result].into_iter().enumerate() {
                if let Some(raw) = &self.raw[index] {
                    raw.update(cx, |raw, cx| {
                        let selected = raw.selected_range();
                        if raw.value().get(selected.clone()) != value.get(selected) {
                            raw.set_selected_range(0..0, cx);
                        }
                    });
                }
            }
        }
        let sources: Vec<&str> = match content {
            ActivityContent::Tool { .. } => vec![],
            ActivityContent::Thinking { text } => vec![text],
            ActivityContent::Skill { prompt, text, .. } => vec![prompt, text],
        };
        if sources
            .iter()
            .copied()
            .ne(self.prose_sources.iter().map(String::as_str))
        {
            self.link_revision = self.link_revision.wrapping_add(1);
        }
        if sources.len() != self.prose.len() {
            self.selection.invalidate();
            self.prose.clear();
            self.presentations.clear();
            self.prose_sources.clear();
            self.prose_scroll.clear();
            self.subscriptions.clear();
            for source in &sources {
                let text = cx.new(|cx| {
                    let mut text =
                        TextViewState::markdown_with_extensions(source, markdown::extensions(), cx);
                    text.set_selection_domain(
                        Some((self.selection.clone(), self.prose.len() as u64)),
                        cx,
                    );
                    text
                });
                let owner = cx.entity().downgrade();
                let presentation = cx.new(|cx| {
                    MarkdownPresentation::new(
                        text.clone(),
                        dark,
                        move |cx| {
                            let _ = owner.update(cx, |_, cx| cx.notify());
                        },
                        cx,
                    )
                });
                self.subscriptions
                    .push(cx.observe(&text, |_, _, cx| cx.notify()));
                self.subscriptions
                    .push(cx.observe(&presentation, |_, _, cx| cx.notify()));
                self.prose.push(text);
                self.presentations.push(presentation);
                self.prose_sources.push(source.to_string());
                self.prose_scroll.push(ScrollHandle::new());
            }
        } else {
            for (index, source) in sources.iter().enumerate() {
                let previous = &self.prose_sources[index];
                if previous != source {
                    self.prose[index].update(cx, |text, cx| {
                        if let Some(suffix) = source.strip_prefix(previous.as_str()) {
                            text.push_str(suffix, cx);
                        } else {
                            self.selection.invalidate();
                            text.set_text(source, cx);
                        }
                    });
                    self.presentations[index].update(cx, |p, cx| p.source_changed(cx));
                    self.prose_sources[index] = source.to_string();
                }
            }
        }
        cx.notify();
    }
    fn copy_prose(&self, range: TextSelectionDomainRange) -> String {
        let (start, end) = if (range.anchor.item, range.anchor.offset)
            <= (range.cursor.item, range.cursor.offset)
        {
            (range.anchor, range.cursor)
        } else {
            (range.cursor, range.anchor)
        };
        (start.item..=end.item)
            .filter_map(|index| {
                let source = self.prose_sources.get(index as usize)?;
                TextViewState::copy_markdown_slice_with_extensions(
                    source,
                    (index == start.item).then_some(start.offset),
                    (index == end.item).then_some(end.offset),
                    SelectionFormat::Plain,
                    &markdown::extensions(),
                )
                .ok()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn raw_fields(
        &mut self,
        values: [&str; 2],
        dark: bool,
        failed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Entity<TextareaState>; 2] {
        std::array::from_fn(|index| {
            if self.raw[index].is_none() {
                let raw = cx.new(|cx| {
                    let mut raw = TextareaState::new(window, cx)
                        .soft_wrap(true)
                        .auto_grow(1, usize::MAX)
                        .default_value(values[index]);
                    raw.set_readonly(true, cx);
                    raw
                });
                self.subscriptions
                    .push(cx.observe(&raw, |_, _, cx| cx.notify()));
                self.raw[index] = Some(raw);
            }
            let raw = self.raw[index].as_ref().unwrap().clone();
            raw.update(cx, |raw, cx| {
                // Readonly is selectable, not disabled; retain the editor across theme changes.
                let t = support::tokens(dark);
                let mut style = support::input_style(dark);
                style.foreground = rgb(if failed && index == 1 {
                    t.danger
                } else {
                    t.muted
                })
                .into();
                style.background = rgb(t.panel_raised).into();
                style.caret_visible = false;
                raw.set_editor_style(style);
                let old = raw.value();
                if old.as_ref() == values[index] {
                    return;
                }
                let range = raw.selected_range();
                let keep = old.get(range.clone()) == values[index].get(range.clone());
                let offset = raw.scroll_offset();
                raw.set_value(values[index], window, cx);
                if keep {
                    raw.set_selected_range(range, cx);
                }
                raw.set_scroll_offset(offset, cx);
            });
            raw
        })
    }
}

pub struct ActivityRow {
    id: String,
    dark: bool,
    content: ActivityContent,
    expanded: bool,
    hovered: bool,
    focus: FocusHandle,
    source: Entity<ActivitySource>,
    _source_changed: gpui_kit::Subscription,
    context_handler: Option<std::sync::Arc<markdown::LinkContextHandler>>,
    details_scroll: ScrollHandle,
}
impl EventEmitter<ActivityEvent> for ActivityRow {}
impl ActivityRow {
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
        self.context_handler = Some(std::sync::Arc::new(handler));
        cx.notify();
    }
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        cx.notify();
    }
    pub fn new(
        id: impl Into<String>,
        content: ActivityContent,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = cx.new(|cx| ActivitySource::new(&content, dark, cx));
        let changed = cx.observe(&source, |_, _, cx| cx.notify());
        Self {
            id: id.into(),
            dark,
            content,
            expanded: false,
            hovered: false,
            focus: cx.focus_handle(),
            source,
            _source_changed: changed,
            context_handler: None,
            details_scroll: ScrollHandle::new(),
        }
    }
    /// Retain this on the transcript's source row, not in its evictable view cache.
    pub fn source_state(&self) -> Entity<ActivitySource> {
        self.source.clone()
    }
    #[cfg(test)]
    pub(crate) fn prose_source_for_test(
        &self,
        index: usize,
        cx: &gpui_kit::App,
    ) -> Entity<TextViewState> {
        self.source.read(cx).prose[index].clone()
    }
    pub fn set_source_state(&mut self, source: Entity<ActivitySource>, cx: &mut Context<Self>) {
        source.update(cx, |state, cx| {
            state.set_content(&self.content, self.dark, cx)
        });
        self._source_changed = cx.observe(&source, |_, _, cx| cx.notify());
        self.source = source;
    }
    pub fn set_details_scroll(&mut self, scroll: ScrollHandle) {
        self.details_scroll = scroll;
    }
    pub fn details_scroll(&self) -> &ScrollHandle {
        &self.details_scroll
    }
    pub fn expanded(&self) -> bool {
        self.expanded
    }
    pub fn hovered(&self) -> bool {
        self.hovered
    }
    pub fn focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }
    pub fn content(&self) -> &ActivityContent {
        &self.content
    }
    pub fn rendered_prose(&self, cx: &gpui_kit::App) -> Vec<String> {
        if !self.expanded {
            return vec![];
        }
        self.source
            .read(cx)
            .prose
            .iter()
            .map(|field| {
                field
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
    pub fn set_content(&mut self, content: ActivityContent, cx: &mut Context<Self>) {
        self.source
            .update(cx, |state, cx| state.set_content(&content, self.dark, cx));
        self.content = content;
        cx.notify();
    }
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.expandable() {
            self.expanded = !self.expanded;
            cx.notify();
        }
    }
    fn expandable(&self) -> bool {
        match &self.content {
            ActivityContent::Tool {
                arguments, result, ..
            } => !arguments.is_empty() || !result.is_empty(),
            ActivityContent::Thinking { text } => !text.is_empty(),
            ActivityContent::Skill { text, .. } => !text.is_empty(),
        }
    }
}
impl Render for ActivityRow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let (name, argument, arguments, result, prompt, text, running, failed) = match &self.content
        {
            ActivityContent::Tool {
                name,
                argument,
                arguments,
                result,
                running,
                failed,
            } => (
                name.as_str(),
                argument.as_str(),
                arguments.as_str(),
                result.as_str(),
                "",
                "",
                *running,
                *failed,
            ),
            ActivityContent::Thinking { text } => {
                ("thinking", "", "", "", "", text.as_str(), false, false)
            }
            ActivityContent::Skill { name, prompt, text } => (
                "skill",
                name.as_str(),
                "",
                "",
                prompt.as_str(),
                text.as_str(),
                false,
                false,
            ),
        };
        let open = self.expanded;
        let header = tool_header(
            self.id.clone(),
            Header {
                name,
                argument,
                open,
                hovered: self.hovered,
                expandable: self.expandable(),
                running,
                failed,
            },
            &self.focus,
            window,
            self.dark,
        )
        .on_hover(cx.listener(|this, hovered, _, cx| {
            this.hovered = *hovered;
            cx.notify();
        }))
        .on_click(cx.listener(|this, _, _, cx| this.toggle(cx)));
        let mut container = div().w_full().child(header);
        if open && matches!(self.content, ActivityContent::Tool { .. }) {
            let raw = self.source.update(cx, |source, cx| {
                source.raw_fields([arguments, result], self.dark, failed, window, cx)
            });
            container = container.child(tool_details_with_scroll(
                format!("{}-details", self.id),
                arguments,
                result,
                failed,
                self.dark,
                (window.viewport_size().height * 0.65).min(px(520.)),
                Some(&self.details_scroll),
                Some(&raw),
            ));
        } else if open {
            let mut details = div()
                .id(format!("{}-details", self.id))
                .test_support()
                .ml(px(14.))
                .mt(px(4.))
                .mb(px(2.))
                .pl(px(10.))
                .border_l_1()
                .border_color(rgb(t.border))
                .flex()
                .flex_col()
                .gap(px(8.));
            let fields: Vec<(&str, &str)> = if matches!(self.content, ActivityContent::Skill { .. })
            {
                [("Prompt", prompt), ("Instructions", text)].to_vec()
            } else {
                vec![("", text)]
            };
            for (field_index, (label, value)) in fields.into_iter().enumerate() {
                if value.is_empty() {
                    continue;
                }
                let mut field = div().flex().flex_col().gap(px(3.));
                if !label.is_empty() {
                    field = field.child(
                        div()
                            .font_family("Menlo")
                            .text_size(px(10.))
                            .line_height(gpui_kit::relative(1.25))
                            .text_color(rgb(t.faint))
                            .child(label.to_owned()),
                    );
                }
                let (presentation, scroll) = {
                    let source = self.source.read(cx);
                    (
                        source.presentations[field_index].clone(),
                        source.prose_scroll[field_index].clone(),
                    )
                };
                let thinking = matches!(self.content, ActivityContent::Thinking { .. });
                let view =
                    MarkdownPresentation::decorate(&presentation, 11., self.dark, window, cx)
                        .style(
                            markdown::text_style_at_size(self.dark, 11., window)
                                .with_foreground(rgb(t.muted).into())
                                .with_paragraph_gap(gpui_kit::rems(if thinking {
                                    0.
                                } else {
                                    0.48125
                                }))
                                .with_strong_weight(if thinking {
                                    gpui_kit::FontWeight::NORMAL
                                } else {
                                    gpui_kit::FontWeight::SEMIBOLD
                                }),
                        )
                        .on_link_activate({
                            let entity = cx.entity().downgrade();
                            let source = self.source.clone();
                            let revision = source.read(cx).link_revision;
                            move |link, event, window, cx| {
                                let Some(target) = markdown::activation_target(link, event) else {
                                    return;
                                };
                                let origin = window.focused(cx);
                                let _ = entity.update(cx, |row, cx| {
                                    if row.source == source
                                        && source.read(cx).link_revision == revision
                                    {
                                        cx.emit(ActivityEvent::LinkRequested {
                                            target,
                                            origin,
                                            anchor: event.position(),
                                        });
                                    }
                                });
                            }
                        })
                        .on_link_context_menu({
                            let entity = cx.entity().downgrade();
                            let source = self.source.clone();
                            let revision = source.read(cx).link_revision;
                            move |raw, event, window, cx| {
                                let Some(target) = markdown::link_target(raw) else {
                                    return;
                                };
                                let handler = entity
                                    .update(cx, |row, cx| {
                                        if row.source != source
                                            || source.read(cx).link_revision != revision
                                        {
                                            return None;
                                        }
                                        if row.context_handler.is_none() {
                                            cx.emit(ActivityEvent::LinkContextRequested {
                                                target: target.clone(),
                                                event: event.clone(),
                                            });
                                        }
                                        row.context_handler.clone()
                                    })
                                    .ok()
                                    .flatten();
                                if let Some(handler) = handler {
                                    handler(target, event, window, cx);
                                }
                            }
                        })
                        .text_size(px(11.))
                        .line_height(gpui_kit::relative(1.62));
                let wheel_scroll = scroll.clone();
                field = field.child(
                    div()
                        .child(
                            div()
                                .id(format!("{}-prose-{field_index}", self.id))
                                .test_support()
                                .min_w(px(0.))
                                .max_h((window.viewport_size().height * 0.65).min(px(520.)))
                                .overflow_y_scroll()
                                .track_scroll(&scroll)
                                .child(view),
                        )
                        .on_scroll_wheel(move |_, _, cx| {
                            if wheel_scroll.max_offset().y > px(0.) {
                                cx.stop_propagation();
                            }
                        }),
                );
                details = details.child(field);
            }
            container = container.child(details);
        }
        container
    }
}

#[cfg(test)]
#[path = "activity_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Bounds, KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, TestApp, TestAppWindow,
        WindowBounds, WindowOptions, point, size, test::TestWindowExt,
    };

    fn fixture(content: ActivityContent) -> (TestApp, TestAppWindow<ActivityRow>) {
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
            |_, cx| ActivityRow::new("row-one", content, false, cx),
        );
        window.draw();
        (app, window)
    }
    fn tool() -> ActivityContent {
        ActivityContent::Tool {
            name: "read".into(),
            argument: "file.rs".into(),
            arguments: "{\"path\":\"file.rs\"}".into(),
            result: "Found".into(),
            running: false,
            failed: false,
        }
    }
    #[test]
    fn disclosure_preserves_state_after_hover_exit() {
        let (_, mut window) = fixture(tool());
        let header = window.update(|_, window, _| window.find("row-one").bounds());
        window.simulate_mouse_move(header.center());
        window.draw();
        assert!(window.read(|row, _| row.hovered()));
        window.simulate_click(header.center(), MouseButton::Left);
        window.draw();
        assert!(window.read(|row, _| row.expanded()));
        assert!(
            window.update(
                |_, window, _| window.find("row-one-details").bounds().size.height > px(0.)
            )
        );
        window.simulate_mouse_move(point(px(800.), px(300.)));
        window.draw();
        assert!(window.read(|row, _| row.expanded()));
        assert!(!window.read(|row, _| row.hovered()));
        window.simulate_click(header.center(), MouseButton::Left);
        window.draw();
        assert!(!window.read(|row, _| row.expanded()));
    }
    #[test]
    fn keyboard_focus_is_distinct_from_pointer_hover() {
        let (_, mut window) = fixture(tool());
        window.update(|row, window, cx| row.focus(window, cx));
        window.draw();
        let keystroke = Keystroke::parse("space").unwrap();
        window.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        window.simulate_event(KeyUpEvent { keystroke });
        window.draw();
        assert!(window.read(|row, _| row.expanded()));
        assert!(window.update(|row, window, _| row.focused(window)));
        assert!(window.update(|_, window, _| window.last_input_was_keyboard()));
        window.simulate_mouse_move(point(px(800.), px(300.)));
        window.draw();
        assert!(!window.read(|row, _| row.hovered()));
        assert!(!window.update(|_, window, _| window.last_input_was_keyboard()));
    }
    #[test]
    fn prose_and_error_rows_keep_independent_detail_content() {
        let (_, mut window) = fixture(ActivityContent::Skill {
            name: "review".into(),
            prompt: "Review the change".into(),
            text: "Check **each** case.".into(),
        });
        window.update(|row, _, cx| row.toggle(cx));
        window.draw();
        assert_eq!(
            window.read(|row, cx| row.rendered_prose(cx)),
            vec!["Review the change", "Check each case."]
        );

        let (_, mut error_window) = fixture(ActivityContent::Tool {
            name: "bash".into(),
            argument: "check".into(),
            arguments: "missing path".into(),
            result: "Unavailable".into(),
            running: false,
            failed: true,
        });
        error_window.update(|row, _, cx| row.toggle(cx));
        error_window.draw();
        assert!(
            error_window.update(
                |_, window, _| window.find("row-one-details").bounds().size.height > px(0.)
            )
        );
    }
    #[test]
    fn multiline_and_unbroken_summaries_stay_one_line_and_disclose_by_pointer() {
        let (_app, mut window, row) = bounded_fixture("row-one", tool());
        window.simulate_resize(size(px(320.), px(700.)));
        window.update(|_, window, cx| window.bounds_changed(cx));
        window.draw();
        let baseline = window.update(|_, window, _| window.find("row-one").bounds());
        for argument in [
            "python <<'PY'\r\nprint('hello')\nPY".repeat(100),
            format!("/{}", "long-path-without-breaks".repeat(200)),
        ] {
            let raw = argument.clone();
            window.update(|_, _, cx| {
                row.update(cx, |row, cx| {
                    row.set_content(
                        ActivityContent::Tool {
                            name: "bash".into(),
                            argument,
                            arguments: raw.clone(),
                            result: "raw\noutput".into(),
                            running: true,
                            failed: false,
                        },
                        cx,
                    );
                })
            });
            window.draw();
            let header = window.update(|_, window, _| window.find("row-one").bounds());
            assert_eq!(header.size, baseline.size);
            let viewport = window.update(|_, window, _| window.viewport_size());
            assert!(
                header.right() <= px(320.),
                "header={header:?} viewport={viewport:?} baseline={baseline:?}"
            );
            window.simulate_mouse_move(header.center());
            window.simulate_click(header.center(), MouseButton::Left);
            window.draw();
            assert!(window.read(|_, cx| row.read(cx).expanded()));
            window.read(|_, cx| {
                let row = row.read(cx);
                let ActivityContent::Tool {
                    arguments, result, ..
                } = row.content()
                else {
                    panic!("expected tool");
                };
                assert_eq!(arguments, &raw);
                assert_eq!(result, "raw\noutput");
            });
            window.simulate_click(header.center(), MouseButton::Left);
            window.draw();
            assert!(!window.read(|_, cx| row.read(cx).expanded()));
        }
    }

    struct SelectionHost(Entity<ActivityRow>);
    impl Render for SelectionHost {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // A window root has intrinsic available width unless the host constrains it.
            div()
                .w(window.viewport_size().width)
                .h(window.viewport_size().height)
                .child(self.0.clone())
        }
    }

    fn bounded_fixture(
        id: &str,
        content: ActivityContent,
    ) -> (
        TestApp,
        TestAppWindow<gpui_kit::base::Root>,
        Entity<ActivityRow>,
    ) {
        let mut app = TestApp::new();
        app.update(gpui_kit::init);
        let row = app.new_entity(|cx| ActivityRow::new(id, content, false, cx));
        let mut window = app.open_window(|window, cx| {
            let host = cx.new(|_| SelectionHost(row.clone()));
            gpui_kit::base::Root::new(host, window, cx)
        });
        window.draw();
        (app, window, row)
    }

    #[test]
    fn expanded_heredoc_details_keep_raw_multiline_values() {
        let arguments = "python <<'PY'\nprint('hello')\nPY";
        let result = "hello\nsecond line\n";
        let (_app, mut window, row) = bounded_fixture(
            "raw-row",
            ActivityContent::Tool {
                name: "bash".into(),
                argument: arguments.into(),
                arguments: arguments.into(),
                result: result.into(),
                running: false,
                failed: false,
            },
        );
        window.draw();
        let header = window.update(|_, window, _| window.find("raw-row").bounds());
        window.simulate_click(header.center(), MouseButton::Left);
        window.draw();
        for id in ["Arguments-body", "Result-body"] {
            let body = window.update(|_, window, _| window.find(id).bounds());
            assert!(body.size.height > px(14.5), "details retain multiple lines");
        }
        window.read(|_, cx| {
            let ActivityContent::Tool {
                arguments: actual_arguments,
                result: actual_result,
                ..
            } = row.read(cx).content()
            else {
                panic!("tool")
            };
            assert_eq!(actual_arguments, arguments);
            assert_eq!(actual_result, result);
        });
    }

    struct StandaloneDetailsHost {
        result: String,
        outer: ScrollHandle,
    }
    impl Render for StandaloneDetailsHost {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("standalone-outer")
                .h(window.viewport_size().height)
                .w_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .track_scroll(&self.outer)
                .child(div().flex_shrink_0().child(tool_details(
                    "standalone-details",
                    "",
                    &self.result,
                    false,
                    false,
                )))
                .child(div().h(px(1500.)).flex_shrink_0())
        }
    }

    #[test]
    fn standalone_details_measure_overflow_without_resetting_element_owned_offset() {
        for overflowing in [false, true] {
            let mut app = TestApp::new();
            app.update(gpui_kit::init);
            let outer = ScrollHandle::new();
            let host = app.new_entity(|_| StandaloneDetailsHost {
                result: if overflowing {
                    "output line\n".repeat(300)
                } else {
                    "Short result".into()
                },
                outer: outer.clone(),
            });
            let mut window =
                app.open_window(|w, cx| gpui_kit::base::Root::new(host.clone(), w, cx));
            window.draw();
            let inner = window.update(|_, w, _| w.find("standalone-details").bounds().center());
            let before = window.update(|_, w, _| w.find("Result-body").bounds().top());
            window.simulate_scroll(inner, point(px(0.), px(-40.)));
            window.draw();
            if overflowing {
                assert_eq!(outer.offset().y, px(0.));
                let after = window.update(|_, w, _| w.find("Result-body").bounds().top());
                assert_eq!(after, before - px(40.));
                window.update(|_, _, cx| host.update(cx, |_, cx| cx.notify()));
                window.draw();
                assert_eq!(
                    window.update(|_, w, _| w.find("Result-body").bounds().top()),
                    after
                );
            } else {
                assert_eq!(outer.offset().y, px(-40.));
            }
        }
    }

    #[test]
    fn disabled_header_does_not_disclose() {
        let (_, mut window) = fixture(ActivityContent::Tool {
            name: "bash".into(),
            argument: "idle".into(),
            arguments: String::new(),
            result: String::new(),
            running: false,
            failed: false,
        });
        let header = window.update(|_, window, _| window.find("row-one").bounds());
        window.simulate_click(header.center(), MouseButton::Left);
        window.draw();
        assert!(!window.read(|row, _| row.expanded()));
    }
}
