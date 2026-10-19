//! Real Root coverage for the persistent editor tail and typed source routing.
use super::*;
use crate::components::dialogs::{DialogKind, DialogProps};
use gpui_kit::{
    EntityInputHandler, ListOffset, MouseButton, Pixels, Point, TestApp, TestAppWindow, base::Root,
    point, test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc};

struct Fixture {
    window: TestAppWindow<Root>,
    transcript: Entity<Transcript>,
    prompt: Entity<NativeDialog>,
    // Entity handles must be released before the app's leak detector runs.
    app: TestApp,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.window.update(|_, w, cx| w.blur(cx));
            self.app.run_until_parked();
        }
    }
}
impl Fixture {
    fn new(count: usize, dark: bool) -> Self {
        let mut app = TestApp::new();
        app.update(|cx| crate::components::init(cx, dark));
        let transcript = app.new_entity(|cx| {
            let mut t = Transcript::new("prompt-transcript", dark, cx);
            t.set_entries(
                (0..count)
                    .map(|i| TranscriptEntry {
                        id: format!("message-{i}"),
                        content: TranscriptContent::Assistant(format!(
                            "Earlier message {i}.\n\nMore history."
                        )),
                    })
                    .collect(),
                cx,
            )
            .unwrap();
            t
        });
        let mut prompt = None;
        let mounted = transcript.clone();
        let mut window = app.open_window(|w, cx| {
            let dialog = cx.new(|cx| {
                NativeDialog::new_editor(
                    DialogProps {
                        kind: DialogKind::ExtensionInput,
                        title: "Question **with emphasis**".into(),
                        description: "Keep this answer while reading history.".into(),
                        placeholder: "Reply".into(),
                        options: vec![],
                    },
                    dark,
                    w,
                    cx,
                )
                .embedded()
                .keyboard_submission()
            });
            prompt = Some(dialog.clone());
            mounted.update(cx, |t, cx| t.set_extension_prompt(Some(dialog), cx));
            Root::new(mounted.clone(), w, cx).flex().flex_col()
        });
        window.simulate_resize(size(px(1000.), px(700.)));
        window.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
        });
        let mut fixture = Self {
            app,
            window,
            transcript,
            prompt: prompt.unwrap(),
        };
        fixture.settle();
        fixture
    }
    fn settle(&mut self) {
        for _ in 0..5 {
            self.app.run_until_parked();
            self.window.draw();
        }
    }
    fn value(&self) -> String {
        self.window.read(|_, cx| self.prompt.read(cx).value(cx))
    }
    fn history(&mut self) {
        self.window.update(|_, _, cx| {
            self.transcript.update(cx, |t, cx| {
                t.list.scroll_to(ListOffset {
                    item_ix: 2,
                    offset_in_item: px(7.),
                });
                cx.notify();
            })
        });
        self.settle();
    }
}

#[test]
fn empty_transcript_prompt_is_measured_ready_and_in_the_exact_column_without_copy_sources() {
    for dark in [false, true] {
        let mut f = Fixture::new(0, dark);
        let (tail, extension) = f.window.update(|_, w, _| {
            (
                w.find("transcript-extension-tail").bounds(),
                w.find("extension").bounds(),
            )
        });
        assert_eq!(tail.size.width, px(1000.));
        // 880px centered column, 50px gutter, then Vue's 8/20px prompt margins.
        assert_eq!(extension.left(), px(60. + 50. + 8.));
        assert_eq!(extension.right(), px(940. - 50. - 20.));
        f.window.read(|_, cx| {
            let t = f.transcript.read(cx);
            assert!(t.rows.is_empty());
            assert!(t.cache.is_empty());
            assert!(t.selection.stored_range().is_none());
            assert_eq!(t.list.item_count(), 1);
            assert_eq!(t.prompt_height, tail.size.height);
            assert!(t.is_following_tail());
        });
        let input = f
            .window
            .update(|_, w, _| w.find("multiline").bounds().center());
        f.window.simulate_click(input, MouseButton::Left);
        f.window.simulate_input("ready answer\nsecond line");
        assert_eq!(f.value(), "ready answer\nsecond line");
    }
}

#[test]
fn retained_tail_survives_real_history_scroll_cache_eviction_stream_resize_and_local_error() {
    for dark in [false, true] {
        let mut f = Fixture::new(240, dark);
        let editor = f.window.read(|_, cx| f.prompt.read(cx).multiline_editor());
        let at = f
            .window
            .update(|_, w, _| w.find("multiline").bounds().center());
        f.window.simulate_click(at, MouseButton::Left);
        f.window.simulate_input("  sacred draft\nλ\n");
        f.history();
        let before = f
            .window
            .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top());
        f.window.update(|_, w, cx| {
            f.transcript.update(cx, |t, cx| {
                let owner = cx.entity().downgrade();
                for ix in 0..t.rows.len() {
                    t.mount(ix, owner.clone(), cx);
                }
                assert!(t.cache.len() <= CACHE_LIMIT);
                t.append_text("message-239", "\n\nStreaming suffix.", cx);
            });
            f.prompt.update(cx, |d, cx| {
                d.set_busy(true, cx);
                d.set_error("Could not send the response. Try again.", cx);
                d.set_theme(!dark, w, cx);
            });
        });
        f.settle();
        f.window.read(|_, cx| {
            let t = f.transcript.read(cx);
            let after = t.list.logical_scroll_top();
            assert_eq!(
                (after.item_ix, after.offset_in_item),
                (before.item_ix, before.offset_in_item)
            );
            assert!(!t.is_following_tail());
            assert_eq!(t.extension_prompt.as_ref(), Some(&f.prompt));
            assert_eq!(f.prompt.read(cx).multiline_editor(), editor);
        });
        assert_eq!(f.value(), "  sacred draft\nλ\n");
        f.window.simulate_resize(size(px(720.), px(480.)));
        f.window.update(|_, w, cx| w.bounds_changed(cx));
        f.settle();
        f.window.read(|_, cx| {
            assert_eq!(
                f.transcript.read(cx).list.logical_scroll_top().item_ix,
                before.item_ix
            );
            assert!(!f.transcript.read(cx).is_following_tail());
        });
        f.window
            .update(|_, _, cx| f.transcript.update(cx, |t, cx| t.scroll_to_end(cx)));
        f.settle();
        let bounds = f.window.update(|_, w, _| w.find("extension").bounds());
        assert!(bounds.bottom() <= px(480.));
        assert_eq!(f.value(), "  sacred draft\nλ\n");
        f.window
            .read(|_, cx| assert_eq!(f.prompt.read(cx).multiline_editor(), editor));
    }
}

#[test]
fn marked_text_stays_on_the_same_editor_while_tail_is_clipped_and_prompt_replaced() {
    let mut f = Fixture::new(240, false);
    let editor = f.window.read(|_, cx| f.prompt.read(cx).multiline_editor());
    let at = f
        .window
        .update(|_, w, _| w.find("multiline").bounds().center());
    f.window.simulate_click(at, MouseButton::Left);
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(None, "稍后", Some(2..2), w, cx);
        })
    });
    f.history();
    let protected = f
        .window
        .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top());
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(None, "稍后", Some(2..2), w, cx);
        });
    });
    f.settle();
    f.window.read(|_, cx| {
        let after = f.transcript.read(cx).list.logical_scroll_top();
        assert_eq!(
            (after.item_ix, after.offset_in_item),
            (protected.item_ix, protected.offset_in_item)
        );
        assert!(!f.transcript.read(cx).is_following_tail());
    });
    f.window.update(|_, _, cx| {
        f.transcript.update(cx, |t, cx| {
            t.set_extension_prompt(Some(f.prompt.clone()), cx);
            t.append_entry(
                TranscriptEntry {
                    id: "live".into(),
                    content: TranscriptContent::Assistant("Stream".into()),
                },
                cx,
            )
            .unwrap();
        })
    });
    f.settle();
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            assert!(e.marked_text_range(w, cx).is_some());
        })
    });
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            e.replace_text_in_range(Some(0..2), "稍后", w, cx);
        })
    });
    assert_eq!(f.value(), "稍后");
    let before = f
        .window
        .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top());
    let replacement = f.window.update(|_, w, cx| {
        cx.new(|cx| {
            NativeDialog::new_editor(
                DialogProps {
                    kind: DialogKind::ExtensionInput,
                    title: "Replacement".into(),
                    description: "".into(),
                    placeholder: "".into(),
                    options: vec![],
                },
                false,
                w,
                cx,
            )
            .embedded()
        })
    });
    f.window.update(|_, _, cx| {
        f.transcript.update(cx, |t, cx| {
            t.set_extension_prompt(Some(replacement.clone()), cx);
            // An old measurement deferred across request replacement cannot alter the new spacer.
            t.measure_prompt(&f.prompt, px(999.), cx);
            assert_eq!(t.prompt_height, px(0.));
            let offset = t.list.logical_scroll_top();
            let mut stale_bounds = t.list.viewport_bounds();
            stale_bounds.origin.y -= px(999.);
            t.reveal_prompt_option(f.prompt.entity_id(), stale_bounds, cx);
            let after = t.list.logical_scroll_top();
            assert_eq!(
                (after.item_ix, after.offset_in_item),
                (offset.item_ix, offset.offset_in_item)
            );
        })
    });
    f.settle();
    assert_eq!(f.value(), "稍后", "retired request still owns its draft");
    f.window.read(|_, cx| {
        let t = f.transcript.read(cx);
        assert_eq!(t.extension_prompt.as_ref(), Some(&replacement));
        let after = t.list.logical_scroll_top();
        assert_eq!(
            (after.item_ix, after.offset_in_item),
            (before.item_ix, before.offset_in_item)
        );
        assert!(t.prompt_height < px(999.));
    });
}

fn shaped_link_point(source: &Entity<TextViewState>, cx: &App) -> Point<Pixels> {
    let source = source.read(cx);
    let bounds = source.bounds();
    for y in 0..f32::from(bounds.size.height).ceil() as usize {
        for x in 0..f32::from(bounds.size.width).ceil() as usize {
            let at = bounds.origin + point(px(x as f32), px(y as f32));
            if source.rendered_offset_at(at) == Some(1) {
                return at;
            }
        }
    }
    panic!("link source must expose shaped hit geometry")
}

#[test]
fn typed_markdown_activation_forwards_written_and_decode_once_targets_with_source_fences() {
    for (text, expected) in [
        (
            "src/a.rs:42",
            markdown::LinkTarget::WrittenPath("src/a.rs".into()),
        ),
        (
            "[file](docs/a%2520b.md)",
            markdown::LinkTarget::DocumentPath("docs/a%20b.md".into()),
        ),
        (
            "[web](https://example.com)",
            markdown::LinkTarget::Web("https://example.com".into()),
        ),
    ] {
        let mut f = Fixture::new(0, false);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let sink = requests.clone();
        let _subscription = f.window.update(|_, _, cx| {
            f.transcript.update(cx, |t, cx| {
                t.set_extension_prompt(None, cx);
                t.append_entry(
                    TranscriptEntry {
                        id: "link-source".into(),
                        content: TranscriptContent::Assistant(text.into()),
                    },
                    cx,
                )
                .unwrap();
                cx.subscribe(&cx.entity(), move |_, _, event: &TranscriptEvent, _| {
                    sink.borrow_mut().push(event.clone())
                })
            })
        });
        f.settle();
        let at = f.window.read(|_, cx| {
            let View::Prose(source, _) = &f.transcript.read(cx).cache[&0].view else {
                panic!("mounted prose source")
            };
            shaped_link_point(source, cx)
        });
        f.window.simulate_click(at, MouseButton::Left);
        f.settle();
        let events = requests.borrow();
        let TranscriptEvent::LinkRequested {
            key,
            target,
            origin,
            ..
        } = events.last().expect("real native link must forward")
        else {
            panic!()
        };
        assert_eq!(target, &expected);
        assert!(origin.is_some());
        f.window
            .read(|_, cx| assert_eq!(f.transcript.read(cx).source_id(*key), Some("link-source")));
        drop(events);
        f.window
            .update(|_, _, cx| f.transcript.update(cx, |t, cx| t.reset(cx)));
        f.settle();
        assert!(
            f.window
                .read(|_, cx| f.transcript.read(cx).source_id(0).is_none())
        );
    }
}

#[test]
fn domain_link_press_survives_repaint_but_drag_never_activates() {
    let mut f = Fixture::new(0, false);
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = requests.clone();
    let _subscription = f.window.update(|_, _, cx| {
        f.transcript.update(cx, |t, cx| {
            t.set_extension_prompt(None, cx);
            t.append_entry(
                TranscriptEntry {
                    id: "domain-link".into(),
                    content: TranscriptContent::Assistant("src/a.rs trailing text".into()),
                },
                cx,
            )
            .unwrap();
            cx.subscribe(&cx.entity(), move |_, _, event: &TranscriptEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
        })
    });
    f.settle();
    let at = f.window.read(|_, cx| {
        let View::Prose(source, _) = &f.transcript.read(cx).cache[&0].view else {
            panic!("mounted prose source")
        };
        shaped_link_point(source, cx)
    });
    f.window.simulate_mouse_down(at, MouseButton::Left);
    f.settle();
    f.window.simulate_mouse_up(at, MouseButton::Left);
    f.settle();
    assert!(
        matches!(requests.borrow().as_slice(), [TranscriptEvent::LinkRequested { target: markdown::LinkTarget::WrittenPath(path), .. }] if path == "src/a.rs")
    );
    requests.borrow_mut().clear();
    f.window.simulate_mouse_down(at, MouseButton::Left);
    f.window.simulate_mouse_move(at + point(px(24.), px(0.)));
    f.settle();
    f.window
        .simulate_mouse_up(at + point(px(24.), px(0.)), MouseButton::Left);
    f.settle();
    assert!(
        requests.borrow().is_empty(),
        "dragging a path must not activate it"
    );
}

#[test]
fn typed_activity_events_reject_retired_mounts_and_preserve_destination_variants() {
    let mut f = Fixture::new(0, false);
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = requests.clone();
    let _subscription = f.window.update(|_, _, cx| {
        f.transcript.update(cx, |t, cx| {
            t.set_extension_prompt(None, cx);
            t.append_entry(
                TranscriptEntry {
                    id: "thinking".into(),
                    content: TranscriptContent::Activity(ActivityContent::Thinking {
                        text: "src/a.rs".into(),
                    }),
                },
                cx,
            )
            .unwrap();
            cx.subscribe(&cx.entity(), move |_, _, event: &TranscriptEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
        })
    });
    f.settle();
    let activity = f
        .window
        .read(|_, cx| match &f.transcript.read(cx).cache[&0].view {
            View::Activity(activity) => activity.clone(),
            _ => panic!(),
        });
    f.window
        .update(|_, _, cx| activity.update(cx, |a, cx| a.toggle(cx)));
    f.settle();
    let at = f
        .window
        .read(|_, cx| shaped_link_point(&activity.read(cx).prose_source_for_test(0, cx), cx));
    f.window.simulate_click(at, MouseButton::Left);
    f.app.run_until_parked();
    assert!(
        matches!(requests.borrow().last(), Some(TranscriptEvent::LinkRequested { key: 0, target: markdown::LinkTarget::WrittenPath(path), .. }) if path == "src/a.rs")
    );
    requests.borrow_mut().clear();
    f.window.update(|_, _, cx| {
        f.transcript.update(cx, |t, cx| {
            t.update_content(
                "thinking",
                TranscriptContent::Assistant("replacement".into()),
                cx,
            );
        })
    });
    f.window.update(|_, _, cx| {
        activity.update(cx, |_, cx| {
            cx.emit(ActivityEvent::LinkRequested {
                target: markdown::LinkTarget::DocumentPath("retired.md".into()),
                origin: None,
                anchor: gpui_kit::point(px(0.), px(0.)),
            })
        })
    });
    f.settle();
    assert!(requests.borrow().is_empty());
}

#[test]
fn wheel_over_persistent_prompt_uses_the_existing_list_reader_without_losing_the_editor() {
    let mut f = Fixture::new(240, false);
    let editor = f.window.read(|_, cx| f.prompt.read(cx).multiline_editor());
    let input = f
        .window
        .update(|_, w, _| w.find("multiline").bounds().center());
    f.window.simulate_click(input, MouseButton::Left);
    f.window.simulate_input("retained answer");
    f.settle();
    let title = f
        .window
        .update(|_, w, _| w.find("extension-title").bounds().center());
    let before = f
        .window
        .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top());
    f.window.simulate_scroll(title, point(px(0.), px(180.)));
    f.settle();
    f.window.read(|_, cx| {
        let t = f.transcript.read(cx);
        let after = t.list.logical_scroll_top();
        assert_ne!(
            (before.item_ix, before.offset_in_item),
            (after.item_ix, after.offset_in_item)
        );
        assert!(!t.is_following_tail());
        assert_eq!(t.extension_prompt.as_ref(), Some(&f.prompt));
        assert_eq!(f.prompt.read(cx).multiline_editor(), editor);
    });
    assert_eq!(f.value(), "retained answer");
}

#[test]
fn overflowing_embedded_prompt_uses_parent_reader_without_losing_marked_editor() {
    let mut f = Fixture::new(240, false);
    let prompt = f.window.update(|_, w, cx| {
        cx.new(|cx| {
            NativeDialog::new_editor(
                DialogProps {
                    kind: DialogKind::ExtensionInput,
                    title: "Long question".into(),
                    description: "Question paragraph.\n\n".repeat(240),
                    placeholder: "Reply".into(),
                    options: vec![],
                },
                false,
                w,
                cx,
            )
            .embedded()
        })
    });
    f.window.update(|_, _, cx| {
        f.transcript
            .update(cx, |t, cx| t.set_extension_prompt(Some(prompt.clone()), cx));
    });
    f.settle();
    let editor = f.window.read(|_, cx| prompt.read(cx).multiline_editor());
    let at = f
        .window
        .update(|_, w, _| w.find("multiline").bounds().center());
    f.window.simulate_click(at, MouseButton::Left);
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            e.replace_and_mark_text_in_range(None, "稍后", Some(2..2), w, cx);
        })
    });
    f.settle();
    let before = f
        .window
        .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top());
    let title_top = f
        .window
        .update(|_, w, _| w.find("extension-title").bounds().top());
    let cancel = f
        .window
        .update(|_, w, _| w.find("cancel").bounds().center());
    f.window.simulate_scroll(cancel, point(px(0.), px(300.)));
    f.settle();
    assert!(
        f.window
            .update(|_, w, _| w.find("extension-title").bounds().top())
            > title_top,
        "title={title_top:?}, cancel={cancel:?}, viewport={:?}, before={before:?}, after={:?}",
        f.window
            .update(|_, w, _| w.find("transcript-extension-tail").bounds()),
        f.window
            .read(|_, cx| f.transcript.read(cx).list.logical_scroll_top())
    );
    f.window.read(|_, cx| {
        let t = f.transcript.read(cx);
        let after = t.list.logical_scroll_top();
        assert_ne!(
            (after.item_ix, after.offset_in_item),
            (before.item_ix, before.offset_in_item),
            "natural embedded content must scroll the retained parent reader",
        );
        assert_eq!(prompt.read(cx).multiline_editor(), editor);
        assert_eq!(prompt.read(cx).value(cx), "稍后");
    });
    f.window.update(|_, w, cx| {
        editor.update(cx, |e, cx| {
            assert!(e.marked_text_range(w, cx).is_some());
        })
    });
}

#[test]
fn retained_prompt_question_routes_typed_generated_links_through_the_shell_hook() {
    let mut f = Fixture::new(0, false);
    let targets = Rc::new(RefCell::new(Vec::new()));
    let sink = targets.clone();
    let linked = f.window.update(|_, w, cx| {
        cx.new(|cx| {
            let mut d = NativeDialog::new_editor(
                DialogProps {
                    kind: DialogKind::ExtensionInput,
                    title: "src/a.rs:42".into(),
                    description: "".into(),
                    placeholder: "".into(),
                    options: vec![],
                },
                false,
                w,
                cx,
            )
            .embedded();
            d.set_target_link_handler(move |target, origin, _, _| {
                sink.borrow_mut().push((target, origin))
            });
            d
        })
    });
    f.window.update(|_, _, cx| {
        f.transcript
            .update(cx, |t, cx| t.set_extension_prompt(Some(linked), cx))
    });
    f.settle();
    let title = f
        .window
        .update(|_, w, _| w.find("extension-title").bounds());
    f.window.simulate_click(
        point(title.left() + px(8.), title.top() + px(7.)),
        MouseButton::Left,
    );
    f.settle();
    assert!(
        matches!(targets.borrow().last(), Some((markdown::LinkTarget::WrittenPath(path), Some(_))) if path == "src/a.rs")
    );
}

#[test]
fn queue_preview_generated_links_emit_typed_requests_not_legacy_strings() {
    use crate::components::queue::{MessageQueue, QueueEvent, QueueProps};
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, false));
    let targets = Rc::new(RefCell::new(Vec::new()));
    let sink = targets.clone();
    let queue = app.new_entity(|cx| {
        MessageQueue::new(
            "typed-queue",
            QueueProps {
                follow_up: vec!["src/a.rs:42".into()],
                ..Default::default()
            },
            false,
            cx,
        )
    });
    let _subscription = app.update(|cx| {
        queue.update(cx, |_, cx| {
            cx.subscribe(&cx.entity(), move |_, _, event: &QueueEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
        })
    });
    let mut window = app.open_window(|w, cx| Root::new(queue.clone(), w, cx).flex().flex_col());
    window.simulate_resize(size(px(900.), px(700.)));
    window.update(|_, w, cx| {
        w.bounds_changed(cx);
        queue.update(cx, |q, cx| q.focus_follow_up(w, cx));
    });
    for _ in 0..5 {
        app.run_until_parked();
        window.draw();
    }
    let preview = window.update(|_, w, _| w.find("preview").bounds());
    window.simulate_click(
        point(preview.left() + px(14.), preview.top() + px(34.)),
        MouseButton::Left,
    );
    app.run_until_parked();
    assert!(
        matches!(targets.borrow().last(), Some(QueueEvent::OpenLink(markdown::LinkTarget::WrittenPath(path))) if path == "src/a.rs")
    );
    assert_eq!(targets.borrow().len(), 1);
}
