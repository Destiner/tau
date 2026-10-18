use super::*;

// These exercise real Root ancestry and geometry, not glyph coverage. The headless
// CoreText ink oracle and owned native Root readback are separate gates.
#[test]
fn root_session_palette_and_high_effort_keep_authored_geometry() {
    for dark in [false, true] {
        let (_app, mut root, shell, requests) = fixture();
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.dark = dark;
                while let Ok(command) = requests.try_recv() {
                    if let Command::Send { request, .. } = command
                        && request["type"] == "get_available_thinking_levels"
                    {
                        s.ingest(
                            rpc(json!({"type":"response", "id":request["id"],
                            "command":"get_available_thinking_levels", "success":true,
                            "data":{"levels":["low", "high"]}})),
                            cx,
                        );
                    }
                }
                s.ingest(
                    rpc(json!({"type":"thinking_level_changed", "level":"high"})),
                    cx,
                );
                let project = &mut s.workspace.as_mut().unwrap().projects[0];
                for (i, title) in [
                    "gpui-rewrite".to_owned(),
                    "gypq debugging ".repeat(80),
                    "HH".to_owned(),
                ]
                .into_iter()
                .enumerate()
                {
                    project.sessions.push(tau_core::models::SessionSummary {
                        id: format!("ink-{i}"),
                        path: format!("/fixture/ink-{i}.jsonl"),
                        title,
                        title_is_derived: false,
                        title_markdown: None,
                        model: String::new(),
                        last_active: String::new(),
                        last_user_message_at: 0,
                        sort_at: 0,
                        archived: false,
                        selected: false,
                    });
                }
                s.sync(w, cx);
            });
        });
        root.draw();
        let footer = root.update(|_, w, cx| {
            let composer = shell.read(cx).sessions["a"].composer.read(cx);
            assert_eq!(composer.effort().unwrap().id, "high");
            let effort = w.find("effort").bounds();
            let label = w.find("effort-label").bounds();
            assert_eq!(effort.size.height, px(22.));
            assert_eq!(label.size.height, px(10.)); // preserve Button's neutral 1em grid
            assert!((label.center().y - effort.center().y).abs() <= px(0.5));
            w.find("footer-gap").bounds()
        });
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| s.show_palette_page("sessions", w, cx));
        });
        root.draw();
        root.update(|_, w, cx| {
            assert_eq!(
                shell
                    .read(cx)
                    .palette
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .page_id(),
                "sessions"
            );
            assert_eq!(w.find("footer-gap").bounds(), footer);
            // One mounted live session, followed by the three actual saved-session rows.
            for i in 0..4 {
                let row = w.find(format!("row-{i}")).bounds();
                let title = w.find(format!("palette-title-{i}")).bounds();
                assert_eq!(row.size.height, px(28.));
                assert_eq!(title.size.height, px(12.));
                assert!(title.left() >= row.left() && title.right() <= row.right());
            }
        });
    }
}
