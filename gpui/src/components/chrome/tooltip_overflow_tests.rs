use super::sidebar_audit_tests::{bounds, fixture};
use super::*;
use crate::components::renderer_interaction_tests::shaped_rows;
use gpui_kit::{point, size};

#[test]
fn root_rich_tooltip_long_words_paint_inside_surface_without_losing_copy() {
    for dark in [false, true] {
        for width in [280., 720.] {
            for archived in [false, true] {
                for message in [
                    format!("/ssh/{}", "unbrokendirectory".repeat(12)),
                    "A long plain multiword title with enough words to wrap onto several lines while retaining every last word".into(),
                    format!("**Title** with `/{}` end", "unbrokendirectory".repeat(10)),
                ] {
                    let (app, mut window, sidebar, _) = fixture(dark);
                    window.simulate_resize(size(px(width), px(600.)));
                    window.update(|_, w, cx| {
                        w.bounds_changed(cx);
                        sidebar.update(cx, |s, cx| {
                            if archived {
                                let mut props = s.props.clone();
                                props.archived = true;
                                props.sessions = s.projects[0].sessions.clone();
                                s.set_props(props, cx);
                            }
                            s.set_session_details(BTreeMap::from([("pi-session-id".into(), SessionDetails {
                                status: if archived { "Archived" } else { "Idle" }.into(),
                                message: message.clone(),
                                relative_time: Some("2m".into()),
                            })]), cx);
                        });
                    });
                    window.draw();
                    let anchor = bounds(&mut window, if archived { "archived-select-pi-session-id" } else { "session-pi-session-id" });
                    window.simulate_mouse_move(anchor.center());
                    app.run_until_parked();
                    let focus = window.update(|_, w, cx| w.focused(cx));
                    app.advance_clock(Duration::from_millis(699));
                    app.run_until_parked();
                    window.draw();
                    window.read(|_, cx| assert!(sidebar.read(cx).tooltip_label().is_none()));
                    app.advance_clock(Duration::from_millis(1));
                    app.run_until_parked();
                    window.draw();
                    app.run_until_parked();
                    window.draw();
                    let tip = bounds(&mut window, "chrome-tooltip");
                    let (text, len) = window.update(|_, w, cx| {
                        assert_eq!(w.focused(cx), focus);
                        let s = sidebar.read(cx);
                        assert_eq!(s.session_details["pi-session-id"].message, message);
                        let text = s.tooltip_markdown["pi-session-id"].clone();
                        let rendered = text.read(cx).rendered_text();
                        let expected = message.replace("**", "").replace('`', "");
                        assert_eq!(rendered.as_str().trim(), expected);
                        (text, rendered.as_str().trim_end().len())
                    });
                    let rows = shaped_rows(&mut window, &text, 0..len);
                    assert!(rows.len() > 1, "long content must really wrap");
                    for row in rows {
                        assert!(row.left() >= tip.left() + px(12.) - px(0.5) && row.right() <= tip.right() - px(12.) + px(0.5), "painted range escapes tooltip: {row:?}, {tip:?}");
                    }
                    window.update(|_, _, cx| text.update(cx, |text, cx| {
                        text.select_all(cx);
                        assert_eq!(text.selected_text().trim(), message.replace("**", "").replace('`', ""));
                    }));
                    assert!(tip.size.width <= px(300.));
                    assert!(tip.left() >= px(6.) && tip.right() <= px(width - 6.));
                }
            }
        }
    }
}

#[test]
fn root_ssh_project_tooltip_wraps_actual_shaped_label_at_viewport_cap() {
    for dark in [false, true] {
        for width in [220., 720.] {
            for location in [
                format!("timur@{}.example:/{}", "unbrokenhost".repeat(8), "unbrokendirectory".repeat(12)),
                "A long plain multiword project location with enough words to exceed the compact tooltip width".into(),
            ] {
                let (app, mut window, sidebar, _) = fixture(dark);
                window.simulate_resize(size(px(width), px(600.)));
                window.update(|_, w, cx| {
                    w.bounds_changed(cx);
                    sidebar.update(cx, |s, cx| {
                        let mut projects = s.projects.clone();
                        projects[0].location = location.clone();
                        s.set_projects(projects, cx);
                    });
                });
                window.draw();
                let anchor = bounds(&mut window, "project-toggle-project-id");
                window.simulate_mouse_move(anchor.origin + point(px(20.), px(10.)));
                app.run_until_parked();
                let focus = window.update(|_, w, cx| w.focused(cx));
                app.advance_clock(Duration::from_millis(699));
                app.run_until_parked();
                window.draw();
                window.read(|_, cx| assert!(sidebar.read(cx).tooltip_label().is_none()));
                app.advance_clock(Duration::from_millis(1));
                app.run_until_parked();
                window.draw();
                let tip = bounds(&mut window, "chrome-tooltip");
                window.update(|_, w, cx| {
                    assert_eq!(w.focused(cx), focus);
                    let s = sidebar.read(cx);
                    assert_eq!(s.tooltip_label().as_deref(), Some(location.as_str()));
                    let layout = s.hint.label_layout.borrow();
                    let layout = layout.as_ref().unwrap();
                    assert_eq!(layout.text(), location);
                    assert_eq!(layout.wrapped_text().replace('\n', ""), location);
                    assert!(layout.bounds().left() >= tip.left() + px(8.));
                    assert!(layout.bounds().right() <= tip.right() - px(8.), "actual label allocation exceeds tooltip: {:?} vs {tip:?}", layout.bounds());
                    for line in layout.line_layouts() {
                        assert!(line.width() <= tip.size.width - px(16.) + px(0.5), "actual shaped line exceeds content cap: {:?} vs {tip:?}", line.width());
                        assert!(!line.wrap_boundaries.is_empty(), "long location must wrap, not clip");
                    }
                });
                assert!(tip.size.width <= px(260.).min(px(width - 12.)));
                assert!(tip.left() >= px(6.) && tip.right() <= px(width - 6.));
            }
        }
    }
}
