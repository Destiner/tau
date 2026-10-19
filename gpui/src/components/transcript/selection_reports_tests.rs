//! Selection reports exercised through production Root, Transcript and TextView geometry.
use super::*;
use crate::components::renderer_interaction_tests::{held_move, shaped_rows};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    MouseButton, MouseDownEvent, MouseUpEvent, Pixels, TestApp, TestAppWindow,
    base::{Root, SelectionFormat, TestSupportExt, TextSelection},
    point, size,
};

const REPORTED: &str = "For the remaining work, I’d make native build/release CI and the updater ready on `gpui`, then merge and remove Vue/Tauri. That avoids leaving `main` without a working release path.";

fn settle(app: &mut TestApp, root: &mut TestAppWindow<Root>) {
    for _ in 0..5 {
        app.run_until_parked();
        root.draw();
    }
}

fn click_three(root: &mut TestAppWindow<Root>, position: gpui_kit::Point<Pixels>) {
    root.simulate_mouse_move(position);
    root.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        click_count: 3,
        modifiers: Default::default(),
        first_mouse: false,
    });
    root.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        click_count: 3,
        modifiers: Default::default(),
    });
    root.draw();
}

#[test]
fn triple_click_selects_one_logical_paragraph_across_inline_flow_fragments() {
    let plain = REPORTED.replace('`', "");
    let marked = REPORTED
        .replace("remaining", "**remaining**")
        .replace("release path", "[release path](https://example.test)");
    for dark in [false, true] {
        for width in [420., 760.] {
            for paragraph in [REPORTED, plain.as_str(), marked.as_str()] {
                // The list variant must not select the entire item or its next paragraph.
                for list in [false, true] {
                    let source = if list {
                        format!("Before.\n\n- {paragraph}\n\n  Adjacent paragraph.\n\nAfter.")
                    } else {
                        format!("Before.\n\n{paragraph}\n\nAdjacent paragraph.")
                    };
                    let expected = paragraph
                        .replace("**", "")
                        .replace('`', "")
                        .replace("[release path](https://example.test)", "release path");
                    let mut app = TestApp::new();
                    app.update(|cx| crate::components::init(cx, dark));
                    let transcript = app.new_entity(|cx| {
                        let mut t = Transcript::new("selection-report", dark, cx);
                        t.set_entries(
                            vec![TranscriptEntry {
                                id: "message".into(),
                                content: TranscriptContent::Assistant(source.clone()),
                            }],
                            cx,
                        )
                        .unwrap();
                        t
                    });
                    let mut root = app.open_window(|w, cx| {
                        Root::new(transcript.clone(), w, cx).flex().flex_col()
                    });
                    root.simulate_resize(size(px(width), px(900.)));
                    root.update(|_, w, cx| {
                        w.activate_window();
                        w.bounds_changed(cx);
                    });
                    settle(&mut app, &mut root);
                    let text = root.read(|_, cx| {
                        let t = transcript.read(cx);
                        let View::Prose(text, _) = &t.cache[&t.rows[0].key].view else {
                            panic!()
                        };
                        text.clone()
                    });
                    let rendered =
                        root.read(|_, cx| text.read(cx).rendered_text().as_str().to_owned());
                    let start = rendered
                        .find(&expected)
                        .expect("production rendered paragraph");
                    let end = start + expected.len();
                    for needle in ["remaining", "gpui", "release path"] {
                        let at = start + expected.find(needle).unwrap();
                        let rows = shaped_rows(&mut root, &text, at..at + needle.len());
                        click_three(&mut root, rows[0].center());
                        root.read(|_, cx| {
                            let t = transcript.read(cx);
                            let range = t
                                .selection
                                .range(cx)
                                .expect("source-owned paragraph selection");
                            assert_eq!(range.anchor.item, t.rows[0].key);
                            assert_eq!(range.cursor.item, t.rows[0].key);
                            assert_eq!(range.anchor.offset, start);
                            assert_eq!(range.cursor.offset, end, "dark={dark} width={width} list={list} needle={needle} source={source}");
                            assert_eq!(t.copy_selection(range), expected);
                            assert_eq!(text.read(cx).selected_text_with(cx), expected);
                            let mapped = text.read(cx).selected_source_range_with(cx).unwrap();
                            assert_eq!(&source[mapped], paragraph);
                            assert!(t.cache.len() <= CACHE_LIMIT);
                        });
                        root.simulate_keystroke("cmd-c");
                        root.read(|_, cx| {
                            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected)
                        });
                    }
                    // Reversing an ordinary drag over the same painted source must
                    // retain exact directed UTF-8 endpoints, including curly apostrophes.
                    let rows = shaped_rows(&mut root, &text, start..end);
                    let a = point(rows[0].left() + px(0.1), rows[0].center().y);
                    let b = point(
                        rows.last().unwrap().right() + px(0.1),
                        rows.last().unwrap().center().y,
                    );
                    root.simulate_mouse_down(b, MouseButton::Left);
                    held_move(&mut root, a);
                    root.simulate_mouse_up(a, MouseButton::Left);
                    root.draw();
                    root.read(|_, cx| {
                        let range = transcript.read(cx).selection.range(cx).unwrap();
                        assert_eq!((range.anchor.offset, range.cursor.offset), (end, start));
                        assert_eq!(transcript.read(cx).copy_selection(range), expected);
                        assert_eq!(
                            TextViewState::copy_markdown_slice(
                                &source,
                                None,
                                None,
                                SelectionFormat::Source
                            )
                            .unwrap(),
                            source
                        );
                    });
                }
            }
        }
    }
}

struct SwitchHost {
    transcripts: [Entity<Transcript>; 2],
    active: usize,
}
impl Render for SwitchHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("unrelated-switch-control")
                    .test_support()
                    .w(px(100.))
                    .h_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.active = 1 - this.active;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child("Switch"),
            )
            .child(self.transcripts[self.active].clone())
    }
}

#[test]
fn unrelated_pointer_down_cannot_start_markdown_selection_after_switch_and_release_is_global() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let transcripts = ["first", "second"].map(|id| {
        app.new_entity(|cx| {
            let mut t = Transcript::new(id, false, cx);
            t.set_entries(
                vec![TranscriptEntry {
                    id: "same-message-id".into(),
                    content: TranscriptContent::Assistant(
                        "A paragraph with `code` and a retained source identity.".into(),
                    ),
                }],
                cx,
            )
            .unwrap();
            t
        })
    });
    let host = app.new_entity(|_| SwitchHost {
        transcripts: transcripts.clone(),
        active: 0,
    });
    let mut root = app.open_window(|w, cx| Root::new(host.clone(), w, cx).flex().flex_col());
    root.simulate_resize(size(px(760.), px(600.)));
    root.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    settle(&mut app, &mut root);
    for _ in 0..4 {
        let control = root.update(|_, w, _| w.find("unrelated-switch-control").bounds().center());
        root.simulate_mouse_down(control, MouseButton::Left);
        settle(&mut app, &mut root);
        let text = root.read(|_, cx| {
            let t = transcripts[host.read(cx).active].read(cx);
            let View::Prose(text, _) = &t.cache[&t.rows[0].key].view else {
                panic!()
            };
            text.clone()
        });
        let rows = shaped_rows(&mut root, &text, 0..10);
        held_move(&mut root, rows[0].center());
        root.read(|_, cx| {
            for t in &transcripts {
                assert!(
                    !t.read(cx).selection.is_selecting(),
                    "an unrelated press cannot own a reader drag"
                );
                assert!(
                    t.read(cx).selection.stored_range().is_none(),
                    "a control press must not admit a Markdown gesture"
                );
            }
        });
        root.simulate_mouse_up(control, MouseButton::Left);
        // A genuine text gesture is admitted, but release over a control that
        // stops propagation must freeze it even if another held move arrives.
        root.simulate_mouse_down(rows[0].center(), MouseButton::Left);
        held_move(&mut root, point(rows[0].right(), rows[0].center().y));
        root.simulate_mouse_up(control, MouseButton::Left);
        let before = root.read(|_, cx| {
            transcripts[host.read(cx).active]
                .read(cx)
                .selection
                .stored_range()
        });
        assert!(before.is_some(), "a genuine text press must still select");
        held_move(&mut root, point(rows[0].left(), rows[0].center().y));
        root.read(|_, cx| {
            let t = transcripts[host.read(cx).active].read(cx);
            assert_eq!(
                t.selection.stored_range(),
                before,
                "stopped bubble must not retain a drag"
            );
            assert!(!t.selection.is_selecting());
        });
        root.simulate_mouse_down(rows[0].center(), MouseButton::Left);
        held_move(&mut root, point(rows[0].right(), rows[0].center().y));
        let before = root.read(|_, cx| {
            transcripts[host.read(cx).active]
                .read(cx)
                .selection
                .stored_range()
        });
        root.simulate_mouse_move(point(rows[0].left(), rows[0].center().y));
        root.read(|_, cx| {
            let t = transcripts[host.read(cx).active].read(cx);
            assert_eq!(
                t.selection.stored_range(),
                before,
                "button-free move must finish, not extend, a missed release"
            );
            assert!(!t.selection.is_selecting());
        });
        root.simulate_mouse_up(control, MouseButton::Left);
        root.update(|_, w, cx| TextSelection::clear(w, cx));
    }
}

#[test]
fn stopped_release_cannot_leave_a_real_markdown_drag_live() {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let transcripts = ["first", "second"].map(|id| {
        app.new_entity(|cx| {
            let mut t = Transcript::new(id, false, cx);
            t.set_entries(
                vec![TranscriptEntry {
                    id: "same-message-id".into(),
                    content: TranscriptContent::Assistant(
                        "A paragraph with `code` and a retained source identity.".into(),
                    ),
                }],
                cx,
            )
            .unwrap();
            t
        })
    });
    let host = app.new_entity(|_| SwitchHost {
        transcripts: transcripts.clone(),
        active: 0,
    });
    let mut root = app.open_window(|w, cx| Root::new(host.clone(), w, cx).flex().flex_col());
    root.simulate_resize(size(px(760.), px(600.)));
    root.update(|_, w, cx| {
        w.activate_window();
        w.bounds_changed(cx);
    });
    settle(&mut app, &mut root);
    let control = root.update(|_, w, _| w.find("unrelated-switch-control").bounds().center());
    let text = root.read(|_, cx| {
        let t = transcripts[0].read(cx);
        let View::Prose(text, _) = &t.cache[&t.rows[0].key].view else {
            panic!()
        };
        text.clone()
    });
    let rows = shaped_rows(&mut root, &text, 0..10);
    root.simulate_mouse_down(rows[0].center(), MouseButton::Left);
    held_move(&mut root, point(rows[0].right(), rows[0].center().y));
    let before = root.read(|_, cx| transcripts[0].read(cx).selection.stored_range());
    assert!(
        before.is_some(),
        "the negative control starts with an admitted text drag"
    );
    root.simulate_mouse_up(control, MouseButton::Left);
    held_move(&mut root, point(rows[0].left(), rows[0].center().y));
    root.read(|_, cx| {
        let t = transcripts[0].read(cx);
        assert!(
            !t.selection.is_selecting(),
            "capture must finish even a stopped release"
        );
        assert_eq!(
            t.selection.stored_range(),
            before,
            "later held movement cannot mutate a finished selection"
        );
    });
}

struct MathObject;
impl gpui_kit::base::MarkdownPlugin for MathObject {
    fn name(&self) -> &str {
        "selection-math"
    }
    fn parse(
        &self,
        node: &gpui_kit::base::markdown_ast::Node,
        _: &gpui_kit::base::MarkdownParseContext<'_>,
    ) -> Option<gpui_kit::base::MarkdownNode> {
        let gpui_kit::base::markdown_ast::Node::InlineMath(math) = node else {
            return None;
        };
        Some(
            gpui_kit::base::MarkdownNode::new("selection-math", ())
                .text(math.value.clone())
                .markdown(format!("${}$", math.value)),
        )
    }
    fn render(
        &self,
        node: &gpui_kit::base::MarkdownNode,
        _: &mut Window,
        _: &mut gpui_kit::App,
    ) -> impl IntoElement {
        div()
            .id("selection-math-object")
            .test_support()
            .w(px(28.))
            .h(px(22.))
            .child(node.as_text().to_owned())
    }
}
struct ObjectParagraphHost {
    text: Entity<TextViewState>,
    extensions: gpui_kit::base::MarkdownExtensions,
    format: SelectionFormat,
}
impl Render for ObjectParagraphHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            gpui_kit::base::TextView::new(&self.text)
                .markdown_extensions(self.extensions.clone())
                .selection_format(self.format),
        )
    }
}

#[test]
fn boundary_inline_objects_preserve_neighbor_text_and_object_multi_click_copy() {
    use gpui_kit::base::MarkdownExtensions;
    // This is an explicitly math-enabled Base fixture, not Tau's math-disabled transcript.
    for dark in [false, true] {
        for paragraph in ["Result: $x$", "$x$ Result:"] {
            for format in [SelectionFormat::Plain, SelectionFormat::Source] {
                let source = format!("{paragraph}\n\nAdjacent paragraph.");
                let expected = match format {
                    SelectionFormat::Source => paragraph.to_owned(),
                    _ => paragraph.replace('$', ""),
                };
                let mut app = TestApp::new();
                app.update(|cx| crate::components::init(cx, dark));
                let extensions = MarkdownExtensions::default().math(true).plugin(MathObject);
                let text = app.new_entity(|cx| {
                    TextViewState::markdown_with_extensions(&source, extensions.clone(), cx)
                });
                let host = app.new_entity(|_| ObjectParagraphHost {
                    text: text.clone(),
                    extensions,
                    format,
                });
                let mut root =
                    app.open_window(|w, cx| Root::new(host.clone(), w, cx).flex().flex_col());
                root.simulate_resize(size(px(760.), px(400.)));
                root.update(|_, w, cx| {
                    w.activate_window();
                    w.bounds_changed(cx);
                });
                settle(&mut app, &mut root);
                let rendered = root.read(|_, cx| text.read(cx).rendered_text().as_str().to_owned());
                let at = rendered.find("Result").unwrap();
                let rows = shaped_rows(&mut root, &text, at..at + 6);
                click_three(&mut root, rows[0].center());
                root.read(|_, cx| {
                    // Plain local row selection retains the block serializer's newline;
                    // the clipboard below must contain exactly the paragraph.
                    assert_eq!(
                        text.read(cx).selected_text_with(cx),
                        if matches!(format, SelectionFormat::Plain) {
                            format!("{expected}\n")
                        } else {
                            expected.clone()
                        },
                        "neighbor text must include boundary object: {paragraph}"
                    );
                    let mapped = text.read(cx).selected_source_range_with(cx).unwrap();
                    assert_eq!(&source[mapped], paragraph);
                });
                root.simulate_keystroke("cmd-c");
                root.read(|_, cx| {
                    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected)
                });
                for count in [2, 3] {
                    let position =
                        root.update(|_, w, _| w.find("selection-math-object").bounds().center());
                    root.simulate_mouse_move(position);
                    root.simulate_event(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: count,
                        modifiers: Default::default(),
                        first_mouse: false,
                    });
                    root.simulate_event(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: count,
                        modifiers: Default::default(),
                    });
                    root.draw();
                    let object_expected = if count == 3 {
                        expected.as_str()
                    } else if matches!(format, SelectionFormat::Source) {
                        "$x$"
                    } else {
                        "x"
                    };
                    root.read(|_, cx| {
                        assert_eq!(
                            text.read(cx).selected_text_with(cx),
                            if count == 3 && matches!(format, SelectionFormat::Plain) {
                                format!("{object_expected}\n")
                            } else {
                                object_expected.to_owned()
                            },
                            "object target count={count}"
                        );
                        let mapped = text.read(cx).selected_source_range_with(cx).unwrap();
                        assert_eq!(&source[mapped], if count == 3 { paragraph } else { "$x$" });
                    });
                    root.simulate_keystroke("cmd-c");
                    root.read(|_, cx| {
                        assert_eq!(
                            cx.read_from_clipboard().unwrap().text().unwrap(),
                            object_expected
                        )
                    });
                }
            }
        }
    }
}
