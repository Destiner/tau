use super::*;
use crate::app::tests::{project, rpc, started, with_shell};
use serde_json::json;

fn titles(s: &Shell) -> Vec<String> {
    s.sidebar_projection(600_000, &runtime_index(&s.sessions)).0[0]
        .sessions
        .iter()
        .map(|row| row.title.clone())
        .collect()
}

fn workspace() -> WorkspaceSnapshot {
    let mut p = project();
    for (id, timestamp) in [
        ("first", 400_000),
        ("second", 300_000),
        ("third", 200_000),
        ("fourth", 100_000),
    ] {
        p.sessions.push(
            serde_json::from_value(json!({
                "id":id,"path":format!("/{id}"),"title":id,"lastActive":"now",
                "sortAt":timestamp,"lastUserMessageAt":timestamp,"archived":false,"selected":false
            }))
            .unwrap(),
        );
    }
    WorkspaceSnapshot {
        projects: vec![p.clone()],
        active_project_path: p.path,
        pi_path: None,
    }
}

// Exercise Shell's real ingress and request IDs, not a parallel hydration reducer.
fn hydrate(
    s: &mut Shell,
    runtime: &str,
    id: &str,
    requests: &async_channel::Receiver<Command>,
    w: &mut Window,
    cx: &mut Context<Shell>,
    expected: &[&str],
) {
    let mut replies = 0;
    while let Ok(command) = requests.try_recv() {
        let Command::Send {
            runtime: target,
            generation,
            request,
        } = command
        else {
            continue;
        };
        assert_eq!(target, runtime);
        let command = request["type"].as_str().unwrap();
        let data = match command {
            "get_state" => json!({"sessionId":id,"sessionFile":format!("/{id}"),"sessionName":id}),
            // Hydrated messages (including newer timestamps) are history, not submissions.
            "get_messages" => json!({"messages":[
                {"role":"user","timestamp":900_000,"content":[{"type":"text","text":"saved request"}]},
                {"role":"assistant","timestamp":950_000,"content":[{"type":"text","text":"saved answer"}]}
            ]}),
            "get_available_models" => json!({"models":[]}),
            "get_scoped_models" => json!({"models":[],"enabled":false}),
            "get_commands" => json!({"commands":[]}),
            "get_available_thinking_levels" => json!({"levels":[]}),
            other => panic!("unexpected bootstrap request: {other}"),
        };
        s.ingest(rpc(runtime, generation, json!({"type":"response","id":request["id"],"command":command,"success":true,"data":data})), cx);
        s.sync_sidebar(w, cx);
        assert_eq!(titles(s), expected);
        replies += 1;
        assert!(replies < 30, "bootstrap must settle");
    }
    assert!(replies >= 6);
    assert!(s.sessions[runtime].controller.state().ready);
    assert!(!s.sessions[runtime].controller.state().history_loading);
}

#[test]
fn cold_and_warm_saved_selections_do_not_promote_hydration_to_activity() {
    with_shell(1024, |s, w, cx, requests| {
        s.workspace = Some(workspace());
        s.sidebar =
            Some(cx.new(|cx| Sidebar::new(sidebar_props(), vec![], app_version(), s.dark, cx)));
        s.sidebar_state.invalidate_metadata();
        s.sync_sidebar(w, cx);
        let expected = ["first", "second", "third", "fourth"];
        assert_eq!(titles(s), expected);
        let mut runtimes = HashMap::new();
        for id in [
            "fourth", "second", "third", "first", "fourth", "third", "second", "first",
        ] {
            s.select(id, w, cx);
            let runtime = s.active.clone();
            assert_eq!(titles(s), expected);
            if !runtimes.contains_key(id) {
                started(s, &runtime, 1, cx);
                hydrate(s, &runtime, id, &requests, w, cx, &expected);
                runtimes.insert(id, runtime);
            } else {
                assert_eq!(runtimes[id], runtime);
                while let Ok(command) = requests.try_recv() {
                    assert!(!matches!(command, Command::Send { .. }));
                }
            }
            // Selection persistence reload is a metadata boundary, not new activity.
            s.ingest(Event::Workspace(Ok(workspace())), cx);
            s.sync_sidebar(w, cx);
            assert_eq!(titles(s), expected);
            assert!(s.sidebar_state.last_user.is_empty());
        }
        let runtime = &runtimes["fourth"];
        let user = json!({"type":"message_start","message":{"role":"user","timestamp":500_000,"content":[{"type":"text","text":"new request"}]}});
        s.ingest(rpc(runtime, 0, user.clone()), cx);
        assert_eq!(titles(s), expected);
        s.ingest(rpc(runtime, 1, user), cx);
        s.sync_sidebar(w, cx);
        assert_eq!(titles(s), ["fourth", "first", "second", "third"]);
        s.select("second", w, cx);
        s.sync_sidebar(w, cx);
        assert_eq!(titles(s), ["fourth", "first", "second", "third"]);
    });
}

#[test]
fn replacement_activity_does_not_reorder_or_mark_saved_predecessor() {
    with_shell(1024, |s, w, cx, requests| {
        s.workspace = Some(workspace());
        s.select("fourth", w, cx);
        let runtime = s.active.clone();
        started(s, &runtime, 1, cx);
        hydrate(
            s,
            &runtime,
            "fourth",
            &requests,
            w,
            cx,
            &["first", "second", "third", "fourth"],
        );
        // SessionInfo ingress requests an authoritative state refresh.
        s.ingest(rpc(&runtime, 1, json!({"type":"session_info_changed"})), cx);
        let request = loop {
            if let Command::Send { request, .. } = requests.try_recv().unwrap()
                && request["type"] == "get_state"
            {
                break request;
            }
        };
        s.ingest(rpc(&runtime, 1, json!({"type":"response","id":request["id"],"command":"get_state","success":true,
            "data":{"sessionId":"replacement","sessionFile":"/replacement","sessionName":"replacement"}})), cx);
        s.active = String::new();
        s.ingest(
            rpc(
                &runtime,
                1,
                json!({"type":"message_start","message":{"role":"user","timestamp":1_000_000}}),
            ),
            cx,
        );
        s.ingest(rpc(&runtime, 1, json!({"type":"agent_settled"})), cx);
        s.sync_sidebar(w, cx);
        let (projects, _) = s.sidebar_projection(1_060_000, &runtime_index(&s.sessions));
        let rows = &projects[0].sessions;
        assert_eq!(
            titles(s),
            ["replacement", "first", "second", "third", "fourth"]
        );
        let predecessor = rows.iter().find(|r| r.id == "fourth").unwrap();
        assert!(!predecessor.unread);
        assert_eq!(predecessor.status, None);
        assert_eq!(predecessor.time, "16m");
        assert!(rows.iter().find(|r| r.id == "replacement").unwrap().unread);
        assert!(rows.iter().all(|r| r.id != *runtime));
        // Actions carry canonical IDs too, not only generated runtime keys.
        assert!(s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "replacement".into(),
            unread: false
        }));
        assert!(
            !s.sidebar_state
                .unread
                .contains(&SessionIdentity::saved(&project().path, "replacement"))
        );
        s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "fourth".into(),
            unread: true,
        });
        assert!(
            s.sidebar_state
                .unread
                .contains(&SessionIdentity::saved(&project().path, "fourth"))
        );
        for saved in &mut s.workspace.as_mut().unwrap().projects[0].sessions {
            saved.archived = saved.id == "first" || saved.id == "fourth";
        }
        s.sidebar_state.invalidate_metadata();
        s.sync_sidebar(w, cx);
        assert_eq!(
            s.sidebar_state
                .archive_rows
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "fourth"]
        );
        let archived = s
            .sidebar_state
            .archive_source
            .iter()
            .find(|m| m.id == "fourth")
            .unwrap();
        assert_eq!(archived.title, "fourth");
        assert_eq!(archived.activity_at, 100_000);
        assert!(
            !s.sidebar_state
                .unread
                .contains(&SessionIdentity::saved(&project().path, "replacement"))
        );
    });
}
