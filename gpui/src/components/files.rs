use std::sync::Arc;

use gpui_kit::{
    Context, FocusHandle, Image, IntoElement, ObjectFit, Render, ScrollHandle, TestSupportExt,
    Window,
    base::{SelectableText, TextSelection, TextView, TextViewState, TextViewStyle},
    div, img, linear_color_stop, linear_gradient,
    prelude::*,
    px, rgb, rgba,
};

use super::{support, syntax};

#[path = "file_preview_path.rs"]
pub mod preview_path;
#[path = "file_preview_type.rs"]
pub mod preview_type;

#[path = "markdown_document.rs"]
mod markdown_document;
use markdown_document::MarkdownDocument;
pub use markdown_document::MarkdownLinkRequested;

#[derive(Clone)]
pub enum FileContent {
    Text(String),
    Image,
    Error(String),
    Unsupported,
    Loading,
}

#[derive(Clone)]
pub struct CloseRequested;

const PLAIN_LINE_HEIGHT: f32 = 1.65;

// Base's code projection normalizes CR and supplies a final newline. Plain text
// is the exact-copy fallback when those transformations would change the snapshot.
fn highlightable(text: &str) -> bool {
    text.ends_with('\n') && !text.contains('\r') && text.encode_utf16().count() <= 20_000
}
fn source_fence(text: &str, language: &str) -> String {
    let count = text
        .lines()
        .map(|line| line.trim_start().chars().take_while(|c| *c == '~').count())
        .max()
        .unwrap_or(0);
    let fence = "~".repeat(count.max(2) + 1);
    format!("{fence}{language}\n{}{fence}", text)
}

fn plain_text_style() -> gpui_kit::TextStyleRefinement {
    gpui_kit::TextStyleRefinement {
        fractional_line_height: Some(true),
        ..Default::default()
    }
}

pub struct FileViewer {
    filename: String,
    directory: String,
    content: FileContent,
    dark: bool,
    closed: bool,
    close_focus: FocusHandle,
    scroll: ScrollHandle,
    source: Option<gpui_kit::Entity<TextViewState>>,
    image: Option<Arc<Image>>,
    document: Option<gpui_kit::Entity<MarkdownDocument>>,
    on_link: Arc<dyn Fn(MarkdownLinkRequested) + Send + Sync>,
    on_context: Option<Arc<super::markdown::LinkContextHandler>>,
    _keys: gpui_kit::Subscription,
    show_loading: bool,
    truncated: bool,
    keyboard_enabled: bool,
}

impl FileViewer {
    pub fn set_theme(&mut self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        // Retain parsed text, selection and reader position across theme changes.
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| document.set_theme(dark, _window, cx));
        }
        cx.notify();
    }
    pub fn new(
        filename: impl Into<String>,
        directory: impl Into<String>,
        content: FileContent,
        image: Option<Arc<Image>>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filename = filename.into();
        let directory = directory.into();
        let close_focus = cx.focus_handle();
        close_focus.focus(window, cx);
        let is_document = markdown_document::is_document(&filename);
        let on_link: Arc<dyn Fn(MarkdownLinkRequested) + Send + Sync> = Arc::new(|_| {});
        let document = if let FileContent::Text(text) = &content {
            is_document.then(|| {
                let handler = on_link.clone();
                cx.new(|cx| {
                    MarkdownDocument::new(text.clone(), dark, move |request| handler(request), cx)
                })
            })
        } else {
            None
        };
        let source =
            if matches!(&content, FileContent::Text(text) if highlightable(text)) && !is_document {
                syntax::language_for_filename(&filename).map(|language| {
                    let code = match &content {
                        FileContent::Text(text) => text.as_str(),
                        _ => "",
                    };
                    // A collision-free fence retains syntax spans and exact terminal blank lines.
                    let fenced = source_fence(code, language);
                    cx.new(|cx| TextViewState::markdown(&fenced, cx).selectable(true))
                })
            } else {
                None
            };
        let weak = cx.weak_entity();
        let owner_window = window.window_handle();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(scene) = weak.upgrade() else { return };
            let view = scene.read(cx);
            if window.window_handle() != owner_window
                || view.closed
                || !view.keyboard_enabled
                || view
                    .presentation(cx)
                    .is_some_and(|presentation| presentation.read(cx).has_expanded_diagram(cx))
            {
                return;
            }
            let key = event.keystroke.key.as_str();
            let modifiers = event.keystroke.modifiers;
            if key == "escape" {
                scene.update(cx, |scene, cx| {
                    scene.close(cx);
                });
                cx.stop_propagation();
            } else if key == "a" && (modifiers.platform || modifiers.control) {
                let source = scene.read(cx).selection_source(cx);
                if let Some(source) = source {
                    source.update(cx, |state, cx| state.select_all(cx));
                } else {
                    TextSelection::select_all(window, cx);
                }
                cx.stop_propagation();
            } else if !modifiers.platform
                && !modifiers.control
                && !modifiers.alt
                && view.close_focus.is_focused(window)
            {
                let scroll = &scene.read(cx).scroll;
                let mut offset = scroll.offset();
                let page = f32::from(scroll.bounds().size.height) * 0.85;
                match key {
                    "down" => offset.y -= px(40.),
                    "up" => offset.y += px(40.),
                    "right" => offset.x -= px(40.),
                    "left" => offset.x += px(40.),
                    "pagedown" => offset.y -= px(page),
                    "pageup" => offset.y += px(page),
                    "home" => offset.y = px(0.),
                    "end" => offset.y = scroll.max_offset().y,
                    _ => return,
                }
                scroll.set_offset(offset);
                cx.stop_propagation();
            }
        });
        Self {
            filename,
            directory,
            content,
            dark,
            closed: false,
            close_focus,
            scroll: ScrollHandle::new(),
            source,
            image,
            document,
            on_link,
            on_context: None,
            _keys: keys,
            show_loading: false,
            truncated: false,
            keyboard_enabled: true,
        }
    }

    /// Central overlay ownership can suspend Escape/selection without moving focus.
    pub fn set_keyboard_enabled(&mut self, enabled: bool) {
        self.keyboard_enabled = enabled;
    }
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.closed {
            self.closed = true;
            cx.emit(CloseRequested);
            cx.notify();
        }
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn filename(&self) -> &str {
        &self.filename
    }
    pub fn directory(&self) -> &str {
        &self.directory
    }
    pub fn content(&self) -> &FileContent {
        &self.content
    }
    pub fn close_focused(&self, window: &Window) -> bool {
        !self.closed && self.close_focus.is_focused(window)
    }
    pub fn scroll_offset(&self) -> gpui_kit::Point<gpui_kit::Pixels> {
        self.scroll.offset()
    }
    pub fn presentation(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<gpui_kit::Entity<super::markdown::MarkdownPresentation>> {
        self.document
            .as_ref()
            .map(|document| document.read(cx).presentation(cx))
    }
    pub fn selected_text(&self, cx: &gpui_kit::App) -> String {
        self.selection_source(cx)
            .map_or_else(String::new, |source| source.read(cx).selected_text())
    }
    #[cfg(test)]
    pub(crate) fn selection_source_for_test(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<gpui_kit::Entity<TextViewState>> {
        self.selection_source(cx)
    }
    fn selection_source(&self, cx: &gpui_kit::App) -> Option<gpui_kit::Entity<TextViewState>> {
        self.document
            .as_ref()
            .map(|document| document.read(cx).text(cx))
            .or_else(|| self.source.clone())
    }
    pub fn rendered_text(&self, cx: &gpui_kit::App) -> Option<String> {
        if let Some(document) = &self.document {
            Some(document.read(cx).rendered_text(cx))
        } else if let Some(source) = &self.source {
            Some(source.read(cx).rendered_text().as_str().to_owned())
        } else if let FileContent::Text(text) = &self.content {
            Some(text.clone())
        } else {
            None
        }
    }
    pub fn set_link_handler(
        &mut self,
        handler: impl Fn(MarkdownLinkRequested) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) {
        self.on_link = Arc::new(handler);
        if let Some(document) = &self.document {
            let handler = self.on_link.clone();
            document.update(cx, |document, cx| document.set_link_handler(handler, cx));
        }
    }
    pub fn set_context_handler(
        &mut self,
        handler: impl Fn(
            super::markdown::LinkTarget,
            &gpui_kit::MouseDownEvent,
            &mut Window,
            &mut gpui_kit::App,
        ) + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let handler: Arc<super::markdown::LinkContextHandler> = Arc::new(handler);
        self.on_context = Some(handler.clone());
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| {
                document.set_context_handler(
                    move |target, event, window, cx| handler(target, event, window, cx),
                    cx,
                )
            });
        }
    }
    pub fn image_loaded(&self, cx: &gpui_kit::App) -> bool {
        matches!(self.content, FileContent::Image)
            && self
                .image
                .as_ref()
                .is_some_and(|image| image.is_asset_cached(cx))
    }
    pub fn set_image(&mut self, image: Arc<Image>, cx: &mut Context<Self>) {
        self.image = Some(image);
        self.set_content(FileContent::Image, cx);
    }
    /// The shell owns the 200ms delay and 15s deadline, fenced by its request token.
    pub fn loading_visible(&self) -> bool {
        matches!(self.content, FileContent::Loading) && self.show_loading
    }
    pub fn set_loading_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.show_loading = visible;
        cx.notify();
    }
    pub fn set_truncated(&mut self, truncated: bool, cx: &mut Context<Self>) {
        self.truncated = truncated;
        cx.notify();
    }
    pub fn set_content(&mut self, content: FileContent, cx: &mut Context<Self>) {
        self.show_loading = false;
        self.document = if let FileContent::Text(text) = &content {
            markdown_document::is_document(&self.filename).then(|| {
                let handler = self.on_link.clone();
                cx.new(|cx| {
                    MarkdownDocument::new(
                        text.clone(),
                        self.dark,
                        move |request| handler(request),
                        cx,
                    )
                })
            })
        } else {
            None
        };
        if let (Some(document), Some(handler)) = (&self.document, self.on_context.clone()) {
            document.update(cx, |document, cx| {
                document.set_context_handler(
                    move |target, event, window, cx| handler(target, event, window, cx),
                    cx,
                )
            });
        }
        self.source = if let FileContent::Text(text) = &content {
            if self.document.is_some() {
                None
            } else {
                self.highlighted_source(text, cx)
            }
        } else {
            None
        };
        if !matches!(content, FileContent::Image) {
            self.image = None;
        }
        self.content = content;
        self.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        cx.notify();
    }
    fn highlighted_source(
        &self,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::Entity<TextViewState>> {
        if !highlightable(text) {
            return None;
        }
        let language = syntax::language_for_filename(&self.filename)?;
        let fenced = source_fence(text, language);
        Some(cx.new(|cx| TextViewState::markdown(&fenced, cx).selectable(true)))
    }
    fn kind(&self) -> &str {
        match self.content {
            FileContent::Text(_) => "text",
            FileContent::Image => "image",
            FileContent::Error(_) => "error",
            FileContent::Unsupported => "unsupported",
            FileContent::Loading => "loading",
        }
    }
}
impl gpui_kit::EventEmitter<CloseRequested> for FileViewer {}

impl Render for FileViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = support::tokens(self.dark);
        let mut page = div()
            .size_full()
            .font_family(super::font::family())
            .text_color(rgb(colors.text));
        if self.closed {
            return page;
        }
        let canvas = if self.dark { 0x000000 } else { 0xffffff };
        page = page.bg(rgb(canvas));
        let kind = self.kind();
        let filename = self.filename.clone();
        let directory = self.directory.clone();
        let header = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(46.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgba((canvas << 8) | 0xf5), 0.),
                linear_color_stop(rgba(canvas << 8), 1.),
            ))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .flex()
                    .gap(px(7.))
                    .max_w(px(f32::from(window.viewport_size().width)
                        .mul_add(0.7, 0.)
                        .min(620.)))
                    .overflow_hidden()
                    .items_baseline()
                    .text_size(px(11.))
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_color(rgb(colors.text))
                            .child(filename),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_color(rgb(colors.muted))
                            .child(directory),
                    ),
            );
        let close_focused = self.close_focus.is_focused(window);
        let close = gpui_kit::base::Button::new("close")
            .accessibility_label("Close file viewer")
            .absolute()
            .top(px(9.))
            .right(px(12.))
            .w(px(28.))
            .h(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .text_color(rgb(colors.muted))
            .opacity(0.65)
            .hover(move |s| s.opacity(1.))
            .when(close_focused, move |s| {
                // GPUI shadows paint under transparent surfaces; keep the outer ring hollow.
                s.opacity(1.)
                    .bg(rgb(canvas))
                    .text_color(rgb(colors.text))
                    .shadow(vec![
                        gpui_kit::BoxShadow::new(
                            px(0.),
                            px(0.),
                            gpui_kit::Rgba {
                                a: 0.22,
                                ..rgb(colors.accent)
                            }
                            .into(),
                        )
                        .spread_radius(px(2.)),
                    ])
            })
            .track_focus(&self.close_focus)
            .on_click(cx.listener(|s, _, _, cx| {
                s.close(cx);
            }))
            .child(support::icon(
                "cross",
                16.,
                if close_focused {
                    colors.text
                } else {
                    colors.muted
                },
            ));
        let content = div()
            .id("content")
            .test_support()
            .absolute()
            .top(px(46.))
            .bottom_0()
            .left_0()
            .right_0()
            .overflow_scroll()
            .track_scroll(&self.scroll);
        let content = match kind {
            "image" => content.child(
                div()
                    .h_full()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(self.image.clone(), |container, image| {
                        container.child(
                            img(image)
                                .object_fit(ObjectFit::Contain)
                                .size_full()
                                .id("image")
                                .test_support(),
                        )
                    }),
            ),
            "loading" | "error" | "unsupported" => content.child(
                div()
                    .h_full()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(colors.muted))
                    .text_size(px(11.))
                    .child(if let FileContent::Error(ref message) = self.content {
                        message.clone()
                    } else if kind == "loading" {
                        if self.show_loading {
                            "Loading preview".into()
                        } else {
                            String::new()
                        }
                    } else {
                        "Preview unavailable.".to_string()
                    }),
            ),
            _ if self.document.is_some() => content.child(
                div()
                    .w_full()
                    .min_h_full()
                    .flex()
                    .justify_center()
                    .child(self.document.as_ref().unwrap().clone()),
            ),
            _ => {
                let body = div()
                    .id("text")
                    .test_support()
                    .min_h_full()
                    .px(px(40.))
                    .pt(px(20.))
                    .pb(px(40.))
                    .font_family(support::mono())
                    .text_size(px(12.))
                    .line_height(gpui_kit::relative(PLAIN_LINE_HEIGHT));
                if let Some(source) = &self.source {
                    let style = TextViewStyle::default()
                        .with_foreground(rgb(colors.text).into())
                        .with_code_background(rgb(canvas).into())
                        .with_code_block(
                            gpui_kit::StyleRefinement::default()
                                .bg(rgb(canvas))
                                .text_size(px(12.))
                                .font_family(support::mono())
                                .p_0(),
                        );
                    content.child(
                        body.child(
                            div().id("highlight").test_support().child(
                                TextView::new(source)
                                    .style(style)
                                    .shared_code_block_highlighter(syntax::code_block_highlighter(
                                        self.dark,
                                    )),
                            ),
                        ),
                    )
                } else {
                    content.child(
                        body.child(
                            SelectableText::new(
                                "file-plain",
                                match &self.content {
                                    FileContent::Text(text) => text.clone(),
                                    _ => String::new(),
                                },
                            )
                            .text_style(plain_text_style()),
                        ),
                    )
                }
            }
        };
        page.child(content).child(header).child(close).when(
            self.truncated && kind != "error",
            |page| {
                page.child(
                    div()
                        .absolute()
                        .right(px(12.))
                        .bottom(px(10.))
                        .px(px(7.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .bg(rgb(canvas))
                        .text_size(px(10.))
                        .text_color(rgb(colors.muted))
                        .child("Showing first 512 KiB"),
                )
            },
        )
    }
}

#[cfg(test)]
#[path = "file_viewer_regression_tests.rs"]
mod file_regressions;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Modifiers, MouseButton, TestAppContext, base::TextSelectionLayer, gpui, test::TestWindowExt,
    };

    fn settle_file_diagrams(
        app: &mut gpui_kit::TestApp,
        window: &mut gpui_kit::TestAppWindow<gpui_kit::base::Root>,
        viewer: &gpui_kit::Entity<FileViewer>,
    ) {
        use std::time::{Duration, Instant};

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.run_until_parked();
            window.draw();
            if window.update(|_, window, cx| {
                let presentation = viewer.read(cx).presentation(cx).unwrap();
                window.try_find("diagram-expand").is_some()
                    && !presentation.read(cx).render_pending(cx)
            }) {
                return;
            }
            assert!(Instant::now() < deadline, "file diagram preparation");
            std::thread::sleep(Duration::from_millis(10));
            app.advance_clock(Duration::from_millis(10));
        }
    }

    fn root_file_diagram_dismissal(escape: bool) {
        use gpui_kit::{TestApp, base::Root};
        use std::time::Duration;

        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, false));
        let mut viewer_entity = None;
        let mut window = app.open_window(|window, cx| {
            let viewer = cx.new(|cx| {
                FileViewer::new(
                    "guide.md",
                    "docs",
                    FileContent::Text(
                        "Reader paragraph.\n\n```rs\nlet output = 42;\n```\n\n```mermaid\ngraph LR\nA --> B\n```\n".into(),
                    ),
                    None,
                    false,
                    window,
                    cx,
                )
            });
            viewer_entity = Some(viewer.clone());
            Root::new(viewer, window, cx)
        });
        let viewer = viewer_entity.unwrap();
        window.simulate_resize(gpui_kit::size(px(900.), px(700.)));
        window.update(|_, window, cx| window.bounds_changed(cx));
        settle_file_diagrams(&mut app, &mut window, &viewer);
        let presentation = window.update(|_, _, cx| viewer.read(cx).presentation(cx).unwrap());
        let diagram = window.update(|_, window, cx| {
            let diagrams = presentation.read(cx).diagram_blocks();
            assert_eq!(diagrams.len(), 1);
            assert!(diagrams[0].read(cx).ready());
            assert!(window.find("diagram-block").visible());
            diagrams[0].clone()
        });

        let copy = window.update(|_, window, _| window.find("copy-1").bounds().center());
        window.simulate_click(copy, MouseButton::Left);
        window.update(|_, window, cx| {
            assert_eq!(presentation.read(cx).copied_block(), Some(0));
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "let output = 42;\n"
            );
            assert_eq!(window.find("copy-1").focused(), Some(true));
        });
        app.advance_clock(Duration::from_millis(1250));
        app.run_until_parked();
        window.draw();
        window.update(|_, _, cx| assert_eq!(presentation.read(cx).copied_block(), None));
        let keystroke = gpui_kit::Keystroke::parse("space").unwrap();
        window.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        window.update(|_, _, cx| {
            assert_eq!(presentation.read(cx).copied_block(), Some(0));
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "let output = 42;\n"
            );
        });

        // Exercise a real reader selection in the Escape case, and the empty state for backdrop.
        if escape {
            window.simulate_keystroke(if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            });
        }
        let selected = window.update(|_, _, cx| viewer.read(cx).selected_text(cx));
        if escape {
            assert!(selected.contains("Reader paragraph."), "{selected:?}");
        } else {
            assert!(selected.is_empty());
        }
        let scroll = window.update(|_, _, cx| viewer.read(cx).scroll_offset());
        let expand = window.update(|_, window, _| window.find("diagram-expand").bounds().center());
        window.simulate_click(expand, MouseButton::Left);
        settle_file_diagrams(&mut app, &mut window, &viewer);
        window.update(|_, window, cx| {
            assert!(diagram.read(cx).is_expanded());
            assert!(window.find("diagram-fullscreen").visible());
            assert!(!viewer.read(cx).is_closed());
            assert_eq!(viewer.read(cx).selected_text(cx), selected);
        });
        if escape {
            window.simulate_keystroke("escape");
        } else {
            // Click over the parent's Close control: the fullscreen layer must consume it.
            let backdrop = window.update(|_, window, _| {
                let at = window.find("close").bounds().center();
                assert!(window.find("diagram-fullscreen").bounds().contains(&at));
                at
            });
            window.simulate_click(backdrop, MouseButton::Left);
        }
        settle_file_diagrams(&mut app, &mut window, &viewer);
        window.update(|_, window, cx| {
            assert!(
                !viewer.read(cx).is_closed(),
                "nested dismissal must leave FileViewer open"
            );
            assert!(window.find("document").visible());
            assert!(window.try_find("diagram-fullscreen").is_none());
            assert!(!diagram.read(cx).is_expanded());
            assert!(diagram.read(cx).expand_focused(window));
            assert_eq!(viewer.read(cx).scroll_offset(), scroll);
            assert_eq!(viewer.read(cx).selected_text(cx), selected);
        });
    }

    #[test]
    fn root_file_escape_dismisses_only_diagram_and_retains_reader_selection() {
        root_file_diagram_dismissal(true);
    }

    #[test]
    fn root_file_backdrop_dismisses_diagram_without_closing_parent() {
        root_file_diagram_dismissal(false);
    }

    struct SelectableFileViewer {
        viewer: gpui_kit::Entity<FileViewer>,
    }

    impl Render for SelectableFileViewer {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(TextSelectionLayer)
                .child(self.viewer.clone())
        }
    }

    #[gpui::test]
    fn markdown_root_layout_text_theme_retention_and_empty(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let mut viewer_entity = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let viewer = cx.new(|cx| {
                FileViewer::new(
                    "README",
                    "fixtures",
                    FileContent::Text("# Native document\n\nA **rendered** paragraph.\n".into()),
                    None,
                    false,
                    window,
                    cx,
                )
            });
            viewer_entity = Some(viewer.clone());
            gpui_kit::base::Root::new(viewer, window, cx)
        });
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let viewer = viewer_entity.unwrap();
        let (document, text, focus) = window.update(|window, cx| {
            let view = viewer.read(cx);
            let document = view.document.clone().unwrap();
            let text = document.read(cx).text(cx);
            let rendered = view.rendered_text(cx).unwrap();
            assert!(rendered.contains("Native document"), "{rendered:?}");
            assert!(rendered.contains("rendered"), "{rendered:?}");
            assert!(!rendered.contains("# Native"));
            assert!(!rendered.contains("**"));
            assert!(view.source.is_none());
            assert!(view.close_focused(window));
            let bounds = window.find("document").bounds();
            let content = window.find("content").bounds();
            let width = f32::from(content.size.width).min(800.);
            assert!((f32::from(bounds.size.width) - width).abs() < 1.);
            assert!((f32::from(bounds.center().x - content.center().x)).abs() < 1.);
            assert_eq!(bounds.top(), content.top());
            let text_bounds = text.read(cx).bounds();
            assert_eq!(text_bounds.left(), bounds.left() + px(40.));
            assert_eq!(text_bounds.top(), bounds.top() + px(20.));
            assert_eq!(text_bounds.size.width, bounds.size.width - px(80.));
            text.update(cx, |text, cx| text.select_all(cx));
            (document, text, window.focused(cx))
        });
        let selected = window.update(|_, cx| viewer.read(cx).selected_text(cx));
        assert!(selected.contains("Native document"));
        for dark in [true, false] {
            window.update(|window, cx| {
                viewer.update(cx, |view, cx| view.set_theme(dark, window, cx));
                let _ = window.draw(cx);
                let view = viewer.read(cx);
                assert_eq!(view.document.as_ref().unwrap(), &document);
                assert_eq!(document.read(cx).text(cx), text);
                assert_eq!(view.selected_text(cx), selected);
                assert_eq!(window.focused(cx), focus);
            });
        }
        window.update(|_, cx| {
            viewer.update(cx, |view, cx| {
                view.set_content(
                    FileContent::Text(
                        (0..100)
                            .map(|row| format!("Paragraph {row}.\n\n"))
                            .collect(),
                    ),
                    cx,
                );
            })
        });
        window.update(|window, cx| {
            let _ = window.draw(cx);
            viewer.update(cx, |view, _| {
                assert!(view.scroll.max_offset().y > px(100.));
                view.scroll.set_offset(gpui_kit::point(px(0.), px(-100.)));
            });
            let _ = window.draw(cx);
            let offset = viewer.read(cx).scroll_offset();
            assert_eq!(offset.y, px(-100.));
            viewer.update(cx, |view, cx| view.set_theme(true, window, cx));
            let _ = window.draw(cx);
            assert_eq!(viewer.read(cx).scroll_offset(), offset);
        });
        window.update(|_, cx| {
            viewer.update(cx, |view, cx| {
                view.set_content(FileContent::Text(String::new()), cx);
            })
        });
        window.update(|window, cx| {
            let _ = window.draw(cx);
            assert!(window.find("document-empty").visible());
            assert_eq!(
                viewer.read(cx).rendered_text(cx).as_deref(),
                Some("Empty document")
            );
        });
        window.update(|_, cx| {
            viewer.update(cx, |view, cx| {
                for (filename, source) in [
                    ("example.mdx", "# Source, not a document"),
                    ("Dockerfile", "FROM alpine"),
                ] {
                    view.filename = filename.into();
                    view.set_content(FileContent::Text(source.into()), cx);
                    assert!(view.document.is_none());
                    assert!(
                        view.source.is_none(),
                        "no final newline must use exact plain text"
                    );
                    assert_eq!(view.rendered_text(cx).as_deref(), Some(source));
                    let terminated = format!("{source}\n");
                    view.set_content(FileContent::Text(terminated.clone()), cx);
                    assert!(
                        view.source.is_some(),
                        "terminated source must retain highlighting"
                    );
                    assert_eq!(view.rendered_text(cx).as_deref(), Some(terminated.as_str()));
                }
            })
        });
    }

    #[gpui::test]
    fn root_document_links_are_typed_requests_without_replacing_text(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let mut viewer_entity = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let viewer = cx.new(|cx| {
                FileViewer::new(
                    "guide.md",
                    "fixtures",
                    FileContent::Text("[Related document](docs/related.md)".into()),
                    None,
                    false,
                    window,
                    cx,
                )
            });
            viewer_entity = Some(viewer.clone());
            gpui_kit::base::Root::new(viewer, window, cx)
        });
        let viewer = viewer_entity.unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = requests.clone();
        let text = window.update(|window, cx| {
            let _ = window.draw(cx);
            let text = viewer.read(cx).selection_source(cx).unwrap();
            viewer.update(cx, |view, cx| {
                view.set_link_handler(
                    move |request| {
                        received.lock().unwrap().push(request);
                    },
                    cx,
                )
            });
            assert_eq!(viewer.read(cx).selection_source(cx).unwrap(), text);
            let _ = window.draw(cx);
            text
        });
        let bounds = window.update(|_, cx| text.read(cx).bounds());
        let at = window.update(|_, cx| {
            let state = text.read(cx);
            (0..f32::from(bounds.size.height).ceil() as usize)
                .find_map(|y| {
                    (0..f32::from(bounds.size.width).ceil() as usize).find_map(|x| {
                        let at = bounds.origin + gpui_kit::point(px(x as f32), px(y as f32));
                        (state.rendered_offset_at(at) == Some(1)).then_some(at)
                    })
                })
                .expect("document link must expose actual shaped hit geometry")
        });
        window.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        let anchor = {
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0].target,
                super::super::markdown::LinkTarget::explicit("docs/related.md").unwrap()
            );
            let anchor = requests[0].anchor;
            assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
            assert_eq!(anchor.x, bounds.left());
            assert!(anchor.y > at.y && anchor.y <= bounds.bottom());
            anchor
        };
        window.update(|window, cx| {
            assert!(!viewer.read(cx).is_closed());
            // Selection owns focus after a pointer press; Tab reaches the inline
            // control first, then the viewer close button.
            window.press("tab", cx);
            assert!(!viewer.read(cx).close_focused(window));
            window.press("tab", cx);
            assert!(viewer.read(cx).close_focused(window));
            window.press("shift-tab", cx);
            let _ = window.draw(cx);
        });
        window.update(|window, cx| window.press("enter", cx));
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            2,
            "real keyboard input must activate the document link"
        );
        assert_eq!(requests[1].target, requests[0].target);
        assert_eq!(requests[1].anchor, anchor);
        let bounds = window.update(|_, cx| text.read(cx).bounds());
        assert!(requests[1].anchor.x >= bounds.left() && requests[1].anchor.x <= bounds.right());
        assert!(requests[1].anchor.y > bounds.top() && requests[1].anchor.y <= bounds.bottom());
    }

    #[gpui::test]
    fn content_lifecycle_and_close_event(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let window = cx.add_window(|window, cx| {
            FileViewer::new(
                "note.rs",
                "src",
                FileContent::Text("let a = 1;\n".into()),
                None,
                false,
                window,
                cx,
            )
        });
        let view = cx.update(|cx| window.root(cx).unwrap());
        cx.read(|cx| assert!(view.read(cx).source.is_some()));
        cx.update(|cx| {
            let source = view.read(cx).source.clone().unwrap();
            source.update(cx, |source, cx| source.select_all(cx));
        });
        cx.read(|cx| assert!(view.read(cx).selected_text(cx).contains("let a = 1;")));
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_content(FileContent::Error("Couldn’t preview this file.".into()), cx)
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(matches!(view.content(), FileContent::Error(_)));
            assert!(view.source.is_none());
            assert_eq!(view.selected_text(cx), "");
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.set_content(FileContent::Image, cx);
                assert!(view.source.is_none());
                view.set_content(FileContent::Text("let b = 2;\n".into()), cx);
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(matches!(view.content(), FileContent::Text(text) if text == "let b = 2;\n"));
            assert!(view.source.is_some());
            assert_eq!(view.selected_text(cx), "");
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.scroll.set_offset(gpui_kit::point(px(0.), px(-50.)));
                view.set_content(FileContent::Text("let c = 3;\n".into()), cx);
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(
                view.source
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .contains("let c = 3;")
            );
            assert_eq!(view.scroll_offset().y, px(0.));
        });
        let closed = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = closed.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_, _: &CloseRequested, _| {
                count.set(count.get() + 1)
            })
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.close(cx);
                view.close(cx);
            })
        });
        cx.read(|cx| assert!(view.read(cx).is_closed()));
        assert_eq!(closed.get(), 1);
    }

    #[gpui::test]
    fn plain_text_rendered_leading_and_pointer_selection_survive_long_scroll(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| crate::components::init(cx, false));
        let text = (0..300)
            .map(|row| format!("row {row:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (root, window) = cx.add_window_view(|window, cx| {
            let viewer = cx.new(|cx| {
                FileViewer::new(
                    "note.txt",
                    "src",
                    FileContent::Text(text.clone()),
                    None,
                    false,
                    window,
                    cx,
                )
            });
            SelectableFileViewer { viewer }
        });
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let viewer = window.update(|_, cx| root.read(cx).viewer.clone());
        window.update(|_, cx| assert!(viewer.read(cx).source.is_none()));
        let bounds = window.update(|window, _| window.find("text").bounds());
        // The actual 300-line layout must retain 12 * 1.65 = 19.8px leading,
        // rather than rounding each row to 20px (60px drift by the last row).
        assert!((f32::from(bounds.size.height) - (300. * 19.8 + 60.)).abs() < 2.);

        window.update(|_, cx| {
            viewer.update(cx, |viewer, _| {
                viewer
                    .scroll
                    .set_offset(gpui_kit::point(px(0.), px(-3900.)));
            });
        });
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = window.update(|window, _| window.find("text").bounds());
        let at = |row: usize| {
            gpui_kit::point(
                bounds.left() + px(41.),
                bounds.top() + px(20. + (row as f32 + 0.5) * 19.8),
            )
        };
        window.simulate_mouse_down(at(200), MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(at(210), Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(at(210), MouseButton::Left, Modifiers::default());
        window.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(
                TextSelection::selected_text(window, cx),
                &text[200 * 8..210 * 8]
            );
        });

        let scroll = window.update(|_, cx| viewer.read(cx).scroll_offset());
        let focus = window.update(|window, cx| window.focused(cx));
        for dark in [true, false] {
            window.update(|window, cx| {
                viewer.update(cx, |viewer, cx| viewer.set_theme(dark, window, cx));
                let _ = window.draw(cx);
                assert_eq!(viewer.read(cx).scroll_offset(), scroll);
                assert_eq!(window.focused(cx), focus);
                assert_eq!(
                    TextSelection::selected_text(window, cx),
                    &text[200 * 8..210 * 8]
                );
                assert!(window.painted_quads().iter().any(|quad| {
                    quad.background == rgb(if dark { 0x000000 } else { 0xffffff }).into()
                }));
            });
        }

        window.update(|_, cx| {
            viewer.update(cx, |viewer, cx| {
                viewer.filename = "note.rs".into();
                viewer.set_content(FileContent::Text("fn main() {}".into()), cx);
                assert!(viewer.source.is_none());
                assert_eq!(viewer.rendered_text(cx).as_deref(), Some("fn main() {}"));
            });
        });
    }
}
