use super::support;
use gpui_kit::{
    Context, Entity, EventEmitter, FocusHandle, IntoElement, Render, ScrollHandle, TestSupportExt,
    Window,
    base::{Button, TextView, TextViewState, TextViewStyle},
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
    tool_details_with_scroll(id, arguments, result, failed, dark, None)
}

fn tool_details_with_scroll(
    id: impl Into<gpui_kit::ElementId>,
    arguments: &str,
    result: &str,
    failed: bool,
    dark: bool,
    scroll: Option<&ScrollHandle>,
) -> impl IntoElement {
    let t = support::tokens(dark);
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
        .max_h(px(455.))
        .overflow_y_scroll()
        .when_some(scroll, |details, scroll| details.track_scroll(scroll));
    for (label, value) in [
        ("Arguments", arguments),
        (if failed { "Error" } else { "Result" }, result),
    ] {
        if value.is_empty() {
            continue;
        }
        details = details.child(
            div()
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
                        .whitespace_normal()
                        .child(value.to_owned()),
                ),
        );
    }
    // Bubble after the child's built-in scroller, including at either edge.
    // Never intercept capture: descendants (e.g. horizontal code) handle first.
    div()
        .child(details)
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
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
}

pub struct ActivityRow {
    id: String,
    dark: bool,
    content: ActivityContent,
    expanded: bool,
    hovered: bool,
    focus: FocusHandle,
    prose: Vec<Entity<TextViewState>>,
    details_scroll: ScrollHandle,
}
impl EventEmitter<ActivityEvent> for ActivityRow {}
impl ActivityRow {
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
        let prose = match &content {
            ActivityContent::Tool { .. } => vec![],
            ActivityContent::Thinking { text } => {
                vec![cx.new(|cx| TextViewState::markdown(text, cx))]
            }
            ActivityContent::Skill { prompt, text, .. } => [prompt, text]
                .iter()
                .map(|value| cx.new(|cx| TextViewState::markdown(value, cx)))
                .collect(),
        };
        Self {
            id: id.into(),
            dark,
            content,
            expanded: false,
            hovered: false,
            focus: cx.focus_handle(),
            prose,
            details_scroll: ScrollHandle::new(),
        }
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
        self.prose
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
        self.prose = match &content {
            ActivityContent::Tool { .. } => vec![],
            ActivityContent::Thinking { text } => {
                vec![cx.new(|cx| TextViewState::markdown(text, cx))]
            }
            ActivityContent::Skill { prompt, text, .. } => [prompt, text]
                .iter()
                .map(|value| cx.new(|cx| TextViewState::markdown(value, cx)))
                .collect(),
        };
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
            container = container.child(tool_details_with_scroll(
                format!("{}-details", self.id),
                arguments,
                result,
                failed,
                self.dark,
                Some(&self.details_scroll),
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
                field = field.child(
                    TextView::new(&self.prose[field_index])
                        .on_link_click({
                            let entity = cx.entity().downgrade();
                            move |url, _, _, cx| {
                                let _ = entity.update(cx, |_, cx| {
                                    cx.emit(ActivityEvent::OpenLink(url.to_string()))
                                });
                            }
                        })
                        .text_size(px(11.))
                        .line_height(gpui_kit::relative(1.62))
                        .style(
                            TextViewStyle::default()
                                .with_foreground(rgb(t.muted).into())
                                // Markdown p margins are 0.7em of 11px; GPUI rems use 16px.
                                .with_paragraph_gap(gpui_kit::rems(
                                    if matches!(self.content, ActivityContent::Thinking { .. }) {
                                        0.
                                    } else {
                                        0.48125
                                    },
                                ))
                                .with_strong_weight(
                                    if matches!(self.content, ActivityContent::Thinking { .. }) {
                                        gpui_kit::FontWeight::NORMAL
                                    } else {
                                        gpui_kit::FontWeight::SEMIBOLD
                                    },
                                )
                                .with_heading(|level| {
                                    let size = match level {
                                        1 => 14.85,
                                        2 => 13.2,
                                        3 => 11.88,
                                        4 => 11.,
                                        5 => 10.45,
                                        _ => 9.9,
                                    };
                                    gpui_kit::StyleRefinement::default()
                                        .text_size(px(size))
                                        .font_weight(gpui_kit::FontWeight(if level <= 4 {
                                            650.
                                        } else {
                                            600.
                                        }))
                                        .line_height(gpui_kit::relative(1.25))
                                        .pb(px(size * 0.55))
                                }),
                        ),
                );
                details = details.child(field);
            }
            container = container.child(details);
        }
        container
    }
}

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
