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

#[derive(Clone)]
pub enum FileContent {
    Text(String),
    Image,
    Error(String),
    Unsupported,
}

#[derive(Clone)]
pub struct CloseRequested;

const PLAIN_LINE_HEIGHT: f32 = 1.65;

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
    _keys: gpui_kit::Subscription,
}

impl FileViewer {
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
        let source = if matches!(content, FileContent::Text(_)) {
            let filename = filename.as_str();
            let language = filename.rsplit_once('.').map(|(_, extension)| extension);
            language
                .filter(|extension| matches!(*extension, "rs" | "js" | "ts" | "json" | "py"))
                .map(|extension| {
                    let code = match &content {
                        FileContent::Text(text) => text.as_str(),
                        _ => "",
                    };
                    // A single fenced block retains Base's syntax spans and native selection.
                    // Strip the trailing newline before the fence so the preview has no extra line.
                    let fenced = format!("```{extension}\n{}\n```", code.trim_end_matches('\n'));
                    cx.new(|cx| TextViewState::markdown(&fenced, cx).selectable(true))
                })
        } else {
            None
        };
        let weak = cx.weak_entity();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(scene) = weak.upgrade() else { return };
            let key = event.keystroke.key.as_str();
            let modifiers = event.keystroke.modifiers;
            if key == "escape" {
                scene.update(cx, |scene, cx| {
                    scene.close(cx);
                });
                cx.stop_propagation();
            } else if key == "a" && (modifiers.platform || modifiers.control) {
                let source = scene.read(cx).source.clone();
                if let Some(source) = source {
                    source.update(cx, |state, cx| state.select_all(cx));
                } else {
                    TextSelection::select_all(window, cx);
                }
                cx.stop_propagation();
            } else if !modifiers.platform && !modifiers.control && !modifiers.alt {
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
            _keys: keys,
        }
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
    pub fn selected_text(&self, cx: &gpui_kit::App) -> String {
        self.source
            .as_ref()
            .map_or_else(String::new, |source| source.read(cx).selected_text())
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
    pub fn set_content(&mut self, content: FileContent, cx: &mut Context<Self>) {
        self.source = if let FileContent::Text(text) = &content {
            self.highlighted_source(text, cx)
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
        let extension = self.filename.rsplit_once('.')?.1;
        if !matches!(extension, "rs" | "js" | "ts" | "json" | "py") {
            return None;
        }
        let fenced = format!("```{extension}\n{}\n```", text.trim_end_matches('\n'));
        Some(cx.new(|cx| TextViewState::markdown(&fenced, cx).selectable(true)))
    }
    fn kind(&self) -> &str {
        match self.content {
            FileContent::Text(_) => "text",
            FileContent::Image => "image",
            FileContent::Error(_) => "error",
            FileContent::Unsupported => "unsupported",
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
                    .max_w(px(620.))
                    .text_size(px(11.))
                    .child(div().text_color(rgb(colors.text)).child(filename))
                    .child(div().text_color(rgb(colors.muted)).child(directory)),
            );
        let close_focused = self.close_focus.is_focused(window);
        let close = gpui_kit::base::Button::new("close")
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
                    .h(px(654.))
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
            "error" | "unsupported" => content.child(
                div()
                    .h(px(654.))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(colors.muted))
                    .text_size(px(11.))
                    .child(if let FileContent::Error(ref message) = self.content {
                        message.clone()
                    } else {
                        "Preview unavailable.".to_string()
                    }),
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
        page.child(content)
            .child(header)
            .child(close)
            .when(kind == "error", |page| {
                page.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(colors.muted))
                        .text_size(px(11.))
                        .child(if let FileContent::Error(ref message) = self.content {
                            message.clone()
                        } else {
                            "Couldn’t preview this file.".to_string()
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Modifiers, MouseButton, TestAppContext, base::TextSelectionLayer, gpui, test::TestWindowExt,
    };

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
    fn content_lifecycle_and_close_event(cx: &mut TestAppContext) {
        cx.update(|cx| crate::components::init(cx, false));
        let window = cx.add_window(|window, cx| {
            FileViewer::new(
                "note.rs",
                "src",
                FileContent::Text("let a = 1;".into()),
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
                view.set_content(FileContent::Text("let b = 2;".into()), cx);
            })
        });
        cx.read(|cx| {
            let view = view.read(cx);
            assert!(matches!(view.content(), FileContent::Text(text) if text == "let b = 2;"));
            assert!(view.source.is_some());
            assert_eq!(view.selected_text(cx), "");
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.scroll.set_offset(gpui_kit::point(px(0.), px(-50.)));
                view.set_content(FileContent::Text("let c = 3;".into()), cx);
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

        window.update(|_, cx| {
            viewer.update(cx, |viewer, cx| {
                viewer.filename = "note.rs".into();
                viewer.set_content(FileContent::Text("fn main() {}".into()), cx);
                assert!(viewer.source.is_some());
            });
        });
    }
}
