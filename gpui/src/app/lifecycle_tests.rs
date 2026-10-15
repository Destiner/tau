use super::*;
use crate::app::tests::{app, create, project, rpc, shell, started, with_shell};
use gpui_kit::{MouseButton, size, test::TestWindowExt};
use serde_json::json;

#[cfg(unix)]
#[path = "batch_stop_review_tests.rs"]
mod batch_stop_review_tests;
#[path = "lifecycle_cancel_review_tests.rs"]
mod cancel_review_tests;
#[cfg(unix)]
#[path = "input_drain_review_tests.rs"]
mod input_drain_review_tests;
#[path = "lifecycle_review_tests.rs"]
mod review_tests;

fn row(id: &str, path: &str) -> tau_core::models::SessionSummary {
    tau_core::models::SessionSummary {
        id: id.into(),
        path: path.into(),
        title: id.into(),
        title_markdown: None,
        model: String::new(),
        last_active: String::new(),
        last_user_message_at: 0,
        sort_at: 0,
        archived: false,
        selected: false,
    }
}
fn saved(s: &mut Shell, key: &str, w: &mut Window, cx: &mut Context<Shell>) {
    let path = format!("/{key}.jsonl");
    if s.workspace.is_none() {
        s.workspace = Some(WorkspaceSnapshot {
            projects: vec![project()],
            active_project_path: project().path,
            pi_path: None,
        });
    }
    s.workspace.as_mut().unwrap().projects[0]
        .sessions
        .push(row(key, &path));
    s.create_retained(
        key.into(),
        project(),
        key.into(),
        path,
        format!("draft {key}"),
        w,
        cx,
    );
    s.touch_runtime(key);
    s.start_live(key, cx);
    started(s, key, 7, cx);
}
fn reads(
    s: &mut Shell,
    key: &str,
    identity: &str,
    path: &str,
    cx: &mut Context<Shell>,
    requests: &async_channel::Receiver<Command>,
) -> Vec<Command> {
    let mut other = vec![];
    while let Ok(command) = requests.try_recv() {
        let Command::Send {
            runtime,
            generation,
            request,
        } = &command
        else {
            other.push(command);
            continue;
        };
        if runtime != key {
            other.push(command);
            continue;
        }
        let data = match request["type"].as_str().unwrap() {
            "get_state" => json!({"sessionId":identity,"sessionFile":path,"isStreaming":false}),
            "get_messages" => json!({"messages":[]}),
            "get_commands" => json!({"commands":[]}),
            "get_available_thinking_levels" => json!({"levels":[]}),
            _ => json!({"models":[]}),
        };
        s.ingest(rpc(key, *generation, json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data})), cx);
    }
    other
}

#[test]
fn seventh_inactive_idle_runtime_releases_lru_only_after_ack_and_retains_draft() {
    with_shell(512, |s, w, cx, requests| {
        s.active = "selected".into();
        let mut commands = vec![];
        for index in 0..7 {
            let key = format!("r{index}");
            saved(s, &key, w, cx);
            commands.extend(reads(
                s,
                &key,
                &key,
                &format!("/{key}.jsonl"),
                cx,
                &requests,
            ));
        }
        let releases: Vec<_> = commands
            .into_iter()
            .filter_map(|c| match c {
                Command::ReleaseRuntime {
                    runtime,
                    attempt,
                    generation,
                } => Some((runtime, attempt, generation)),
                _ => None,
            })
            .collect();
        assert_eq!(releases.len(), 1);
        let (runtime, attempt, generation) = releases[0].clone();
        assert_eq!(runtime, "r0");
        assert!(s.sessions[&runtime].controller.state().ready);
        s.ingest(
            Event::RuntimeReleased {
                runtime: runtime.clone(),
                attempt,
                generation,
                success: false,
            },
            cx,
        );
        assert!(s.sessions[&runtime].controller.state().ready);
        assert!(s.retry_workflow_pending());
        s.retry_workflows(cx);
        s.retry_workflows(cx);
        assert_eq!(
            std::iter::from_fn(|| requests.try_recv().ok())
                .filter(|c| matches!(c, Command::ReleaseRuntime { .. }))
                .count(),
            1
        );
        s.ingest(
            Event::RuntimeReleased {
                runtime: runtime.clone(),
                attempt,
                generation,
                success: true,
            },
            cx,
        );
        assert!(!s.sessions[&runtime].controller.state().ready);
        assert_eq!(s.sessions[&runtime].controller.state().draft, "draft r0");
        assert_eq!(s.sessions[&runtime].controller.generation(), generation);
        s.select(&runtime, w, cx);
        assert!(std::iter::from_fn(|| requests.try_recv().ok()).any(
            |c| matches!(c, Command::Start { source: Some(path), .. } if path == "/r0.jsonl")
        ));
    });
}

#[test]
fn remove_storage_failure_keeps_project_and_draft_then_cleanup_is_owned_and_retriable() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        s.active = "a".into();
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        s.mutate_workspace(
            workspace_mutations::Mutation::Collapse {
                project: project().path,
                collapsed: true,
            },
            cx,
        );
        assert!(s.handle_read_state(&Action::SetUnread {
            project_id: project().path,
            session_id: "a".into(),
            unread: true
        }));
        s.remove_project(&project().path, cx);
        let token = s.runtime_lifecycle.removals[&project().path].token;
        s.ingest(
            Event::ProjectRemoved {
                project: project().path,
                token,
                success: false,
            },
            cx,
        );
        assert_eq!(s.workspace.as_ref().unwrap().projects.len(), 1);
        assert_eq!(s.sessions["a"].controller.state().draft, "draft a");
        s.retry_workflows(cx);
        s.ingest(
            Event::ProjectRemoved {
                project: project().path,
                token,
                success: true,
            },
            cx,
        );
        assert!(s.workspace.as_ref().unwrap().projects.is_empty());
        assert!(s.sessions.is_empty() && s.active.is_empty());
        let cleanup = s.runtime_lifecycle.releases.keys().next().unwrap().clone();
        s.ingest(
            Event::RuntimeReleased {
                runtime: cleanup.runtime.clone(),
                attempt: cleanup.attempt,
                generation: cleanup.generation,
                success: false,
            },
            cx,
        );
        assert!(!s.import_admitted());
        assert!(s.retry_workflow_pending());
        s.retry_workflows(cx);
        s.ingest(
            Event::RuntimeReleased {
                runtime: cleanup.runtime,
                attempt: cleanup.attempt,
                generation: cleanup.generation,
                success: true,
            },
            cx,
        );
        assert!(s.import_admitted());
        s.dispatch(Command::Import(project().path), cx);
        let import_token = s.runtime_lifecycle.imports[&project().path];
        s.ingest(
            Event::ProjectImported {
                path: project().path,
                token: import_token,
                result: Ok(project()),
            },
            cx,
        );
        s.ingest(
            Event::RegisteredRuntimeRow {
                runtime: "a".into(),
                generation: 7,
                project: project().path,
                row: row("stale", "/stale.jsonl"),
            },
            cx,
        );
        assert!(
            s.workspace.as_ref().unwrap().projects[0]
                .sessions
                .is_empty()
        );
        assert!(
            !s.workspace.as_ref().unwrap().projects[0].collapsed,
            "old fold intent cannot survive reimport"
        );
        assert!(s.preferences.drafts.is_empty());
    });
}

#[test]
fn bootstrap_source_recovery_archives_only_typed_original_and_retries_without_duplicates() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "old", w, cx);
        reads(s, "old", "new", "/old.jsonl", cx, &requests);
        assert!(s.sessions["old"].controller.source_recovery().is_some());
        assert!(s.sessions["old"].ephemeral);
        assert_eq!(s.sessions["old"].source_id, "new");
        let (project, id, revision) = s
            .runtime_lifecycle
            .recoveries
            .keys()
            .next()
            .unwrap()
            .clone();
        s.ingest(
            Event::SourceArchived {
                project: project.clone(),
                id: id.clone(),
                path: "/old.jsonl".into(),
                token: revision,
                success: false,
            },
            cx,
        );
        s.source_recoveries(cx);
        assert_eq!(s.runtime_lifecycle.recoveries.len(), 1);
        s.retry_workflows(cx);
        s.ingest(
            Event::SourceArchived {
                project,
                id,
                path: "/old.jsonl".into(),
                token: revision,
                success: true,
            },
            cx,
        );
        assert!(s.sessions["old"].controller.source_recovery().is_none());
        assert!(s.workspace.as_ref().unwrap().projects[0].sessions[0].archived);
        assert_eq!(s.workspace.as_ref().unwrap().projects[0].sessions.len(), 1);
        assert_eq!(s.sessions["old"].controller.state().draft, "draft old");
        assert!(
            s.resumable_source("old").is_none(),
            "unsaved adopted identity must not reopen the stale path"
        );
        s.sessions
            .get_mut("old")
            .unwrap()
            .controller
            .transport_failed(7);
        s.retry_pi("old", cx);
        assert!(std::iter::from_fn(|| requests.try_recv().ok()).any(|command| matches!(command, Command::Start { runtime, source: None, .. } if runtime == "old")));
    });
}

#[test]
fn empty_provisional_successor_survives_navigation_and_reselection_without_restart() {
    with_shell(512, |s, w, cx, requests| {
        create(s, "a", "", w, cx);
        started(s, "a", 7, cx);
        reads(s, "a", "original", "/original.jsonl", cx, &requests);
        let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
        s.effects("a", effects, cx);
        reads(s, "a", "successor", "/successor.jsonl", cx, &requests);
        assert!(s.sessions["a"].controller.provisional_replacement());
        create(s, "b", "retained", w, cx);
        assert!(s.sessions.contains_key("a"));
        s.select("successor", w, cx);
        assert_eq!(s.active, "a");
        assert_eq!(s.sessions["a"].controller.generation(), 7);
        assert!(
            !std::iter::from_fn(|| requests.try_recv().ok())
                .any(|c| matches!(c, Command::Start { runtime, .. } if runtime == "a"))
        );
    });
}

#[test]
fn predecessor_registration_retry_remains_owned_after_current_identity_changes() {
    with_shell(512, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        let registration = Command::Register {
            runtime: "a".into(),
            generation: 7,
            project: project().path,
            id: "a".into(),
            path: "/a.jsonl".into(),
            name: "A".into(),
        };
        s.dispatch(registration.clone(), cx);
        let effects = s.sessions.get_mut("a").unwrap().controller.refresh();
        s.effects("a", effects, cx);
        reads(s, "a", "b", "/b.jsonl", cx, &requests);
        s.ingest(Event::RegistrationFailed(registration), cx);
        assert_eq!(s.register_retries.len(), 1);
        assert_eq!(s.sessions["a"].controller.state().identity.id, "b");
        s.retry_workflows(cx);
        s.ingest(
            Event::RegistrationStored {
                runtime: "a".into(),
                generation: 7,
                id: "a".into(),
                path: "/a.jsonl".into(),
            },
            cx,
        );
        assert!(s.sessions["a"].registration_pending.is_empty());
        assert!(s.register_retries.is_empty());
    });
}

#[test]
fn startup_cleanup_failure_blocks_reconnect_until_actual_ack() {
    with_shell(128, |s, w, cx, requests| {
        saved(s, "a", w, cx);
        reads(s, "a", "a", "/a.jsonl", cx, &requests);
        s.dispatch(
            Command::CancelStart {
                runtime: "a".into(),
                attempt: 1,
            },
            cx,
        );
        s.ingest(
            Event::StartCancelled {
                runtime: "a".into(),
                attempt: 1,
                success: false,
            },
            cx,
        );
        assert!(s.runtime_cleanup_pending("a"));
        s.retry_pi("a", cx);
        s.retry_workflows(cx);
        s.retry_workflows(cx);
        assert_eq!(
            std::iter::from_fn(|| requests.try_recv().ok())
                .filter(|c| matches!(c, Command::CancelStart { .. }))
                .count(),
            2
        );
        s.ingest(
            Event::StartCancelled {
                runtime: "a".into(),
                attempt: 1,
                success: true,
            },
            cx,
        );
        assert!(!s.runtime_cleanup_pending("a"));
    });
}

#[test]
fn acknowledged_remote_release_clears_feedback_without_reconnect_or_modal_palette_ownership() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(512);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(700.)));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.mount(w, cx);
            saved(s, "remote", w, cx);
            reads(s, "remote", "remote", "/remote.jsonl", cx, &requests);
            s.sessions
                .get_mut("remote")
                .unwrap()
                .project
                .connection_string = Some("fixture".into());
            s.active = "other".into();
            let live = &s.sessions["remote"];
            let token = RuntimeToken {
                runtime: "remote".into(),
                attempt: live.start_attempt,
                generation: live.controller.generation(),
            };
            s.runtime_lifecycle.releases.insert(
                token.clone(),
                Release {
                    reason: ReleaseReason::Idle(live.controller.state().identity.clone()),
                    failed: false,
                },
            );
            s.ingest(
                Event::RuntimeReleased {
                    runtime: token.runtime,
                    attempt: token.attempt,
                    generation: token.generation,
                    success: true,
                },
                cx,
            );
            s.select("remote", w, cx);
            assert!(!s.sessions["remote"].starting);
            assert_eq!(
                s.sessions["remote"].controller.state().status,
                Status::Error
            );
            assert!(s.fallback_is_open(cx));
            assert!(s.sessions["remote"].controller.state().error.is_some());
            assert!(!s.sessions["remote"].controller.state().ready);
        })
    });
    root.activate_accessibility();
    root.draw();
    let message = "This remote Pi runtime was released while idle. Reconnect to reopen saved history; in-memory extension state was not retained.";
    assert!(
        root.accessibility_tree()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, n)| {
                n.value()
                    .or(n.label())
                    .is_some_and(|text| text.contains(message))
            })
    );
    let close = root.update(|_, w, _| w.find("close").bounds().center());
    root.simulate_click(close, MouseButton::Left);
    root.draw();
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            // Cancel acknowledges the displayed incident, not the disconnected runtime.
            assert!(s.fallback.is_none());
            assert!(s.sessions["remote"].controller.state().error.is_none());
            assert!(!s.sessions["remote"].controller.state().ready);
            assert!(!s.sessions["remote"].starting);
            assert!(!s.fallback_is_open(cx));
            s.toggle_palette(w, cx);
            assert!(s.palette.is_open(cx));
        })
    });
    assert!(
        !std::iter::from_fn(|| requests.try_recv().ok())
            .any(|c| matches!(c, Command::Start { .. }))
    );
}
