use super::*;
use crate::app::tests::{rpc, started};
use serde_json::json;

#[test]
fn new_session_focuses_current_editor_before_ready_without_reclaiming_palette_focus() {
    for open_palette in [false, true] {
        let mut app = app();
        let (commands, _requests) = async_channel::bounded(256);
        let shell = app.new_entity(|_| crate::app::tests::shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                s.workspace = Some(WorkspaceSnapshot {
                    projects: vec![project()],
                    active_project_path: project().path,
                    pi_path: None,
                });
                s.mount(w, cx);
                s.action(Action::NewSession, w, cx);
            })
        });
        focus_frames(&mut root);
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            assert!(
                s.sessions[&s.active]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w)
            );
        });
        let key = app.update(|cx| shell.read(cx).active.clone());
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        if open_palette {
            root.simulate_keystroke(&format!("{modifier}-k"));
            assert!(app.update(|cx| shell.read(cx).palette.is_open(cx)));
        }
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                started(s, &key, 1, cx);
                s.ingest(
                    rpc(
                        &key,
                        1,
                        json!({
                            "type":"response", "id":format!("tau:{key}:g1:1"),
                            "command":"get_state", "success":true, "data":{}
                        }),
                    ),
                    cx,
                );
                s.sync(w, cx);
            })
        });
        focus_frames(&mut root);
        root.update(|_, w, cx| {
            let s = shell.read(cx);
            let focus = if open_palette {
                s.palette
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .search_editor()
                    .read(cx)
                    .focus_handle(cx)
            } else {
                s.sessions[&key]
                    .composer
                    .read(cx)
                    .editor()
                    .read(cx)
                    .focus_handle(cx)
            };
            assert!(focus.is_focused(w));
        });
        if !open_palette {
            root.simulate_input("retained new-session draft");
            assert_eq!(
                app.update(|cx| shell.read(cx).sessions[&key]
                    .controller
                    .state()
                    .draft
                    .clone()),
                "retained new-session draft"
            );
            root.simulate_keystroke(&format!("{modifier}-k"));
            assert!(app.update(|cx| shell.read(cx).palette.is_open(cx)));
        }
    }
}
