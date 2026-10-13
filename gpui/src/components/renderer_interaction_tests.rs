//! App-owned regressions: all geometry below is painted by the production TextView.
use super::markdown::{MarkdownView, text_style};
use gpui_kit::{
    Bounds, Context, Entity, IntoElement, MouseButton, MouseMoveEvent, Pixels, Point, Render,
    TestApp, TestAppWindow, Window,
    base::{Root, SelectionFormat, TextSelectionDomain, TextViewState, text::RangeHighlight},
    div, point,
    prelude::*,
    px, rgb, size,
};
use std::ops::Range;
use std::sync::{Arc, Mutex};

// The prepared dependency has no rendered_text_tests() observation API. Public range
// highlights paint the actual shaped glyph intervals without changing layout. Remove
// the probe before sending any input; no probe participates in selection or hit testing.
pub(crate) fn shaped_rows(
    root: &mut TestAppWindow<Root>,
    text: &Entity<TextViewState>,
    range: Range<usize>,
) -> Vec<Bounds<Pixels>> {
    let marker = rgb(0xf312b7);
    root.update(|_, _, cx| {
        text.update(cx, |text, cx| {
            text.set_range_highlights([RangeHighlight::new(range, marker)], cx)
                .unwrap();
        });
    });
    root.draw();
    let mut rows = root.update(|_, w, _| {
        let scale = w.scale_factor();
        w.painted_quads()
            .into_iter()
            .filter(|q| q.background == marker.into())
            .map(|q| {
                Bounds::from_corners(
                    point(px(q.bounds.left().0 / scale), px(q.bounds.top().0 / scale)),
                    point(
                        px(q.bounds.right().0 / scale),
                        px(q.bounds.bottom().0 / scale),
                    ),
                )
            })
            .collect::<Vec<_>>()
    });
    rows.sort_by(|a, b| {
        f32::from(a.top())
            .total_cmp(&f32::from(b.top()))
            .then_with(|| f32::from(a.left()).total_cmp(&f32::from(b.left())))
    });
    // A wrapped row can have several inline runs. Merge only intervals at the same y.
    let mut merged: Vec<Bounds<Pixels>> = Vec::new();
    for row in rows {
        if let Some(last) = merged.last_mut()
            && (last.top() - row.top()).abs() < px(0.01)
            && (last.bottom() - row.bottom()).abs() < px(0.01)
        {
            *last = Bounds::from_corners(
                point(last.left().min(row.left()), last.top()),
                point(last.right().max(row.right()), last.bottom()),
            );
        } else {
            merged.push(row);
        }
    }
    root.update(|_, _, cx| text.update(cx, |text, cx| text.clear_range_highlights(cx)));
    root.draw();
    assert!(
        !merged.is_empty(),
        "range must have real painted glyph geometry"
    );
    merged
}

pub(crate) fn assert_crlf_code_geometry(
    root: &mut TestAppWindow<Root>,
    text: &Entity<TextViewState>,
) {
    let rendered = root.read(|_, cx| text.read(cx).rendered_text());
    assert!(
        !rendered.as_str().contains('\r'),
        "CR must not reach glyph shaping"
    );
    let at = rendered.as_str().find("λ\nlast  line").unwrap();
    let glyph = shaped_rows(root, text, at..at + "λ".len());
    let code = shaped_rows(root, text, at..at + "λ\nlast  line".len());
    assert_eq!(
        code[0], glyph[0],
        "line ending must add no phantom glyph width"
    );
    assert_eq!(code.len(), 2, "CRLF must paint exactly two code rows");
    assert!(code[1].top() > code[0].top());
}

pub(crate) fn held_move(root: &mut TestAppWindow<Root>, position: Point<Pixels>) {
    root.simulate_event(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
}

fn underline_count(root: &mut TestAppWindow<Root>, rows: &[Bounds<Pixels>]) -> usize {
    root.update(|_, w, _| {
        let scale = w.scale_factor();
        w.painted_quads()
            .iter()
            .filter(|q| {
                (q.bounds.size.height.0 - scale).abs() < 0.01
                    && q.bounds.size.width.0 > 2. * scale
                    && rows.iter().any(|r| {
                        let y = q.bounds.top().0 / scale;
                        y >= f32::from(r.top())
                            && y <= f32::from(r.bottom()) + 3.
                            && q.bounds.left().0 / scale >= f32::from(r.left()) - 1.
                            && q.bounds.right().0 / scale <= f32::from(r.right()) + 1.
                    })
            })
            .count()
    })
}

#[test]
fn production_root_crlf_code_source_copy_preserves_original_after_unicode() {
    let original = "甲乙 **café** 👩‍💻\n\n```txt\n  λ\r\nlast  line\r\n```";
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| super::init(cx, dark));
        struct SourceCopyView(Entity<TextViewState>);
        impl Render for SourceCopyView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                gpui_kit::base::TextView::new(&self.0).selection_format(SelectionFormat::Source)
            }
        }
        let text = app.new_entity(|cx| TextViewState::markdown(original, cx));
        let view = app.new_entity(|_| SourceCopyView(text.clone()));
        let mut root = app.open_window(|w, cx| Root::new(view.clone(), w, cx));
        root.simulate_resize(size(px(600.), px(400.)));
        root.update(|_, w, _| w.activate_window());
        root.draw();
        assert_crlf_code_geometry(&mut root, &text);
        let first = shaped_rows(&mut root, &text, 0.."甲乙".len())[0];
        root.simulate_click(first.center(), MouseButton::Left);
        root.simulate_keystroke("cmd-a");
        root.simulate_keystroke("cmd-c");
        root.read(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().and_then(|v| v.text()),
                Some(original.into())
            )
        });
    }
}

#[test]
fn production_root_file_link_pointer_exit_schedules_unhover_and_preserves_keyboard_focus() {
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| super::init(cx, dark));
        let activated = Arc::new(Mutex::new(Vec::new()));
        let received = activated.clone();
        let view = app.new_entity(|cx| {
            let mut view =
                MarkdownView::new("src/a.rs and café", dark, |_| {}, cx).natural_height();
            view.set_target_handler(move |target| received.lock().unwrap().push(target), cx);
            view
        });
        let mut root = app.open_window(|w, cx| Root::new(view.clone(), w, cx));
        root.simulate_resize(size(px(320.), px(240.)));
        root.update(|_, w, _| w.activate_window());
        root.draw();
        let text = root.read(|_, cx| view.read(cx).text().clone());
        let rows = shaped_rows(&mut root, &text, 0..8);
        let inside = rows[0].center();
        let bounds = root.read(|_, cx| text.read(cx).bounds());
        let outside = point(bounds.right() + px(20.), bounds.bottom() + px(20.));
        root.simulate_mouse_move(inside);
        assert!(underline_count(&mut root, &rows) > 0);
        root.simulate_click(inside, MouseButton::Left);
        let focus = root.update(|_, w, cx| w.focused(cx));
        assert!(focus.is_some());
        root.simulate_mouse_move(outside);
        // NO draw/render_frame/refresh: dispatch must invalidate and complete its own frame.
        assert_eq!(
            underline_count(&mut root, &rows),
            0,
            "exit left stale hover paint"
        );
        assert_eq!(root.update(|_, w, cx| w.focused(cx)), focus);
        root.simulate_keystroke("tab");
        assert!(
            underline_count(&mut root, &rows) > 0,
            "Tab must show off-pointer focus"
        );
        root.simulate_mouse_down(inside, MouseButton::Left);
        held_move(&mut root, outside);
        held_move(&mut root, inside);
        root.simulate_mouse_up(inside, MouseButton::Left);
        assert_eq!(
            activated.lock().unwrap().len(),
            1,
            "drag-out/back must not activate"
        );
        assert_eq!(
            root.read(|_, cx| view.read(cx).source().to_owned()),
            "src/a.rs and café"
        );
    }
}

struct Sources {
    views: Vec<Entity<MarkdownView>>,
}
impl Render for Sources {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(190.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(self.views.iter().cloned())
    }
}

#[test]
fn production_root_multisource_forward_backward_fractional_gaps_keep_selection_and_blue_paint() {
    let sources = [
        "甲乙 café 👩‍💻 words that wrap across several visual rows in this narrow paragraph.\n\nsecond paragraph",
        "- list λ\n- list café\n\n```txt\n  raw λ\r\nlast  line\r\n```",
    ];
    for dark in [false, true] {
        let mut app = TestApp::new();
        app.update(|cx| super::init(cx, dark));
        let domain = TextSelectionDomain::new(
            |item, _| (item < 2).then_some(item),
            move |range, _| {
                let (a, b) = if (range.anchor.item, range.anchor.offset)
                    <= (range.cursor.item, range.cursor.offset)
                {
                    (range.anchor, range.cursor)
                } else {
                    (range.cursor, range.anchor)
                };
                (a.item..=b.item)
                    .map(|item| {
                        TextViewState::copy_markdown_slice(
                            sources[item as usize],
                            (item == a.item).then_some(a.offset),
                            (item == b.item).then_some(b.offset),
                            SelectionFormat::Plain,
                        )
                        .unwrap()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        );
        let host = app.new_entity(|cx| Sources {
            views: sources
                .iter()
                .enumerate()
                .map(|(item, source)| {
                    cx.new(|cx| {
                        let view = MarkdownView::new(*source, dark, |_| {}, cx).natural_height();
                        view.text().update(cx, |text, cx| {
                            text.set_selection_domain(Some((domain.clone(), item as u64)), cx)
                        });
                        view
                    })
                })
                .collect(),
        });
        let mut root = app.open_window(|w, cx| Root::new(host.clone(), w, cx));
        root.simulate_resize(size(px(400.), px(900.)));
        root.update(|_, w, _| w.activate_window());
        root.draw();
        let texts = root.read(|_, cx| {
            host.read(cx)
                .views
                .iter()
                .map(|v| v.read(cx).text().clone())
                .collect::<Vec<_>>()
        });
        let mut lines = Vec::new();
        for (item, text) in texts.iter().enumerate() {
            let len = root.read(|_, cx| text.read(cx).rendered_text().len());
            lines.extend(
                shaped_rows(&mut root, text, 0..len)
                    .into_iter()
                    .map(|bounds| (item, bounds)),
            );
        }
        assert!(
            lines.len() >= 7,
            "fixture must wrap and contain paragraph/list/code rows"
        );
        let first = point(lines[0].1.left() + px(0.1), lines[0].1.center().y);
        let last_row = lines.last().unwrap().1;
        let last = point(last_row.right() + px(0.1), last_row.center().y);
        let pairs = lines
            .windows(2)
            .filter(|p| p[1].1.top() > p[0].1.bottom())
            .collect::<Vec<_>>();
        assert!(
            pairs.iter().any(|p| p[0].0 != p[1].0),
            "must cross source owners"
        );
        assert!(
            pairs.iter().any(|p| p[0].0 == p[1].0),
            "must test internal visual gaps too"
        );
        for forward in [true, false] {
            root.simulate_mouse_down(if forward { first } else { last }, MouseButton::Left);
            let ordered = if forward {
                pairs.clone()
            } else {
                pairs.iter().rev().copied().collect()
            };
            for pair in ordered {
                for fraction in if forward {
                    [0.125, 0.375, 0.625, 0.875]
                } else {
                    [0.875, 0.625, 0.375, 0.125]
                } {
                    let head = point(
                        pair[0].1.center().x,
                        pair[0].1.bottom() + (pair[1].1.top() - pair[0].1.bottom()) * fraction,
                    );
                    let (item, boundary) = if fraction < 0.5 {
                        (
                            pair[0].0,
                            point(pair[0].1.right() + px(2.), pair[0].1.center().y),
                        )
                    } else {
                        (
                            pair[1].0,
                            point(pair[1].1.left() - px(2.), pair[1].1.center().y),
                        )
                    };
                    let expected_offset = root
                        .read(|_, cx| texts[item].read(cx).rendered_offset_at(boundary).unwrap());
                    held_move(&mut root, head);
                    let mut preserved = None;
                    for _ in 0..3 {
                        root.draw();
                        root.update(|_, w, cx| {
                            let range =
                                domain.range(cx).expect("held gap erased logical selection");
                            assert_eq!(range.cursor.item, item as u64);
                            assert_eq!(
                                range.cursor.offset, expected_offset,
                                "gap must choose nearest boundary, not stale head"
                            );
                            if let Some(before) = preserved {
                                assert_eq!(range, before);
                            }
                            preserved = Some(range);
                            let color = text_style(dark, w).selection();
                            let blue = w
                                .painted_quads()
                                .into_iter()
                                .filter(|q| {
                                    q.background == color.into()
                                        && q.bounds.size.height.0 > w.scale_factor()
                                })
                                .collect::<Vec<_>>();
                            assert!(
                                !blue.is_empty(),
                                "held selection must paint actual blue, not only preserve metadata"
                            );
                            for (owner, row) in &lines {
                                let offset = texts[*owner]
                                    .read(cx)
                                    .rendered_offset_at(row.center())
                                    .unwrap();
                                if texts[*owner]
                                    .read(cx)
                                    .selected_rendered_range_with(cx)
                                    .is_some_and(|r| r.start < offset && offset < r.end)
                                {
                                    let center = row.scale(w.scale_factor()).center();
                                    assert!(
                                        blue.iter().any(|q| q.bounds.contains(&center)),
                                        "selected glyph row lacks blue coverage"
                                    );
                                }
                            }
                        });
                    }
                    root.simulate_keystroke("cmd-c");
                    root.read(|_, cx| {
                        let range = domain.range(cx).unwrap();
                        let (a, b) = if (range.anchor.item, range.anchor.offset)
                            <= (range.cursor.item, range.cursor.offset)
                        {
                            (range.anchor, range.cursor)
                        } else {
                            (range.cursor, range.anchor)
                        };
                        let expected = (a.item..=b.item)
                            .map(|item| {
                                let rendered = texts[item as usize].read(cx).rendered_text();
                                rendered.as_str()[(if item == a.item { a.offset } else { 0 })
                                    ..(if item == b.item {
                                        b.offset
                                    } else {
                                        rendered.len()
                                    })]
                                    .to_owned()
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        assert_eq!(
                            cx.read_from_clipboard().and_then(|v| v.text()),
                            Some(expected)
                        );
                    });
                }
            }
            root.simulate_mouse_up(if forward { last } else { first }, MouseButton::Left);
        }
        // Adjacent wrapped visual rows can share an edge rather than have block
        // padding. Sample fractional y on both sides of that measured seam too.
        let seams = lines
            .windows(2)
            .filter(|p| {
                p[0].0 == 0 && p[1].0 == 0 && (p[1].1.top() - p[0].1.bottom()).abs() < px(0.01)
            })
            .collect::<Vec<_>>();
        assert!(
            !seams.is_empty(),
            "fixture must contain adjacent wrapped visual rows"
        );
        for forward in [true, false] {
            root.simulate_mouse_down(if forward { first } else { last }, MouseButton::Left);
            for pair in &seams {
                let epsilon = pair[0].1.size.height.min(pair[1].1.size.height) * 0.025;
                for fraction in [0.125, 0.375, 0.625, 0.875] {
                    let head = point(
                        pair[0].1.center().x,
                        pair[0].1.bottom() - epsilon + epsilon * 2. * fraction,
                    );
                    let expected =
                        root.read(|_, cx| texts[0].read(cx).rendered_offset_at(head).unwrap());
                    held_move(&mut root, head);
                    for _ in 0..3 {
                        root.draw();
                        root.update(|_, w, cx| {
                            let range = domain.range(cx).unwrap();
                            assert_eq!(range.cursor.item, 0);
                            assert_eq!(range.cursor.offset, expected);
                            let projected =
                                texts[0].read(cx).selected_rendered_range_with(cx).unwrap();
                            assert!(projected.start <= expected && expected <= projected.end);
                            let color = text_style(dark, w).selection();
                            assert!(
                                w.painted_quads()
                                    .iter()
                                    .any(|q| q.background == color.into()
                                        && q.bounds.size.height.0 > w.scale_factor())
                            );
                        });
                    }
                }
            }
            root.simulate_mouse_up(if forward { last } else { first }, MouseButton::Left);
        }
        root.read(|_, cx| {
            let rendered = texts[0].read(cx).rendered_text();
            let at = rendered.as_str().find("👩‍💻").unwrap();
            let raw = texts[0]
                .read(cx)
                .source_range_for_rendered_range(at..at + "👩‍💻".len())
                .unwrap();
            assert_eq!(&sources[0][raw], "👩‍💻");
            let code = texts[1].read(cx).rendered_text();
            assert_eq!(
                code.as_str().trim_end_matches('\n'),
                "list λ\nlist café\n  raw λ\nlast  line"
            );
            let at = code.as_str().find("λ\nlast").unwrap();
            let mapped = texts[1]
                .read(cx)
                .source_range_for_rendered_range(at..at + "λ\nlast".len())
                .unwrap();
            assert_eq!(
                &sources[1][mapped], "λ\r\nlast",
                "code projection must retain exact raw CRLF bytes after Unicode"
            );
        });
        assert_crlf_code_geometry(&mut root, &texts[1]);
    }
}
