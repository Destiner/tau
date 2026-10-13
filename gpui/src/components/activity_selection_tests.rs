use super::*;
use gpui_kit::{
    Focusable, MouseButton, TestApp, TestAppWindow, base::Root, point, size, test::TestWindowExt,
};

struct Host {
    row: Entity<ActivityRow>,
    outer: ScrollHandle,
    shown: bool,
}
impl Render for Host {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("activity-outer")
            .w(window.viewport_size().width)
            .h(window.viewport_size().height)
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.outer)
            .when(self.shown, |host| {
                host.child(div().flex_shrink_0().child(self.row.clone()))
            })
            .child(div().flex_shrink_0().h(px(1800.)))
    }
}
fn fixture(
    content: ActivityContent,
    dark: bool,
) -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Host>,
    Entity<ActivityRow>,
) {
    let mut app = TestApp::new();
    app.update(|cx| crate::components::init(cx, dark));
    let row = app.new_entity(|cx| ActivityRow::new("exact", content, dark, cx));
    let host = app.new_entity(|_| Host {
        row: row.clone(),
        outer: ScrollHandle::new(),
        shown: true,
    });
    let mut window = app.open_window(|window, cx| Root::new(host.clone(), window, cx));
    window.simulate_resize(size(px(900.), px(700.)));
    window.update(|_, window, cx| {
        window.activate_window();
        window.bounds_changed(cx);
    });
    window.draw();
    let header = window.update(|_, window, _| window.find("exact").bounds());
    window.simulate_click(header.center(), MouseButton::Left);
    app.run_until_parked();
    window.draw();
    (app, window, host, row)
}
fn drag_all(window: &mut TestAppWindow<Root>, id: &str) {
    let bounds = window.update(|_, window, _| window.find(id.to_owned()).bounds());
    let start = point(bounds.left() + px(0.5), bounds.top() + px(3.));
    let end = point(bounds.right() - px(1.), bounds.bottom() - px(1.));
    window.simulate_mouse_down(start, MouseButton::Left);
    window.simulate_event(gpui_kit::MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    window.simulate_mouse_up(end, MouseButton::Left);
    window.draw();
}
fn copy(window: &mut TestAppWindow<Root>) -> String {
    window.simulate_keystroke("cmd-c");
    window.draw();
    window.read(|_, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default()
    })
}
fn tool(arguments: &str, result: &str, failed: bool) -> ActivityContent {
    ActivityContent::Tool {
        name: "bash".into(),
        argument: "check".into(),
        arguments: arguments.into(),
        result: result.into(),
        running: false,
        failed,
    }
}

#[test]
fn root_pointer_copy_preserves_spaces_cr_unicode_and_terminal_newline_for_each_raw_field() {
    for dark in [false, true] {
        for failed in [false, true] {
            let arguments = "  python <<'PY'\r\nprint('λ  x')  \nPY\n";
            let result = "  λ  result\r\nnext  line  \n\n";
            let (_app, mut window, _, _) = fixture(tool(arguments, result, failed), dark);
            for (id, expected) in [
                ("Arguments-body", arguments),
                (if failed { "Error-body" } else { "Result-body" }, result),
            ] {
                drag_all(&mut window, id);
                assert_eq!(
                    copy(&mut window),
                    expected,
                    "{id}, dark={dark}, failed={failed}"
                );
            }
            window.update(|_, w, cx| w.blur(cx));
            _app.run_until_parked();
        }
    }
}

fn selected_ink_contrast(
    ink: gpui_kit::Rgba,
    selection: gpui_kit::Rgba,
    panel: gpui_kit::Rgba,
) -> f32 {
    let luminance = |channels: [f32; 3]| {
        channels
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(v, weight)| {
                weight
                    * if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum::<f32>()
    };
    let composite = |s, p| s * selection.a + p * (1. - selection.a);
    let foreground = luminance([ink.r, ink.g, ink.b]);
    let background = luminance([
        composite(selection.r, panel.r),
        composite(selection.g, panel.g),
        composite(selection.b, panel.b),
    ]);
    (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
}

#[test]
fn readonly_raw_selection_paints_browser_blue_in_both_themes_without_changing_source() {
    let original = "  λ  result\r\nnext  line  \n\n";
    let (_app, mut window, _, row) = fixture(tool("", original, false), false);
    drag_all(&mut window, "Result-body");
    let source = window.read(|_, cx| row.read(cx).source_state());
    let editor = window.read(|_, cx| source.read(cx).raw[1].as_ref().unwrap().clone());
    let range = window.read(|_, cx| editor.read(cx).selected_range());
    for (dark, expected_contrast) in [(false, 3.7022473), (true, 2.6118925), (false, 3.7022473)] {
        window.update(|_, w, cx| row.update(cx, |row, cx| row.set_theme(dark, w, cx)));
        window.draw();
        let body = window.update(|_, w, _| w.find("Result-body").bounds());
        window.update(|_, w, _| {
            let selection = support::input_style(dark).selection;
            let clipped_body = body.scale(w.scale_factor());
            let _painted = w
                .painted_paths()
                .into_iter()
                .find(|path| {
                    path.color == selection.into() && clipped_body.contains(&path.bounds.center())
                })
                .expect("selected raw glyph rows must paint the actual blue selection");
            let t = support::tokens(dark);
            assert!(
                !w.painted_quads().iter().any(|quad| {
                    clipped_body.contains(&quad.bounds.center())
                        && quad.background == rgb(t.accent).into()
                }),
                "readonly output must not paint an insertion caret"
            );
            let contrast =
                selected_ink_contrast(rgb(t.muted), selection.into(), rgb(t.panel_raised));
            assert!(
                (contrast - expected_contrast).abs() < 0.0001,
                "contrast={contrast}, dark={dark}"
            );
        });
        window.simulate_input("forbidden user edit");
        window.update(|_, w, cx| {
            let raw = editor.read(cx);
            assert!(!raw.is_editable());
            assert!(
                raw.focus_handle(cx).is_focused(w),
                "readonly remains focusable, unlike disabled"
            );
            assert_eq!(raw.value().as_ref(), original);
            assert_eq!(raw.selected_range(), range);
            assert_eq!(source.read(cx).raw[1].as_ref(), Some(&editor));
        });
        assert_eq!(copy(&mut window), original);
    }
    window.update(|_, w, cx| w.blur(cx));
    _app.run_until_parked();
}

#[test]
fn retained_raw_source_survives_stream_update_disclosure_and_view_replacement() {
    let original = "first  line\r\nlast\n";
    let (_app, mut window, host, row) = fixture(tool("", original, false), false);
    drag_all(&mut window, "Result-body");
    assert_eq!(copy(&mut window), original);
    let source = window.read(|_, cx| row.read(cx).source_state());
    window.update(|_, _, cx| {
        row.update(cx, |row, cx| {
            row.set_content(tool("", &format!("{original}new output\n"), false), cx)
        })
    });
    window.draw();
    assert_eq!(
        copy(&mut window),
        original,
        "selected byte range must not grow with stream"
    );
    let selected = window.read(|_, cx| {
        source.read(cx).raw[1]
            .as_ref()
            .unwrap()
            .read(cx)
            .selected_range()
    });
    window.update(|_, _, cx| row.update(cx, |row, cx| row.toggle(cx)));
    window.draw();
    window.update(|_, _, cx| row.update(cx, |row, cx| row.toggle(cx)));
    window.draw();
    assert_eq!(copy(&mut window), original);
    // The source row, not the evictable ActivityRow, retains the text control.
    window.update(|_, _, cx| {
        let replacement = cx.new(|cx| {
            let mut replacement = ActivityRow::new(
                "exact",
                tool("", &format!("{original}new output\n"), false),
                false,
                cx,
            );
            replacement.set_source_state(source.clone(), cx);
            replacement.toggle(cx);
            replacement
        });
        host.update(cx, |host, cx| {
            host.row = replacement;
            cx.notify();
        });
    });
    window.draw();
    window.read(|_, cx| {
        let retained = source.read(cx).raw[1].as_ref().unwrap().read(cx);
        assert_eq!(retained.selected_range(), selected);
        assert_eq!(retained.selected_value().as_ref(), original);
    });
    assert_eq!(copy(&mut window), original);
    window.update(|_, w, cx| w.blur(cx));
    _app.run_until_parked();
}

#[test]
fn shared_prose_has_actual_viewport_cap_conditional_wheel_and_source_owned_offset() {
    for skill in [false, true] {
        for overflowing in [false, true] {
            let text = if overflowing {
                "Paragraph with **reasoning**.\n\n".repeat(240)
            } else {
                "Short reasoning.".into()
            };
            let content = if skill {
                ActivityContent::Skill {
                    name: "review".into(),
                    prompt: "Prompt".into(),
                    text,
                }
            } else {
                ActivityContent::Thinking { text }
            };
            let (app, mut window, host, row) = fixture(content.clone(), false);
            app.run_until_parked();
            window.draw();
            let index = usize::from(skill);
            let id = format!("exact-prose-{index}");
            let source = window.read(|_, cx| row.read(cx).source_state());
            let inner = window.read(|_, cx| source.read(cx).prose_scroll[index].clone());
            let outer = window.read(|_, cx| host.read(cx).outer.clone());
            for (height, cap) in [(1000., 520.), (480., 312.), (700., 455.)] {
                window.simulate_resize(size(px(900.), px(height)));
                window.update(|_, window, cx| window.bounds_changed(cx));
                window.draw();
                let bounds = window.update(|_, window, _| window.find(id.clone()).bounds());
                if overflowing {
                    assert_eq!(bounds.size.height, px(cap));
                    window.simulate_scroll(bounds.center(), point(px(0.), px(-45.)));
                    window.draw();
                    assert!(inner.offset().y < px(0.));
                    assert_eq!(outer.offset().y, px(0.));
                    for delta in [-100_000., -30., 100_000., 30.] {
                        window.simulate_scroll(bounds.center(), point(px(0.), px(delta)));
                        window.draw();
                        assert_eq!(outer.offset().y, px(0.), "overflow must own both edges");
                    }
                } else {
                    assert!(bounds.size.height < px(cap));
                    window.simulate_scroll(bounds.center(), point(px(0.), px(-35.)));
                    window.draw();
                    assert_eq!(inner.offset().y, px(0.));
                    assert_eq!(outer.offset().y, px(-35.));
                    outer.set_offset(point(px(0.), px(0.)));
                    window.draw();
                }
            }
            if overflowing {
                let bounds = window.update(|_, window, _| window.find(id.clone()).bounds());
                window.simulate_scroll(bounds.center(), point(px(0.), px(-50.)));
                window.draw();
                let offset = inner.offset();
                window.update(|_, _, cx| {
                    let replacement = cx.new(|cx| {
                        let mut row = ActivityRow::new("exact", content, false, cx);
                        row.set_source_state(source.clone(), cx);
                        row.toggle(cx);
                        row
                    });
                    host.update(cx, |host, cx| {
                        host.row = replacement;
                        cx.notify();
                    });
                });
                window.draw();
                assert_eq!(inner.offset(), offset);
            }
        }
    }
}

#[test]
fn prose_pointer_selection_survives_append_and_an_unpainted_source_owned_remount() {
    let original = "First reasoning.";
    let (app, mut window, host, row) = fixture(
        ActivityContent::Thinking {
            text: original.into(),
        },
        false,
    );
    app.run_until_parked();
    window.draw();
    drag_all(&mut window, "exact-prose-0");
    assert_eq!(copy(&mut window), original);
    let source = window.read(|_, cx| row.read(cx).source_state());
    window.update(|_, _, cx| {
        row.update(cx, |row, cx| {
            row.set_content(
                ActivityContent::Thinking {
                    text: format!("{original}\n\nNext reasoning."),
                },
                cx,
            )
        })
    });
    app.run_until_parked();
    window.draw();
    assert_eq!(copy(&mut window), original);
    window.update(|_, _, cx| {
        host.update(cx, |host, cx| {
            host.shown = false;
            cx.notify();
        })
    });
    window.draw();
    window.update(|_, _, cx| {
        let replacement = cx.new(|cx| {
            let mut row = ActivityRow::new(
                "exact",
                ActivityContent::Thinking {
                    text: format!("{original}\n\nNext reasoning."),
                },
                false,
                cx,
            );
            row.set_source_state(source.clone(), cx);
            row.toggle(cx);
            row
        });
        host.update(cx, |host, cx| {
            host.row = replacement;
            host.shown = true;
            cx.notify();
        });
    });
    app.run_until_parked();
    window.draw();
    assert_eq!(
        copy(&mut window),
        original,
        "logical domain survives an unpainted frame"
    );
}

#[test]
fn thinking_and_skill_admit_the_shared_diagram_renderer() {
    for skill in [false, true] {
        let text = "```mermaid\ngraph TD\nA --> B\n```".into();
        let content = if skill {
            ActivityContent::Skill {
                name: "review".into(),
                prompt: "".into(),
                text,
            }
        } else {
            ActivityContent::Thinking { text }
        };
        let (app, mut window, _, row) = fixture(content, false);
        for _ in 0..3 {
            app.run_until_parked();
            window.draw();
        }
        window.read(|_, cx| {
            let source = row.read(cx).source_state();
            assert_eq!(
                source.read(cx).presentations[usize::from(skill)]
                    .read(cx)
                    .diagram_blocks()
                    .len(),
                1
            );
        });
    }
}

#[test]
fn thinking_and_skill_mount_shared_code_copy_affordance_not_a_bespoke_text_view() {
    for skill in [false, true] {
        let text = "```rs\nlet value = 1;\n```".into();
        let content = if skill {
            ActivityContent::Skill {
                name: "review".into(),
                prompt: "".into(),
                text,
            }
        } else {
            ActivityContent::Thinking { text }
        };
        let (app, mut window, _, row) = fixture(content, false);
        app.run_until_parked();
        window.draw();
        window.read(|_, cx| {
            let state = row.read(cx).source_state();
            let source = state.read(cx);
            assert_eq!(
                source.presentations[usize::from(skill)]
                    .read(cx)
                    .rendered_code_blocks(),
                1
            );
        });
    }
}
