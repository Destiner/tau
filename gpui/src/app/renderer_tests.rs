use super::tests::{app, create, project, rpc, shell, started};
use super::*;
use crate::components::renderer_interaction_tests::{held_move, shaped_rows};
use gpui_kit::{MouseButton, TestAppWindow, point, size, test::TestWindowExt};
use serde_json::json;

#[test]
fn production_shell_pi_multisource_gap_drag_keeps_copy_and_blue_for_three_held_frames() {
    for dark in [false, true] {
        let mut app = app();
        let (commands, _requests) = async_channel::bounded(256);
        let shell = app.new_entity(|_| shell(commands));
        let mut root: TestAppWindow<Root> =
            app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(800.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| {
                s.preferences.theme = if dark { "dark" } else { "light" }.into();
                s.dark = dark;
                s.window = Some(w.window_handle());
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.mount(w, cx);
                create(s, "a", "retained draft", w, cx);
                started(s, "a", 8, cx);
                s.ingest(
                    rpc(
                        "a",
                        8,
                        json!({"type":"message_start", "message":{
                    "role":"user", "content":"甲乙 café 👩‍💻"}}),
                    ),
                    cx,
                );
                s.ingest(
                    rpc(
                        "a",
                        8,
                        json!({"type":"message_end", "message":{
                    "role":"user", "content":"甲乙 café 👩‍💻"}}),
                    ),
                    cx,
                );
                s.ingest(
                    rpc(
                        "a",
                        8,
                        json!({"type":"message_start", "message":{
                    "role":"assistant", "timestamp":1, "content":[]}}),
                    ),
                    cx,
                );
                s.ingest(
                    rpc(
                        "a",
                        8,
                        json!({"type":"message_update", "assistantMessageEvent":{
                    "type":"text_delta", "contentIndex":0,
                    "delta":"café λ\n\n- tail\n\n```txt\n  code λ\r\nlast  line\r\n```"}}),
                    ),
                    cx,
                );
                s.sync(w, cx);
            });
        });
        for _ in 0..3 {
            root.draw();
        }
        let transcript = root.read(|_, cx| shell.read(cx).sessions["a"].transcript.clone());
        let texts = root.read(|_, cx| {
            crate::components::transcript::renderer_transcript_tests::cached_prose(
                transcript.read(cx),
            )
        });
        assert_eq!(
            texts.len(),
            2,
            "Pi events must mount two actual source owners"
        );
        let mut lines = Vec::new();
        for (item, (_, text)) in texts.iter().enumerate() {
            let len = root.read(|_, cx| text.read(cx).rendered_text().len());
            lines.extend(
                shaped_rows(&mut root, text, 0..len)
                    .into_iter()
                    .map(|r| (item, r)),
            );
        }
        let pair = lines.windows(2).find(|p| p[0].0 != p[1].0).unwrap();
        assert!(
            pair[1].1.top() > pair[0].1.bottom(),
            "real Shell must lay out a source gap"
        );
        let first = lines[0].1;
        let last = lines.last().unwrap().1;
        let original_editor =
            root.read(|_, cx| shell.read(cx).sessions["a"].composer.read(cx).editor());
        for forward in [true, false] {
            let anchor = if forward {
                point(first.left() + px(0.1), first.center().y)
            } else {
                point(last.right() + px(2.), last.center().y)
            };
            root.simulate_mouse_down(anchor, MouseButton::Left);
            for fraction in if forward {
                [0.125, 0.375, 0.625, 0.875]
            } else {
                [0.875, 0.625, 0.375, 0.125]
            } {
                let head = point(
                    pair[0].1.center().x,
                    pair[0].1.bottom() + (pair[1].1.top() - pair[0].1.bottom()) * fraction,
                );
                held_move(&mut root, head);
                let mut preserved = None;
                for _ in 0..3 {
                    root.draw();
                    root.update(|_, w, cx| {
                        let selected = gpui_kit::base::TextSelection::selected_text(w, cx);
                        assert!(!selected.is_empty(), "source gap must not blank selection");
                        if let Some(ref before) = preserved {
                            assert_eq!(&selected, before);
                        }
                        preserved = Some(selected);
                        let color = crate::components::markdown::text_style(dark, w).selection();
                        let slot = w
                            .find("shell-transcript-slot")
                            .bounds()
                            .scale(w.scale_factor());
                        assert!(
                            w.painted_quads()
                                .iter()
                                .any(|q| q.background == color.into()
                                    && q.bounds.size.height.0 > w.scale_factor()
                                    && slot.contains(&q.bounds.center())),
                            "actual Shell selection blue must survive held frames"
                        );
                    });
                }
                root.simulate_keystroke("cmd-c");
                root.read(|_, cx| {
                    assert_eq!(cx.read_from_clipboard().and_then(|v| v.text()), preserved);
                    assert_eq!(
                        shell.read(cx).sessions["a"].composer.read(cx).editor(),
                        original_editor
                    );
                    assert_eq!(original_editor.read(cx).value().as_ref(), "retained draft");
                });
            }
            root.simulate_mouse_up(anchor, MouseButton::Left);
        }
        root.read(|_, cx| {
            assert_eq!(
                texts[0].1.read(cx).rendered_text().as_str().trim_end(),
                "甲乙 café 👩‍💻"
            );
            assert_eq!(
                texts[1]
                    .1
                    .read(cx)
                    .rendered_text()
                    .as_str()
                    .trim_end_matches('\n'),
                "café λ\ntail\n  code λ\nlast  line"
            );
            let code = texts[1].1.read(cx).rendered_text();
            let at = code.as_str().find("λ\nlast").unwrap();
            let mapped = texts[1]
                .1
                .read(cx)
                .source_range_for_rendered_range(at..at + "λ\nlast".len())
                .unwrap();
            assert_eq!(
                &"café λ\n\n- tail\n\n```txt\n  code λ\r\nlast  line\r\n```"[mapped],
                "λ\r\nlast"
            );
        });
        crate::components::renderer_interaction_tests::assert_crlf_code_geometry(
            &mut root,
            &texts[1].1,
        );
    }
}
