use super::{
    font as native_font,
    support::{self, tokens},
    syntax,
};
use gpui_kit::{
    ClipboardItem, Context, Entity, IntoElement, Render, Task, TestSupportExt, TextRun, Window,
    base::{TextView, TextViewState, TextViewStyle},
    div, font,
    prelude::*,
    px, rgb,
};
use std::time::Duration;

pub struct MarkdownView {
    source: String,
    on_link: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    dark: bool,
    text: Entity<TextViewState>,
    copied: Option<usize>,
    revision: usize,
    timer: Option<Task<()>>,
}

impl MarkdownView {
    pub fn new(
        source: impl Into<String>,
        dark: bool,
        on_link: impl Fn(&str) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = source.into();
        let text = cx.new(|cx| TextViewState::markdown(&source, cx));
        Self {
            source,
            dark,
            text,
            copied: None,
            revision: 0,
            timer: None,
            on_link: std::sync::Arc::new(on_link),
        }
    }

    pub fn set_source(&mut self, source: impl Into<String>, cx: &mut Context<Self>) {
        let source = source.into();
        if self.source == source {
            return;
        }
        self.text.update(cx, |text, cx| text.set_text(&source, cx));
        self.source = source;
        self.copied = None;
        self.revision += 1;
        self.timer = None;
        cx.notify();
    }

    pub fn set_link_handler(
        &mut self,
        handler: impl Fn(&str) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) {
        self.on_link = std::sync::Arc::new(handler);
        cx.notify();
    }

    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn text(&self) -> &Entity<TextViewState> {
        &self.text
    }
    pub fn copied_block(&self) -> Option<usize> {
        self.copied
    }
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = tokens(self.dark);
        // CSS 80ch is eighty advances of the body font's zero glyph, not eighty characters of prose.
        let zero_width = window
            .text_system()
            .layout_line(
                "0",
                px(13.),
                &[TextRun {
                    len: 1,
                    font: font(native_font::family()),
                    color: rgb(p.text).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width;
        let scene = cx.entity().clone();
        let copied = self.copied;
        let source = self.source.as_str();
        let fence_starts: Vec<usize> = source
            .match_indices("```")
            .enumerate()
            .filter_map(|(index, (start, _))| (index % 2 == 0).then_some(start))
            .collect();
        let mut table_style = gpui_kit::StyleRefinement::default()
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(p.border))
            .bg(rgb(p.canvas));
        table_style.overflow.x = Some(gpui_kit::Overflow::Scroll);
        let style = TextViewStyle::default()
            .with_prose_max_width(zero_width * 80.)
            .with_strong_weight(gpui_kit::FontWeight::SEMIBOLD)
            .with_dark(self.dark)
            .with_code_block_horizontal_scroll(true)
            .with_code_block_paragraph_gap(gpui_kit::rems(0.57))
            .with_paragraph_gap(gpui_kit::rems(0.7))
            .with_list_style(px(18.2), px(3.64), rgb(p.faint).into())
            .with_inline_code_padding(px(3.432))
            .with_heading(|level| {
                let size = match level {
                    1 => 17.55,
                    2 => 15.6,
                    3 => 14.04,
                    4 => 13.,
                    5 => 12.35,
                    _ => 11.7,
                };
                gpui_kit::StyleRefinement::default()
                    .text_size(px(size))
                    .font_weight(gpui_kit::FontWeight(if level <= 4 { 650. } else { 600. }))
                    .line_height(gpui_kit::relative(1.25))
                    .pt(px(match level {
                        1 => 0.,
                        2 => 7.5,
                        3 => 5.5,
                        _ => 2.,
                    }))
                    .pb(px(size * 0.55
                        + match level {
                            2 => 1.,
                            3 => 2.,
                            _ => 0.,
                        }))
            })
            .with_foreground(rgb(p.text).into())
            .with_muted_foreground(rgb(p.muted).into())
            .with_link(rgb(if self.dark { 0x59c2ff } else { 0x22a4e6 }).into())
            .with_code_background(rgb(if self.dark { 0x080b11 } else { 0xebeef0 }).into())
            .with_inline_code(gpui_kit::HighlightStyle::default())
            .with_border(rgb(p.border).into())
            .with_code_block(
                gpui_kit::StyleRefinement::default()
                    .p(px(11.))
                    .px(px(12.))
                    .mt(px(-2.1))
                    .rounded(px(9.))
                    .text_size(px(11.44))
                    .line_height(gpui_kit::relative(1.7))
                    .border_1()
                    .border_color(rgb(p.border)),
            )
            .with_table(table_style.text_size(px(12.22)))
            .with_table_fit_content(true)
            .with_table_css_cells(true)
            .with_table_head(
                gpui_kit::StyleRefinement::default().font_weight(gpui_kit::FontWeight::SEMIBOLD),
            )
            .with_table_cell(
                gpui_kit::StyleRefinement::default()
                    .py(px(6.))
                    .px(px(12.))
                    .whitespace_nowrap(),
            );
        div()
            .w_full()
            .h_full()
            .text_color(rgb(p.text))
            .font_family(native_font::family())
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.62))
            .child(
                TextView::new(&self.text)
                    .scrollable(true)
                    .size_full()
                    .style(style)
                    .on_link_click({
                        let handler = self.on_link.clone();
                        move |url, _, _, _| {
                            handler(url.as_ref());
                        }
                    })
                    .shared_code_block_highlighter(syntax::code_block_highlighter(self.dark))
                    .code_block_actions(move |block, _, _| {
                        let code = block.code().to_string();
                        let index = block.span.map_or(0, |span| {
                            fence_starts
                                .partition_point(|start| *start <= span.start)
                                .saturating_sub(1)
                        });
                        let language = block.lang().map(|s| s.to_string()).unwrap_or_default();
                        let owner = scene.clone();
                        div()
                            .relative()
                            .flex()
                            .items_start()
                            .child(
                                div()
                                    .id(format!("language-{}", index + 1))
                                    .test_support()
                                    .mr(px(3.))
                                    .mt(px(5.))
                                    .font_family(support::mono())
                                    .text_size(px(10.))
                                    .line_height(gpui_kit::relative(1.))
                                    .text_color(rgb(p.muted))
                                    .opacity(0.)
                                    .group_hover("markdown-code", |s| s.opacity(0.7))
                                    .child(language),
                            )
                            .child(
                                div()
                                    .id(format!("copy-{}", index + 1))
                                    .test_support()
                                    .cursor_default()
                                    .p(px(4.))
                                    .rounded(px(4.))
                                    .opacity(0.)
                                    .group_hover("markdown-code", |s| s.opacity(0.45))
                                    .hover(|s| s.opacity(1.))
                                    .child(support::icon(
                                        if copied == Some(index) {
                                            "check"
                                        } else {
                                            "copy"
                                        },
                                        13.,
                                        p.muted,
                                    ))
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            code.clone(),
                                        ));
                                        owner.update(cx, |scene, cx| {
                                            scene.revision += 1;
                                            let revision = scene.revision;
                                            scene.copied = Some(index);
                                            scene.timer = Some(cx.spawn(async move |weak, cx| {
                                                cx.background_executor()
                                                    .timer(Duration::from_millis(1200))
                                                    .await;
                                                if let Some(entity) = weak.upgrade() {
                                                    entity.update(cx, |scene, cx| {
                                                        if scene.revision == revision {
                                                            scene.copied = None;
                                                            cx.notify();
                                                        }
                                                    });
                                                }
                                            }));
                                            cx.notify();
                                        });
                                    }),
                            )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        InputEvent, MouseButton, ScrollDelta, ScrollWheelEvent, TestApp, TestAppContext,
        TestSupportExt, gpui, point, test::TestWindowExt,
    };

    #[test]
    fn copy_feedback_expires_and_source_replacement_cancels_old_timer() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut window =
            app.open_window(|_, cx| MarkdownView::new("```rs\nlet a = 1;\n```", false, |_| {}, cx));
        window.draw();
        let copy = window.update(|_, window, _| window.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        window.update(|view, _, cx| view.set_source("```rs\nlet b = 2;\n```", cx));
        window.draw();
        let copy = window.update(|_, window, _| window.find("copy-1").bounds());
        window.simulate_click(copy.center(), MouseButton::Left);
        app.advance_clock(Duration::from_millis(600));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), Some(0));
        app.advance_clock(Duration::from_millis(601));
        app.run_until_parked();
        assert_eq!(window.read(|view, _| view.copied_block()), None);
    }

    #[test]
    fn rendered_link_click_uses_latest_caller_handler() {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let old = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let old_handler = old.clone();
        let mut window = app.open_window(|_, cx| {
            MarkdownView::new(
                "[link](https://example.org)",
                false,
                move |url| {
                    old_handler.lock().unwrap().push(url.to_string());
                },
                cx,
            )
        });
        window.draw();
        window.simulate_click(gpui_kit::point(px(10.), px(10.)), MouseButton::Left);
        assert_eq!(old.lock().unwrap().as_slice(), &["https://example.org"]);

        let latest = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = latest.clone();
        window.update(|view, _, cx| {
            view.set_link_handler(
                move |url| received.lock().unwrap().push(url.to_string()),
                cx,
            )
        });
        window.draw();
        window.simulate_click(gpui_kit::point(px(10.), px(10.)), MouseButton::Left);
        assert_eq!(old.lock().unwrap().len(), 1);
        assert_eq!(latest.lock().unwrap().as_slice(), &["https://example.org"]);
    }

    struct BoundedMarkdownHost {
        view: Entity<MarkdownView>,
    }

    impl Render for BoundedMarkdownHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("markdown-host")
                .test_support()
                .w(px(800.))
                .h(px(300.))
                .overflow_hidden()
                .child(self.view.clone())
        }
    }

    #[gpui::test]
    fn bounded_code_layout_and_copy_stay_pinned_after_wheel(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let source = format!(
            "| Name | State |\n| --- | --- |\n| Alpha | Ready |\n\n```txt\n{}\n```",
            "long-code-line-".repeat(120)
        );
        let (_, window) = cx.add_window_view(|_, cx| BoundedMarkdownHost {
            view: cx.new(|cx| MarkdownView::new(source, false, |_| {}, cx)),
        });
        let (host, code, copy) = window.update(|window, cx| {
            let _ = window.draw(cx);
            (
                window.find("markdown-host").bounds(),
                window.find("code-1").bounds(),
                window.find("copy-1").bounds(),
            )
        });
        assert_eq!(host.size.width, px(800.));
        assert!(
            code.size.width <= host.size.width,
            "code {code:?}, host {host:?}"
        );
        assert!(code.left() >= host.left() && code.right() <= host.right());
        assert!(
            code.size.height < px(100.),
            "long line must not wrap: {code:?}"
        );
        assert!(
            copy.left() >= code.left() && copy.right() <= code.right(),
            "copy {copy:?}, code {code:?}"
        );
        assert!(copy.top() >= code.top() && copy.bottom() <= code.bottom());
        window.update(|window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position: point(code.left() + px(40.), code.top() + px(20.)),
                    delta: ScrollDelta::Pixels(point(px(-250.), px(0.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            let _ = window.draw(cx);
            assert_eq!(window.find("code-1").bounds(), code);
            assert_eq!(window.find("copy-1").bounds(), copy);
        });
    }

    #[gpui::test]
    fn wide_code_stays_single_row_and_table_renders(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let source = format!(
            "| Name | State |\n| --- | --- |\n| Alpha | Ready |\n\n```txt\n{}\n```",
            "long-code-line-".repeat(120)
        );
        let (view, window) =
            cx.add_window_view(|_, cx| MarkdownView::new(source, false, |_| {}, cx));
        window.update(|window, cx| {
            let _ = window.draw(cx);
            let rendered = view.read(cx).text.read(cx).rendered_text();
            assert!(rendered.as_str().contains("Alpha"));
            assert!(rendered.as_str().contains("Ready"));
        });
        let copy = window.update(|window, _| window.find("copy-1").bounds());
        assert!(
            copy.top() < px(150.),
            "long code must remain a single scrollable row: {copy:?}"
        );
    }

    #[gpui::test]
    fn source_update_reparses_text_and_invalidates_copy(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let view = cx.update(|cx| {
            cx.new(|cx| MarkdownView::new("# First\n\n```rs\nlet a = 1;\n```", false, |_| {}, cx))
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.text.update(cx, |text, cx| text.select_all(cx));
                view.copied = Some(0);
                let revision = view.revision;
                view.set_source("# First\n\n```rs\nlet a = 1;\n```", cx);
                assert_eq!(view.revision, revision);
                assert_eq!(view.copied, Some(0));
                assert!(!view.text.read(cx).selected_text().is_empty());
                view.set_source("# Second\n\nA [link](https://example.org)", cx);
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert_eq!(view.source(), "# Second\n\nA [link](https://example.org)");
            assert_eq!(view.copied_block(), None);
            assert!(
                view.text()
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .contains("Second")
            );
        });
    }
}
