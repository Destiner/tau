//! Marker work must not move prose or change source-owned selection.
//! Ink size needs the separately gated actual-WebKit/offscreen-Metal comparison.
use super::{MarkdownView, text_style_at_size};
use crate::components::renderer_interaction_tests::shaped_rows;
use gpui_kit::{
    Context, Entity, IntoElement, Render, TestApp, Window,
    base::{Root, TextViewState},
    div,
    prelude::*,
    px, size,
};

struct MarkerHost {
    markdown: Entity<MarkdownView>,
    dark: bool,
}

impl Render for MarkerHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::components::support::root(self.dark)
            .child(div().size_full().p(px(24.)).child(self.markdown.clone()))
    }
}

#[test]
fn markdown_marker_columns_keep_body_rows_and_source_selection() {
    for dark in [false, true] {
        for scale in [1., 2.] {
            let mut reference = None;
            for source in [
                "- Marker one\n- Marker two",
                "7. Marker one\n8. Marker two",
                "- [ ] Marker one\n- [x] Marker two",
            ] {
                let mut app = TestApp::new();
                app.update(|cx| crate::components::init(cx, dark));
                let markdown = app
                    .new_entity(|cx| MarkdownView::new(source, dark, |_| {}, cx).natural_height());
                let text = app.update(|cx| markdown.read(cx).text().clone());
                let host = app.new_entity(|_| MarkerHost { markdown, dark });
                let mut root = app.open_window(|w, cx| Root::new(host, w, cx));
                root.simulate_resize(size(px(360.), px(240.)));
                root.simulate_scale_factor_change(scale);
                root.draw();
                app.run_until_parked();
                root.draw();
                let rendered =
                    root.read(|_, cx| text.read(cx).rendered_text().as_str().to_string());
                let rows = shaped_rows(&mut root, &text, 0..rendered.len());
                assert_eq!(rows.len(), 2, "{source}/{dark}/{scale}");
                let bounds = root.read(|_, cx| text.read(cx).bounds());
                let task = source.starts_with("- [ ]");
                let indent = if task {
                    13. * (0.95 + 0.45 + 0.25)
                } else {
                    18.2
                };
                for row in &rows {
                    assert!(
                        (row.left() - bounds.left() - px((indent * scale).round() / scale)).abs()
                            < px(0.1),
                        "{source}/{dark}/{scale}: row={row:?}, bounds={bounds:?}, indent={indent}"
                    );
                }
                if task {
                    root.update(|_, w, _| {
                        let expected = (13. * 0.95 * scale).round();
                        let mut checkboxes = Vec::new();
                        for quad in w.painted_quads().into_iter().filter(|quad| {
                            (quad.bounds.size.width.0 - expected).abs() < 0.1
                                && quad.bounds.size.width == quad.bounds.size.height
                                && quad.border_widths.top.0 > 0.
                        }) {
                            if !checkboxes.contains(&quad.bounds) {
                                checkboxes.push(quad.bounds);
                            }
                        }
                        assert_eq!(checkboxes.len(), 2, "both original task checkbox borders");
                    });
                }
                let row_step = rows[1].top() - rows[0].top();
                if let Some(expected) = reference {
                    assert_eq!(row_step, expected, "unordered/ordered/task row spacing");
                } else {
                    reference = Some(row_step);
                }
                root.update(|_, _, cx| text.update(cx, |text, cx| text.select_all(cx)));
                root.read(|_, cx| {
                    let state: &TextViewState = text.read(cx);
                    assert_eq!(state.selected_text().trim(), "Marker one\nMarker two");
                    assert_eq!(state.selected_source_range(), Some(0..source.len()));
                });
            }
        }
    }
}

#[test]
fn markdown_marker_style_keeps_existing_body_metrics_at_both_sizes() {
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let mut root = app.open_window(|w, cx| {
            let host = cx.new(|cx| MarkerHost {
                markdown: cx.new(|cx| MarkdownView::new("- Marker", dark, |_| {}, cx)),
                dark,
            });
            Root::new(host, w, cx)
        });
        root.update(|_, w, _| {
            for body in [11., 13.] {
                let style = text_style_at_size(dark, body, w);
                let (indent, gap, _) = style.list_style().expect("production marker style");
                assert_eq!(indent, px(body * 1.4));
                assert_eq!(gap, px(body * 0.28));
                assert_eq!(style.body_text_size(), Some(px(body)));
            }
        });
    }
}
