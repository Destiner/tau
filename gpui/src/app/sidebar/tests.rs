use super::*;
use crate::app::tests::{create, project, rpc, started, with_shell};
use serde_json::json;

fn projected(s: &Shell) -> (Vec<ProjectItem>, BTreeMap<String, SessionDetails>) {
    s.sidebar_projection(
        Local::now().timestamp_millis().max(0) as u64,
        &runtime_index(&s.sessions),
    )
}
fn saved_fixture(s: &mut Shell, w: &mut Window, cx: &mut Context<Shell>) {
    let mut p = project();
    p.sessions.push(serde_json::from_value(json!({"id":"saved","path":"/saved","title":"Original","lastActive":"now","sortAt":0,"lastUserMessageAt":0,"archived":false,"selected":true})).unwrap());
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
    s.ingest(rpc("live", 1, json!({"type":"response","id":"tau:live:g1:1","command":"get_state","success":true,"data":{"sessionId":"saved","sessionFile":"/saved","sessionName":"Original"}})), cx);
    for (n, command, data) in [
        (2, "get_messages", json!({"messages":[]})),
        (3, "get_available_models", json!({"models":[]})),
        (4, "get_commands", json!({"commands":[]})),
        (5, "get_messages", json!({"messages":[]})),
        (6, "get_available_thinking_levels", json!({"levels":[]})),
    ] {
        s.ingest(rpc("live", 1, json!({"type":"response","id":format!("tau:live:g1:{n}"),"command":command,"success":true,"data":data})), cx);
    }
    s.active = "live".into();
}
fn row(s: &Shell, id: &str) -> SessionItem {
    let semantic = s
        .sessions
        .get(id)
        .map(|live| SessionIdentity::live(id, live));
    let id = semantic
        .as_ref()
        .filter(|binding| !binding.pi_id.is_empty())
        .map_or(id, |binding| binding.pi_id.as_str());
    projected(s)
        .0
        .into_iter()
        .flat_map(|p| p.sessions)
        .find(|s| s.id == id)
        .unwrap()
}

#[test]
fn saved_and_ephemeral_dirty_drafts_ignore_whitespace_and_use_vue_priority() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        assert_eq!(row(s, "live").status, Some(SessionStatus::Draft));
        s.sessions
            .get_mut("live")
            .unwrap()
            .controller
            .set_draft(" \n\t ");
        assert_eq!(row(s, "live").status, None);
        create(s, "ephemeral", "unsaved work", w, cx);
        assert_eq!(row(s, "ephemeral").status, Some(SessionStatus::Draft));
        s.sessions
            .get_mut("ephemeral")
            .unwrap()
            .controller
            .set_draft("  ");
        assert_eq!(row(s, "ephemeral").status, None);
        s.active = "ephemeral".into();
        s.ingest(rpc("live", 1, json!({"type":"agent_start"})), cx);
        s.ingest(rpc("live", 1, json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"answer"}})), cx);
        assert!(row(s, "live").unread);
        assert_eq!(row(s, "live").status, Some(SessionStatus::Working));
        s.ingest(rpc("live", 1, json!({"type":"extension_ui_request","id":"question","method":"confirm","title":"Continue?"})), cx);
        assert_eq!(row(s, "live").status, Some(SessionStatus::Unread));
        assert_eq!(projected(s).1["saved"].status, "Unread");
    });
}

#[test]
fn completion_is_protocol_driven_and_select_clears_current_identity_only() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        s.sessions.get_mut("live").unwrap().controller.set_draft("");
        create(s, "selected", "", w, cx);
        s.active = "selected".into();
        s.ingest(rpc("live", 1, json!({"type":"agent_start"})), cx);
        s.ingest(rpc("live", 1, json!({"type":"agent_end"})), cx);
        assert!(!row(s, "live").unread);
        s.ingest(rpc("live", 0, json!({"type":"agent_settled"})), cx);
        assert!(!row(s, "live").unread);
        s.ingest(rpc("live", 1, json!({"type":"agent_settled"})), cx);
        assert_eq!(row(s, "live").status, Some(SessionStatus::Unread));
        s.select("live", w, cx);
        assert!(!row(s, "live").unread);
        s.ingest(rpc("live", 1, json!({"type":"agent_start"})), cx);
        s.ingest(rpc("live", 1, json!({"type":"agent_settled"})), cx);
        assert!(!row(s, "live").unread);
        assert!(s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "live".into(),
            unread: true
        }));
        assert!(row(s, "live").unread);
        s.sync_sidebar(w, cx);
        assert!(row(s, "live").unread); // Manual Mark Unread survives selection-neutral sync.
        let identity = SessionIdentity::live("live", &s.sessions["live"]);
        assert!(!s.sidebar_state.unread.contains(&SessionIdentity::saved(
            "different-project",
            &identity.pi_id
        )));
    });
}

#[test]
fn abort_and_transport_failure_do_not_fake_completion() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        s.sessions.get_mut("live").unwrap().controller.set_draft("");
        create(s, "selected", "", w, cx);
        s.active = "selected".into();
        s.ingest(rpc("live", 1, json!({"type":"agent_start"})), cx);
        s.sessions.get_mut("live").unwrap().controller.abort();
        assert!(!row(s, "live").unread);
        s.sessions
            .get_mut("live")
            .unwrap()
            .controller
            .transport_failed(1);
        assert!(!row(s, "live").unread);
        // Vue marks the actual settlement event, even if an abort preceded it.
        s.ingest(rpc("live", 1, json!({"type":"agent_settled"})), cx);
        assert!(row(s, "live").unread);
    });
}

#[test]
fn projection_uses_saved_markdown_ssh_location_and_runtime_user_timestamp() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        let p = &mut s.workspace.as_mut().unwrap().projects[0];
        p.connection_string = Some("ssh-alias".into());
        p.working_directory = "/remote/work".into();
        p.sessions[0].title_markdown = Some("**Actual user request**".into());
        p.sessions[0].sort_at = 1000;
        s.sessions.get_mut("live").unwrap().controller.set_draft("");
        s.ingest(rpc("live", 1, json!({"type":"message_start","message":{"role":"user","timestamp":120000,"content":"request"}})), cx);
        s.ingest(rpc("live", 1, json!({"type":"tool_execution_end","toolCallId":"bash","result":{"content":[{"type":"text","text":"Not the last message"}]}})), cx);
        let (projects, details) = s.sidebar_projection(180000, &runtime_index(&s.sessions));
        assert_eq!(projects[0].location, "ssh-alias · /remote/work");
        assert_eq!(details["saved"].message, "**Actual user request**");
        assert_eq!(details["saved"].relative_time.as_deref(), Some("1m"));
    });
}

#[test]
fn archive_order_is_global_stable_milliseconds_and_cache_ignores_streaming() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        let original = &mut s.workspace.as_mut().unwrap().projects[0];
        original.sessions[0].archived = true;
        original.sessions[0].sort_at = 1000;
        original.sessions[0].model = "model-a".into();
        let mut other = project();
        other.path = "/other".into();
        other.name = "Other".into();
        for (id, sort, user) in [
            ("latest", 3000, 0),
            ("tie-first", 2000, 2001),
            ("tie-second", 2000, 2001),
            ("tie-lower", 2000, 1999),
        ] {
            other.sessions.push(serde_json::from_value(json!({"id":id,"path":format!("/{id}"),"title":id,"lastActive":"now","sortAt":sort,"lastUserMessageAt":user,"archived":true,"selected":false})).unwrap());
        }
        s.workspace.as_mut().unwrap().projects.push(other);
        assert_eq!(
            s.archived_items()
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["latest", "tie-first", "tie-second", "tie-lower", "saved"]
        );
        s.sync_sidebar(w, cx);
        assert_eq!(s.sidebar_state.archive_details["saved"].project, "Project");
        assert_eq!(
            s.sidebar_state.archive_details["saved"].model.as_deref(),
            Some("model-a")
        );
        assert_eq!(projected(s).1["saved"].status, "Archived");
        create(s, "background", "", w, cx);
        s.sidebar_state.runtime_created("background");
        started(s, "background", 2, cx);
        s.ingest(rpc("background", 2, json!({"type":"agent_start"})), cx);
        s.sync_sidebar(w, cx);
        let rebuilds = s.sidebar_state.archive_rebuilds;
        let row_ptr = s.sidebar_state.archive_rows.as_ptr();
        for _ in 0..10 {
            s.ingest(rpc("background", 2, json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"stream"}})), cx);
            s.sync_sidebar(w, cx);
        }
        assert_eq!(s.sidebar_state.archive_rebuilds, rebuilds);
        assert_eq!(s.sidebar_state.archive_rows.as_ptr(), row_ptr);
    });
}

#[test]
fn dormant_saved_draft_is_projected_without_materializing_a_runtime() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        s.sessions.clear();
        s.preferences.drafts.push(Draft {
            project: project().path,
            source_id: "saved".into(),
            text: "retained work".into(),
            ..Draft::default()
        });
        assert_eq!(row(s, "saved").status, Some(SessionStatus::Draft));
        s.preferences.drafts[0].text = " \n\t".into();
        assert_eq!(row(s, "saved").status, None);
        assert!(s.sessions.is_empty());
    });
}

#[test]
fn runtime_reuse_fences_saved_identity_but_materialization_preserves_unread() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "live".into(),
            unread: true,
        });
        s.sessions
            .get_mut("live")
            .unwrap()
            .controller
            .bind_source("replacement", "/replacement");
        s.sidebar_state.invalidate_metadata();
        s.sync_sidebar(w, cx);
        assert!(!row(s, "live").unread);
        create(s, "ephemeral", "", w, cx);
        s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "ephemeral".into(),
            unread: true,
        });
        s.sync_sidebar(w, cx);
        s.sessions
            .get_mut("ephemeral")
            .unwrap()
            .controller
            .bind_source("materialized", "/materialized");
        s.sidebar_state.invalidate_metadata();
        s.sync_sidebar(w, cx);
        assert!(row(s, "ephemeral").unread);
        s.mark_sidebar_read("ephemeral");
        assert!(!row(s, "ephemeral").unread);
    });
}

#[test]
fn archive_groups_use_local_calendar_and_monday_weeks_not_utc_days() {
    let offset = chrono::FixedOffset::east_opt(2 * 3600).unwrap();
    let now = offset.with_ymd_and_hms(2026, 6, 10, 0, 10, 0).unwrap();
    let timestamp = |day, hour| {
        offset
            .with_ymd_and_hms(2026, 6, day, hour, 0, 0)
            .unwrap()
            .timestamp_millis() as u64
    };
    assert_eq!(group_at(timestamp(10, 0), now), "Today");
    assert_eq!(group_at(timestamp(9, 23), now), "Yesterday");
    assert_eq!(group_at(timestamp(8, 12), now), "This Week");
    assert_eq!(group_at(timestamp(7, 12), now), "Last Week");
    assert_eq!(group_at(timestamp(1, 12), now), "Last Week");
    assert_eq!(group_at(0, now), "Older");
    assert_eq!(group_at(timestamp(11, 0), now), "Today");
    assert_eq!(relative_time(1000, "", 61000), "1m");
}

#[test]
fn unsent_drafts_sort_by_creation_not_runtime_key_or_user_activity() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        for key in ["z-first", "a-second", "m-newest"] {
            create(s, key, "draft", w, cx);
            s.sidebar_state.runtime_created(key);
        }
        let binding = SessionIdentity::live("z-first", &s.sessions["z-first"]);
        s.sidebar_state.last_user.insert(binding, u64::MAX);
        let ids: Vec<_> = projected(s).0[0]
            .sessions
            .iter()
            .map(|r| r.id.clone())
            .collect();
        assert_eq!(ids, ["m-newest", "a-second", "z-first", "saved"]);
    });
}

#[test]
fn unchanged_stream_skips_full_projection_and_metadata_even_in_archive_mode() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        s.sidebar =
            Some(cx.new(|cx| Sidebar::new(sidebar_props(), vec![], app_version(), s.dark, cx)));
        s.session_actions.archived = true;
        s.active = "other".into();
        let now = Local.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        let delta = json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"stream"}});
        // The first background text marks unread once; subsequent chunks are content-only.
        s.observe_sidebar_event("live", 1, &delta);
        s.sync_sidebar_at(now, w, cx);
        let counts = (
            s.sidebar_state.projection_builds,
            s.sidebar_state.metadata_builds,
            s.sidebar_state.archive_rebuilds,
        );
        let rows = s.sidebar_state.archive_rows.as_ptr();
        for _ in 0..100 {
            s.observe_sidebar_event("live", 1, &delta);
            s.sync_sidebar_at(now, w, cx);
        }
        assert_eq!(
            counts,
            (
                s.sidebar_state.projection_builds,
                s.sidebar_state.metadata_builds,
                s.sidebar_state.archive_rebuilds
            )
        );
        assert_eq!(rows, s.sidebar_state.archive_rows.as_ptr());
        s.active = "live".into();
        s.dark = !s.dark;
        s.sync_sidebar_at(now, w, cx);
        assert_eq!(counts.0, s.sidebar_state.projection_builds);
        assert_eq!(counts.1, s.sidebar_state.metadata_builds);
    });
}

#[test]
fn timer_ages_minutes_and_calendar_groups_without_reprojecting_archive_metadata() {
    with_shell(512, |s, w, cx, _| {
        saved_fixture(s, w, cx);
        let now = Local.with_ymd_and_hms(2026, 6, 10, 23, 58, 0).unwrap();
        let session = &mut s.workspace.as_mut().unwrap().projects[0].sessions[0];
        session.archived = true;
        session.sort_at = now.timestamp_millis() as u64;
        s.sidebar_state.invalidate_metadata();
        s.sync_sidebar_at(now, w, cx);
        let builds = s.sidebar_state.metadata_builds;
        let rebuilds = s.sidebar_state.archive_rebuilds;
        assert_eq!(s.sidebar_state.archive_rows[0].time, "now");
        assert_eq!(s.sidebar_state.archive_details["saved"].group, "Today");
        s.sync_sidebar_at(now + chrono::Duration::seconds(59), w, cx);
        assert_eq!(s.sidebar_state.archive_rows[0].time, "now");
        s.sync_sidebar_at(now + chrono::Duration::minutes(1), w, cx);
        assert_eq!(s.sidebar_state.archive_rows[0].time, "1m");
        assert_eq!(s.sidebar_state.archive_rebuilds, rebuilds);
        s.sync_sidebar_at(now + chrono::Duration::minutes(2), w, cx);
        assert_eq!(s.sidebar_state.archive_rows[0].time, "2m");
        assert_eq!(s.sidebar_state.archive_details["saved"].group, "Yesterday");
        assert_eq!(s.sidebar_state.archive_rebuilds, rebuilds + 1);
        assert_eq!(s.sidebar_state.metadata_builds, builds);
    });
}
