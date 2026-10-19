use std::sync::Arc;

#[path = "frontmatter.rs"]
mod frontmatter;

use gpui_kit::{
    App, Context, Entity, IntoElement, Render, TestSupportExt, Window,
    base::{SelectableText, TextViewState},
    div,
    prelude::*,
    px, rgb,
};

use super::super::{markdown::MarkdownView, support};

/// A request only: the caller owns path resolution, permissions and navigation.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownLinkRequested {
    pub target: super::super::markdown::LinkTarget,
    pub anchor: gpui_kit::Point<gpui_kit::Pixels>,
}
impl MarkdownLinkRequested {
    /// Unsupported schemes/fragments are inert, never opened as local files.
    pub fn destination(&self) -> Option<super::super::markdown::LinkTarget> {
        Some(self.target.clone())
    }
}

pub fn is_document(filename: &str) -> bool {
    let name = filename.to_ascii_lowercase();
    name == "readme"
        || name.rsplit_once('.').is_some_and(|(base, extension)| {
            !base.is_empty() && matches!(extension, "md" | "markdown")
        })
}

pub struct MarkdownDocument {
    source: String,
    fields: Vec<frontmatter::Field>,
    dark: bool,
    view: Entity<MarkdownView>,
    on_link: Arc<dyn Fn(MarkdownLinkRequested) + Send + Sync>,
}

impl MarkdownDocument {
    pub fn new(
        source: String,
        dark: bool,
        on_link: impl Fn(MarkdownLinkRequested) + Send + Sync + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = frontmatter::parse(&source);
        let body = preview.body.to_owned();
        let fields = preview.fields;
        let on_link: Arc<dyn Fn(MarkdownLinkRequested) + Send + Sync> = Arc::new(on_link);
        let handler = on_link.clone();
        let view = cx.new(|cx| {
            let mut view = MarkdownView::new(body.clone(), dark, |_| {}, cx).natural_height();
            view.set_anchored_target_handler(
                move |target, anchor| handler(MarkdownLinkRequested { target, anchor }),
                cx,
            );
            view
        });
        Self {
            source: body,
            fields,
            dark,
            view,
            on_link,
        }
    }

    pub fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dark = dark;
        self.view
            .update(cx, |view, cx| view.set_theme(dark, window, cx));
        cx.notify();
    }

    pub fn set_link_handler(
        &mut self,
        handler: Arc<dyn Fn(MarkdownLinkRequested) + Send + Sync>,
        cx: &mut Context<Self>,
    ) {
        self.on_link = handler.clone();
        self.view.update(cx, |view, cx| {
            view.set_anchored_target_handler(
                move |target, anchor| handler(MarkdownLinkRequested { target, anchor }),
                cx,
            );
        });
        cx.notify();
    }

    pub fn set_context_handler(
        &mut self,
        handler: impl Fn(
            super::super::markdown::LinkTarget,
            &gpui_kit::MouseDownEvent,
            &mut Window,
            &mut App,
        ) + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) {
        self.view
            .update(cx, |view, cx| view.set_context_handler(handler, cx));
        cx.notify();
    }

    pub fn presentation(&self, cx: &App) -> Entity<super::super::markdown::MarkdownPresentation> {
        self.view.read(cx).presentation().clone()
    }

    pub fn text(&self, cx: &App) -> Entity<TextViewState> {
        self.view.read(cx).text().clone()
    }

    pub fn rendered_text(&self, cx: &App) -> String {
        if self.source.is_empty() && self.fields.is_empty() {
            "Empty document".into()
        } else {
            let mut text = self
                .fields
                .iter()
                .map(|f| format!("{}{}", f.key, f.value))
                .collect::<String>();
            text.push_str(self.text(cx).read(cx).rendered_text().as_str());
            text
        }
    }
}

impl Render for MarkdownDocument {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let colors = support::tokens(self.dark);
        let panel = div()
            .id("document-frontmatter")
            .test_support()
            .w_full()
            .mb(px(24.))
            .p(px(8.))
            .border_1()
            .border_color(rgb(colors.border))
            .rounded(px(6.))
            .bg(rgb(colors.panel))
            .font_family(super::super::font::family())
            .text_size(px(12.))
            .line_height(gpui_kit::relative(1.5))
            .text_color(rgb(colors.text))
            .flex()
            .flex_col()
            .gap(px(9.))
            .children(self.fields.iter().enumerate().map(|(index, field)| {
                div()
                    .id(("metadata-row", index))
                    .test_support()
                    .w_full()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .id(("metadata-key", index))
                            .test_support()
                            .w(px(120.))
                            .flex_shrink_0()
                            .text_color(rgb(colors.muted))
                            .child(SelectableText::new(
                                ("metadata-key-text", index),
                                field.key.clone(),
                            )),
                    )
                    .child(
                        div()
                            .id(("metadata-value", index))
                            .test_support()
                            .flex_1()
                            .min_w(px(0.))
                            .child(SelectableText::new(
                                ("metadata-value-text", index),
                                field.value.clone(),
                            )),
                    )
            }));
        div()
            .id("document")
            .test_support()
            .w_full()
            .max_w(px(800.))
            .min_h_full()
            .px(px(40.))
            .pt(px(20.))
            .pb(px(60.))
            .when(!self.fields.is_empty(), |body| body.child(panel))
            .when(self.source.is_empty() && self.fields.is_empty(), |body| {
                body.child(
                    div()
                        .id("document-empty")
                        .test_support()
                        .text_color(rgb(support::tokens(self.dark).muted))
                        .child("Empty document"),
                )
            })
            .when(!self.source.is_empty(), |body| {
                body.child(self.view.clone())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, gpui, test::TestWindowExt};

    #[gpui::test]
    fn metadata_panel_paint_layout_selection_and_theme(cx: &mut gpui::TestAppContext) {
        use gpui_kit::base::{Root, TextSelection};
        use gpui_kit::{Modifiers, MouseButton, point};
        cx.update(|cx| crate::components::init(cx, false));
        let mut retained = None;
        let (_, window) = cx.add_window_view(|window, cx| {
            let document = cx.new(|cx| {
                MarkdownDocument::new(
                    "---\ntitle: September release\ntags: [desktop, release]\n---\n# Body".into(),
                    false,
                    |_| {},
                    cx,
                )
            });
            retained = Some(document.clone());
            Root::new(document, window, cx)
        });
        let document = retained.unwrap();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = window.update(|window, _| {
            let document = window.find("document").bounds();
            let panel = window.find("document-frontmatter").bounds();
            let key = window.find(("metadata-key", 0usize)).bounds();
            let value = window.find(("metadata-value", 0usize)).bounds();
            assert_eq!(panel.left() - document.left(), px(40.));
            assert_eq!(panel.top() - document.top(), px(20.));
            assert_eq!(key.left() - panel.left(), px(9.));
            assert_eq!(key.size.width, px(120.));
            assert_eq!(value.left() - key.right(), px(12.));
            assert_eq!(key.size.height, px(18.));
            assert!(
                window
                    .painted_quads()
                    .iter()
                    .any(|q| q.background == rgb(support::tokens(false).panel).into())
            );
            value
        });
        window.simulate_mouse_down(
            point(bounds.left() + px(1.), bounds.top() + px(9.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.simulate_mouse_move(
            point(bounds.right() - px(1.), bounds.top() + px(9.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        window.simulate_mouse_up(
            point(bounds.right() - px(1.), bounds.top() + px(9.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(
                TextSelection::selected_text(window, cx),
                "September release"
            );
        });
        for dark in [true, false] {
            window.update(|window, cx| {
                document.update(cx, |document, cx| document.set_theme(dark, window, cx));
                let _ = window.draw(cx);
                assert_eq!(
                    TextSelection::selected_text(window, cx),
                    "September release"
                );
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .any(|q| q.background == rgb(support::tokens(dark).panel).into())
                );
                let text = document.read(cx).rendered_text(cx);
                assert!(text.contains("September release"));
                assert!(text.contains("Body"));
                assert!(!text.contains("# Body"));
            });
        }
    }

    #[gpui::test]
    fn metadata_only_and_empty_frontmatter(cx: &mut gpui::TestAppContext) {
        use gpui_kit::base::Root;
        cx.update(|cx| crate::components::init(cx, false));
        for (source, has_fields) in [
            ("---\ntitle: Only metadata\n---\n", true),
            ("---\n---\n", false),
        ] {
            let (_, window) = cx.add_window_view(|window, cx| {
                let document = cx.new(|cx| MarkdownDocument::new(source.into(), true, |_| {}, cx));
                Root::new(document, window, cx)
            });
            window.update(|window, cx| {
                let _ = window.draw(cx);
                assert_eq!(
                    gpui_kit::base::test_support::find(window, &[], &"document-frontmatter".into())
                        .is_some(),
                    has_fields
                );
                assert_eq!(
                    gpui_kit::base::test_support::find(window, &[], &"document-empty".into())
                        .is_some(),
                    !has_fields
                );
            });
        }
    }

    #[test]
    fn production_document_names_not_all_markdown_languages() {
        for filename in ["README", "readme", "Guide.MD", "guide.markdown"] {
            assert!(is_document(filename), "{filename}");
        }
        for filename in ["guide.mdx", ".md", "readme.txt", "sample.rs"] {
            assert!(!is_document(filename), "{filename}");
        }
    }
}
