use super::super::tests::{create, project, rpc, started, with_shell};
use super::*;
use serde_json::json;
fn drain(requests: &async_channel::Receiver<Command>) -> impl Iterator<Item = Command> + '_ {
    std::iter::from_fn(move || requests.try_recv().ok())
}
pub(super) fn fixture(s: &mut Shell, w: &mut Window, cx: &mut Context<Shell>) {
    fixture_with_pending_reads(s, w, cx, false);
}
pub(super) fn fixture_with_pending_reads(
    s: &mut Shell,
    w: &mut Window,
    cx: &mut Context<Shell>,
    pending: bool,
) {
    let mut p = project();
    p.sessions.push(serde_json::from_value(json!({"id":"saved","path":"/saved","title":"Original","lastActive":"now","lastUserMessageAt":0,"sortAt":0,"archived":false,"selected":true})).unwrap());
    s.workspace = Some(WorkspaceSnapshot {
        projects: vec![p.clone()],
        active_project_path: p.path.clone(),
        pi_path: None,
    });
    s.create_live(
        "live".into(),
        p,
        "saved".into(),
        "/saved".into(),
        "sacred".into(),
        w,
        cx,
    );
    started(s, "live", 1, cx);
    // Controller's bootstrap request IDs are deterministic and generation-scoped.
    s.ingest(rpc("live", 1, json!({"type":"response","id":"tau:live:g1:1","command":"get_state","success":true,"data":{"sessionId":"saved","sessionFile":"/saved","sessionName":"Original"}})), cx);
    for (n, command, data) in [
        (2, "get_messages", json!({"messages":[]})),
        (3, "get_available_models", json!({"models":[]})),
        (4, "get_commands", json!({"commands":[]})),
        (5, "get_messages", json!({"messages":[]})),
        (6, "get_available_thinking_levels", json!({"levels":[]})),
    ] {
        if pending && matches!(command, "get_available_models" | "get_commands") {
            continue;
        }
        s.ingest(rpc("live",1,json!({"type":"response","id":format!("tau:live:g1:{n}"),"command":command,"success":true,"data":data})),cx);
    }
    s.active = "live".into();
}
#[test]
fn captured_header_action_and_name_callback_do_not_touch_switched_session() {
    with_shell(128, |s, w, cx, requests| {
        fixture(s, w, cx);
        s.session_action(
            &Action::Rename {
                session_id: "live".into(),
                title: "New".into(),
            },
            w,
            cx,
        );
        let rename = drain(&requests)
            .find_map(|c| match c {
                Command::Send {
                    runtime,
                    generation,
                    request,
                } if request["type"] == "set_session_name" => Some((runtime, generation, request)),
                _ => None,
            })
            .unwrap();
        assert_eq!(rename.0, "live");
        assert_eq!(rename.1, 1);
        create(s, "other", "Other draft", w, cx);
        s.active = "other".into();
        assert!(s.session_action(
            &Action::Rename {
                session_id: "live".into(),
                title: "Leaked".into()
            },
            w,
            cx
        ));
        assert_eq!(s.sessions["live"].controller.state().identity.name, "New");
        s.ingest(rpc("live",1,json!({"type":"response","id":rename.2["id"],"command":"set_session_name","success":false})),cx);
        assert_eq!(
            s.sessions["live"].controller.state().identity.name,
            "Original"
        );
        assert_eq!(s.sessions["other"].controller.state().draft, "Other draft");
        s.session_actions.names.insert("live".into(), 1);
        s.finish_name(
            "live".into(),
            0,
            1,
            "saved".into(),
            "/saved".into(),
            "Original".into(),
            false,
        );
        assert!(s.session_actions.errors.is_empty());
        s.finish_name(
            "live".into(),
            1,
            1,
            "saved".into(),
            "/saved".into(),
            "Original".into(),
            false,
        );
        assert!(s.session_action_error().is_none());
        s.active = "live".into();
        assert!(s.session_action_error().is_some());
        s.retry_session_action(w, cx);
        assert!(
            drain(&requests)
                .any(|c| matches!(c,Command::StoreName { runtime, .. } if runtime=="live"))
        );
        assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
    });
}
#[test]
fn archive_is_optimistic_preserves_drafts_and_failure_is_local_retryable() {
    with_shell(128, |s, w, cx, requests| {
        fixture(s, w, cx);
        assert!(s.can_archive("live"));
        s.session_action(&Action::Archive("live".into()), w, cx);
        let (revision, project, id) = drain(&requests)
            .find_map(|c| match c {
                Command::Archive {
                    revision,
                    project,
                    id,
                    ..
                } => Some((revision, project, id)),
                _ => None,
            })
            .unwrap();
        assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
        assert_ne!(s.active, "live");
        assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
        let mut stale = s.workspace.clone().unwrap();
        stale.projects[0].sessions[0].archived = false;
        s.reconcile_archive(&mut stale);
        assert!(stale.projects[0].sessions[0].archived);
        s.finish_archive(project, id, revision, true, false);
        assert!(!s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
        assert!(s.session_action_error().is_none());
        s.active = "live".into();
        assert!(s.session_action_error().unwrap().contains("archived"));
        s.session_action(&Action::Archive("live".into()), w, cx);
        assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
    });
}
#[test]
fn archived_browse_is_not_restore_and_older_failure_cannot_undo_restore() {
    with_shell(128, |s, w, cx, _| {
        fixture(s, w, cx);
        s.session_action(&Action::Archive("live".into()), w, cx);
        let binding = (project().path, "saved".into());
        let archive = s.session_actions.archives[&binding].revision;
        s.session_action(&Action::ToggleArchived, w, cx);
        assert_eq!(s.archived_items().len(), 1);
        s.select("saved", w, cx);
        assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
        s.session_action(&Action::Restore("saved".into()), w, cx);
        assert!(s.archived_items().is_empty());
        s.finish_archive(binding.0.clone(), binding.1.clone(), archive, true, false);
        assert!(!s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
        assert_eq!(s.sessions["live"].controller.state().draft, "sacred");
    });
}
#[test]
fn empty_unsaved_and_working_sessions_cannot_archive() {
    with_shell(128, |s, w, cx, _| {
        fixture(s, w, cx);
        create(s, "empty", "", w, cx);
        assert!(!s.can_archive("empty"));
        s.ingest(rpc("live", 1, json!({"type":"agent_start"})), cx);
        assert!(!s.can_archive("live"));
        s.session_action(&Action::Archive("live".into()), w, cx);
        assert!(!s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
    });
}
