use super::*;
use crate::app::tests::{app, project, rpc, shell, started};
use gpui_kit::{MouseButton, MouseDownEvent, MouseUpEvent, size, test::TestWindowExt};
use serde_json::json;

const SKILL: &str = "<skill name=\"triage\" location=\"/invented/skills/triage/SKILL.md\">\nInvented instructions.\n</skill>\n\nFix the invented report.";

fn root_title_case(source: &str, expected: &str, history: bool, explicit: &str) {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(1024);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(800.)));
    root.update(|_, w, cx| shell.update(cx, |s, cx| {
        let mut p = project();
        p.sessions.push(serde_json::from_value(json!({
            "id":"invented-skill", "path":"/invented/session.jsonl",
            "title":if explicit.is_empty() { "New Session" } else { explicit },
            "titleIsDerived":explicit.is_empty(),
            "lastActive":"now", "lastUserMessageAt":0, "sortAt":0,
            "archived":false, "selected":true
        })).unwrap());
        s.workspace = Some(WorkspaceSnapshot {
            projects:vec![p.clone()], active_project_path:p.path.clone(), pi_path:None,
        });
        s.restored = true;
        s.create_live("skill-live".into(), p, "invented-skill".into(), "/invented/session.jsonl".into(), "sacred draft".into(), w, cx);
        if history {
            let live = s.sessions.get_mut("skill-live").unwrap();
            let token = live.controller.saved_preview_token().unwrap();
            assert!(live.controller.apply_saved_preview(&token, &[json!({"role":"user", "timestamp":123, "content":source})]));
            // The saved title must work before get_state (including named previews).
            assert_eq!(super::super::view::title(live), expected);
        }
        started(s, "skill-live", 1, cx);
        let bootstrap: Vec<_> = std::iter::from_fn(|| requests.try_recv().ok()).collect();
        let request_id = |kind: &str| bootstrap.iter().find_map(|command| match command {
            Command::Send { request, .. } if request["type"] == kind => Some(request["id"].clone()),
            _ => None,
        }).expect("actual bootstrap request");
        s.ingest(rpc("skill-live", 1, json!({"type":"response", "id":request_id("get_state"), "command":"get_state", "success":true, "data":{
            "sessionId":"invented-skill", "sessionFile":"/invented/session.jsonl", "sessionName":explicit
        }})), cx);
        let message = json!({"role":"user", "timestamp":123, "content":[{"type":"text", "text":source}]});
        if history {
            s.ingest(rpc("skill-live", 1, json!({"type":"response", "id":request_id("get_messages"), "command":"get_messages", "success":true, "data":{"messages":[message]}})), cx);
        } else {
            s.ingest(rpc("skill-live", 1, json!({"type":"message_start", "message":message})), cx);
        }
        s.mount(w, cx);
        s.sync(w, cx);
        assert_eq!(super::super::view::title(&s.sessions["skill-live"]), expected);
        let sidebar = s.palette_sidebar_projects();
        assert_eq!(sidebar[0].sessions[0].title, expected);
        assert_eq!(s.sessions["skill-live"].controller.state().identity.name, explicit);
        let entry = &s.sessions["skill-live"].controller.state().transcript[0];
        assert_eq!(entry.text, source);
        assert_eq!(s.sessions["skill-live"].controller.state().draft, "sacred draft");
    }));
    root.draw();
    let position = root.update(|_, w, _| {
        assert!(w.find("session-row-invented-skill").bounds().size.height > px(0.));
        w.find("session-name").bounds().center()
    });
    root.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    root.simulate_event(MouseUpEvent {
        position,
        button: MouseButton::Left,
        modifiers: Default::default(),
        click_count: 1,
    });
    root.draw();
    root.update(|_, _, cx| {
        let s = shell.read(cx);
        let header = s.header.as_ref().unwrap().read(cx);
        assert!(header.renaming());
        // The rename control is single-line; the presentation/source assertions above stay exact.
        assert_eq!(
            header.editor().read(cx).value().as_ref(),
            expected.replace(['\r', '\n'], "")
        );
    });
    root.simulate_keystroke("escape");
    if explicit.is_empty() {
        for command in std::iter::from_fn(|| requests.try_recv().ok()) {
            match command {
                Command::Register { name, .. } | Command::StoreName { name, .. } => assert!(
                    name.is_empty(),
                    "derived presentation must not become a Pi name"
                ),
                Command::Send { request, .. } => assert_ne!(request["type"], "set_session_name"),
                _ => {}
            }
        }
    }
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.workspace.as_mut().unwrap().projects[0].sessions[0].archived = true;
            s.session_actions.archived = true;
            s.sidebar_state.invalidate_metadata();
            s.sync(w, cx);
        })
    });
    root.draw();
    root.update(|_, w, cx| {
        assert!(w.find("archived-name-invented-skill").bounds().size.height > px(0.));
        let s = shell.read(cx);
        let archived = s
            .sidebar_state
            .archive_rows
            .iter()
            .find(|row| row.id == "invented-skill");
        assert_eq!(archived.map(|row| row.title.as_str()), Some(expected));
    });
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            assert!(s.session_action(
                &Action::Rename {
                    session_id: "skill-live".into(),
                    title: "Named after skill".into(),
                },
                w,
                cx
            ));
            s.sync(w, cx);
            assert_eq!(
                super::super::view::title(&s.sessions["skill-live"]),
                "Named after skill"
            );
            assert_eq!(s.sidebar_state.archive_rows[0].title, "Named after skill");
            assert_eq!(
                s.sessions["skill-live"].controller.state().transcript[0].text,
                source
            );
        })
    });
    root.draw();
}

#[test]
fn actual_root_header_sidebar_and_archive_show_first_live_skill_before_any_assistant() {
    root_title_case(SKILL, "/skill:triage", false, "");
}

#[test]
fn actual_root_header_sidebar_and_archive_show_hydrated_first_skill_after_restart() {
    root_title_case(SKILL, "/skill:triage", true, "");
}

#[test]
fn actual_root_skill_titles_preserve_explicit_names_and_ordinary_or_malformed_text() {
    root_title_case(SKILL, "Explicit Pi name", true, "Explicit Pi name");
    root_title_case(SKILL, SKILL, false, SKILL);
    for source in [
        "Ordinary first message",
        "<skill name=\"triage\" location=\"/invented/SKILL.md\">\nUnclosed",
    ] {
        root_title_case(source, source, false, "");
    }
}
