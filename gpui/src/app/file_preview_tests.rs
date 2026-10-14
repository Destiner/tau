use super::*;
use gpui_kit::{
    Focusable, MouseButton, TestApp, TestAppWindow, base::Root, px, test::TestWindowExt,
};

fn shell_fixture() -> (TestApp, TestAppWindow<Root>, Entity<Shell>) {
    let mut app = super::super::tests::app();
    let (sender, _) = async_channel::bounded(64);
    let shell = app.new_entity(|_| {
        let mut shell = super::super::tests::shell(sender);
        shell.workspace = Some(super::super::WorkspaceSnapshot {
            projects: vec![super::super::tests::project()],
            active_project_path: super::super::tests::project().path,
            pi_path: None,
        });
        shell
    });
    let mut window = app.open_window(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.window = Some(window.window_handle());
            shell.mount(window, cx);
            shell.create_retained(
                "preview-fixture".into(),
                super::super::tests::project(),
                String::new(),
                String::new(),
                "untouched draft".into(),
                window,
                cx,
            );
            shell.active = "preview-fixture".into();
            shell.sessions["preview-fixture"]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        });
        Root::new(shell.clone(), window, cx)
    });
    window.update(|_, _, cx| {
        shell.update(cx, |shell, _| {
            shell
                .sessions
                .get_mut("preview-fixture")
                .unwrap()
                .controller
                .begin_generation(1);
        });
    });
    (app, window, shell)
}

fn fixture_snapshot(directory: &std::path::Path, filename: &str, text: &str) -> Arc<Snapshot> {
    std::fs::write(directory.join(filename), text).unwrap();
    let PreparedPreview::Ready(snapshot) = PreviewService::default()
        .prepare(None, directory.to_str().unwrap(), filename)
        .unwrap()
    else {
        panic!("snapshot")
    };
    snapshot
}

#[test]
fn shell_root_delayed_prepare_loading_deadline_actual_close_and_stale_body() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = fixture_snapshot(directory.path(), "README.md", "# Synthetic only\n");
    let path = snapshot.path.clone();
    let (prepared, preparations) = async_channel::unbounded();
    let (body, bodies) = async_channel::unbounded();
    let (app, mut window, shell) = shell_fixture();
    let request = window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.preview.prepare_receiver = Some(preparations);
            shell.preview.body_receiver = Some(bodies);
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::DocumentPath("README.md".into()),
                window.focused(cx),
                window,
                cx,
            );
            shell.preview.owner.pending.clone().unwrap()
        })
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(199));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview.feedback, None));
    app.advance_clock(Duration::from_millis(1));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview.feedback, Some("Opening…")));
    prepared
        .try_send(Ok(PreparedPreview::Ready(snapshot.clone())))
        .unwrap();
    app.run_until_parked();
    window.draw();
    window.update(|_, _, cx| {
        let shell = shell.read(cx);
        assert!(shell.preview_open());
        assert!(matches!(
            shell.preview_view().unwrap().read(cx).content(),
            FileContent::Loading
        ));
        assert_eq!(
            shell.sessions["preview-fixture"].shown_draft,
            "untouched draft"
        );
    });
    app.advance_clock(Duration::from_millis(199));
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert!(
            !shell
                .read(cx)
                .preview_view()
                .unwrap()
                .read(cx)
                .loading_visible()
        )
    });
    app.advance_clock(Duration::from_millis(1));
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert!(
            shell
                .read(cx)
                .preview_view()
                .unwrap()
                .read(cx)
                .loading_visible()
        )
    });
    app.advance_clock(Duration::from_millis(14_800));
    app.run_until_parked();
    window.draw();
    window.update(|_, _, cx| {
        let shell = shell.read(cx);
        assert!(!shell.preview.owner.pending());
        assert!(matches!(
            shell.preview_view().unwrap().read(cx).content(),
            FileContent::Error(_)
        ));
    });
    body.try_send(load_snapshot(&request, snapshot.clone()))
        .unwrap();
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert!(matches!(
            shell.read(cx).preview_view().unwrap().read(cx).content(),
            FileContent::Error(_)
        ))
    });
    let close = window.update(|_, window, _| window.find("close").bounds().center());
    window.simulate_click(close, MouseButton::Left);
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert!(!shell.read(cx).preview_open());
        assert_eq!(
            shell.read(cx).sessions["preview-fixture"].shown_draft,
            "untouched draft"
        );
    });
    drop(snapshot);
    assert!(!path.exists());
}

#[test]
fn shell_root_full_viewport_blocks_chrome_and_retires_stale_body() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = fixture_snapshot(directory.path(), "README.md", "# Late body\n");
    let (body, bodies) = async_channel::unbounded();
    let (app, mut window, shell) = shell_fixture();
    let (commands, _requests) = async_channel::bounded(256);
    window.update(|_, _, cx| shell.update(cx, |shell, _| shell.commands = commands));
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.create_retained(
                "b".into(),
                super::super::tests::project(),
                String::new(),
                String::new(),
                "  retained B 🦀\nexact  ".into(),
                window,
                cx,
            );
            shell.sync(window, cx);
        });
    });
    window.draw();
    let blocked = window.update(|_, w, _| {
        [
            "session-status-b",
            "session-name",
            "version",
            "sidebar-resize-handle",
        ]
        .map(|id| w.find(id).bounds().center())
    });
    let (origin, before, request) = window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            let origin = window.focused(cx);
            let before = (
                window.find("sidebar").bounds(),
                window.find("header").bounds(),
            );
            let ready = snapshot.clone();
            shell.preview.prepare_worker =
                Some(Arc::new(move |_| Ok(PreparedPreview::Ready(ready.clone()))));
            shell.preview.body_receiver = Some(bodies);
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::DocumentPath("README.md".into()),
                origin.clone(),
                window,
                cx,
            );
            (origin, before, shell.preview.owner.pending.clone().unwrap())
        })
    });
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(window.find("sidebar").bounds(), before.0);
        assert_eq!(window.find("header").bounds(), before.1);
        let viewport = window.find("shell-viewport").bounds();
        let overlay = window.find("shell-file-overlay").bounds();
        assert_eq!(overlay, viewport);
        assert_eq!(overlay.size, window.viewport_size());
        assert_eq!(window.find("content").bounds().bottom(), overlay.bottom());
        assert_eq!(
            window.find("close").bounds().right(),
            overlay.right() - px(12.)
        );
        assert!(shell.read(cx).preview_open());
    });
    // These physical coordinates belonged to sidebar/titlebar controls before opening.
    for at in blocked {
        window.simulate_click(at, MouseButton::Left);
        app.run_until_parked();
        window.draw();
        window.update(|_, _, cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.active, "preview-fixture");
            assert!(shell.preview_open());
            assert!(!shell.header.as_ref().unwrap().read(cx).renaming());
            assert!(!shell.sidebar.as_ref().unwrap().read(cx).version_open());
        });
    }
    window.simulate_resize(gpui_kit::size(px(1180.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    window.update(|_, window, _| {
        assert_eq!(
            window.find("shell-file-overlay").bounds(),
            window.find("shell-viewport").bounds()
        );
        assert_eq!(
            window.find("shell-file-overlay").bounds().size,
            window.viewport_size()
        );
    });
    window.simulate_keystroke("escape");
    app.run_until_parked();
    for _ in 0..4 {
        window.update(|_, w, cx| w.simulate_next_frame(cx));
        window.draw();
    }
    window.update(|_, w, cx| {
        assert!(!shell.read(cx).preview_open());
        assert_eq!(w.focused(cx), origin);
    });
    let row = window.update(|_, w, _| w.find("session-status-b").bounds().center());
    window.simulate_click(row, MouseButton::Left);
    app.run_until_parked();
    window.draw();
    body.try_send(load_snapshot(&request, snapshot)).unwrap();
    app.run_until_parked();
    window.draw();
    let row = window.update(|_, w, _| w.find("session-status-preview-fixture").bounds().center());
    window.simulate_click(row, MouseButton::Left);
    app.run_until_parked();
    window.draw();
    window.update(|_, w, cx| {
        let shell = shell.read(cx);
        assert!(!shell.preview_open());
        assert!(w.try_find("shell-file-overlay").is_none());
        assert_eq!(shell.active, "preview-fixture");
        assert_eq!(
            shell.sessions["preview-fixture"].controller.state().draft,
            "untouched draft"
        );
        assert_eq!(
            shell.sessions["b"].controller.state().draft,
            "  retained B 🦀\nexact  "
        );
        let saved = shell.pending_saves.last_key_value().unwrap().1;
        assert!(
            saved
                .drafts
                .iter()
                .any(|draft| draft.key == "preview-fixture" && draft.text == "untouched draft")
        );
        assert!(
            saved
                .drafts
                .iter()
                .any(|draft| draft.key == "b" && draft.text == "  retained B 🦀\nexact  ")
        );
    });
}

#[test]
fn shell_root_actual_document_relative_click_replaces_without_changing_return_origin_or_draft() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("README.md"),
        "[Next](next%20file.md)\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("next file.md"), "# Next\n").unwrap();
    let base = directory.path().to_str().unwrap().to_owned();
    let (app, mut window, shell) = shell_fixture();
    let origin = window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .sessions
                .get_mut("preview-fixture")
                .unwrap()
                .project
                .working_directory = base.clone();
            shell.preview.prepare_worker = Some(Arc::new(|request| {
                PreviewService::default().prepare(
                    None,
                    &request.context.base_path,
                    &request.resolved_path,
                )
            }));
            let origin = window.focused(cx);
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::WrittenPath("README.md".into()),
                origin.clone(),
                window,
                cx,
            );
            origin
        })
    });
    app.run_until_parked();
    window.draw();
    let (at, old_path) = window.update(|_, _, cx| {
        let shell = shell.read(cx);
        let viewer = shell.preview_view().unwrap();
        assert_eq!(viewer.read(cx).filename(), "README.md");
        let source = viewer.read(cx).selection_source_for_test(cx).unwrap();
        (
            actual_source_point(&source, cx),
            shell.preview.snapshot.as_ref().unwrap().path.clone(),
        )
    });
    window.simulate_click(at, MouseButton::Left);
    app.run_until_parked();
    window.draw();
    window.update(|_, _, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.preview_view().unwrap().read(cx).filename(),
            "next file.md"
        );
        assert_eq!(shell.preview.origin, origin);
        assert_eq!(
            shell.sessions["preview-fixture"].shown_draft,
            "untouched draft"
        );
        assert!(!old_path.exists());
    });
    let close = window.update(|_, window, _| window.find("close").bounds().center());
    window.simulate_click(close, MouseButton::Left);
    app.run_until_parked();
    for _ in 0..2 {
        window.update(|_, window, cx| window.simulate_next_frame(cx));
        window.draw();
        app.run_until_parked();
    }
    window.update(|_, window, cx| {
        assert!(!shell.read(cx).preview_open());
        assert_eq!(window.focused(cx), origin);
    });
}

#[test]
fn shell_prepare_deadline_discards_late_snapshot_and_retains_draft() {
    let directory = tempfile::tempdir().unwrap();
    let snapshot = fixture_snapshot(directory.path(), "late.txt", "synthetic");
    let path = snapshot.path.clone();
    let (sender, receiver) = async_channel::unbounded();
    let (app, mut window, shell) = shell_fixture();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.preview.prepare_receiver = Some(receiver);
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::WrittenPath("late.txt".into()),
                None,
                window,
                cx,
            );
        })
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(30));
    app.run_until_parked();
    window.update(|_, _, cx| {
        let shell = shell.read(cx);
        assert!(!shell.preview.owner.pending());
        assert_eq!(shell.preview_feedback(), Some("Preview unavailable"));
        assert!(!shell.preview_open());
    });
    sender
        .try_send(Ok(PreparedPreview::Ready(snapshot)))
        .unwrap();
    app.run_until_parked();
    assert!(!path.exists());
    app.advance_clock(Duration::from_millis(1_200));
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert_eq!(shell.read(cx).preview_feedback(), None);
        assert_eq!(
            shell.read(cx).sessions["preview-fixture"].shown_draft,
            "untouched draft"
        );
    });
}

#[test]
fn shell_context_changes_and_new_modal_fence_prepare_without_focus_steal() {
    for change in 0..8 {
        let (app, mut window, shell) = shell_fixture();
        let (sender, receiver) = async_channel::unbounded();
        window.update(|_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.preview.prepare_receiver = Some(receiver);
                shell.open_preview_link(
                    "preview-fixture",
                    LinkTarget::WrittenPath("README.md".into()),
                    None,
                    window,
                    cx,
                );
                let live = shell.sessions.get_mut("preview-fixture").unwrap();
                match change {
                    0 => shell.active = "another-selection".into(),
                    1 => {
                        live.controller.begin_generation(42);
                    }
                    2 => live.source_id = "replacement-source".into(),
                    3 => live.source_path = "replacement-path".into(),
                    4 => live.project.working_directory = "/synthetic/replacement".into(),
                    5 => shell.error = Some("New modal".into()),
                    6 => live
                        .controller
                        .bind_source("replacement-identity", "/synthetic/identity"),
                    _ => live.project.connection_string = Some("synthetic-remote".into()),
                }
                let focus = window.focused(cx);
                shell.sync_preview(cx);
                assert_eq!(window.focused(cx), focus);
                if change != 5 {
                    assert!(!shell.preview.owner.pending());
                }
            })
        });
        sender.try_send(Ok(PreparedPreview::Busy)).unwrap();
        app.run_until_parked();
        window.update(|_, _, cx| assert!(!shell.read(cx).preview_open()));
    }
}

fn context(session: &str) -> PreviewContext {
    PreviewContext {
        session: session.into(),
        source: "message-1".into(),
        base_path: "/work/tau".into(),
        remote_project: None,
        document_source_path: None,
        project_root: Some("/work/tau".into()),
    }
}
#[test]
fn pending_duplicate_close_source_and_session_fences() {
    let mut owner = PreviewOwner::default();
    let old = owner.begin(context("first"), "src/App.vue".into()).unwrap();
    assert!(
        owner
            .begin(context("first"), "src/App.vue".into())
            .is_none()
    );
    owner.invalidate();
    let current = owner
        .begin(context("second"), "docs/README.md".into())
        .unwrap();
    assert!(!owner.is_current(&old));
    assert!(owner.complete(&old, PreviewResult::Unavailable).is_none());
    assert!(owner.is_current(&current));
    assert!(matches!(
        owner.complete(&current, PreviewResult::Busy),
        Some(PreviewResult::Busy)
    ));
    assert!(!owner.pending());
    let reopened = owner
        .begin(context("second"), "docs/README.md".into())
        .unwrap();
    assert_ne!(reopened.generation, current.generation);
}
#[test]
fn document_replacement_resolves_against_source_not_project_or_local_remote_home() {
    let mut owner = PreviewOwner::default();
    let mut remote = context("remote");
    remote.base_path = "~/project/docs".into();
    remote.project_root = Some("~/project".into());
    remote.document_source_path = Some("~/project/docs/README.md".into());
    remote.remote_project = Some("ssh:[agent,project]".into());
    let request = owner
        .begin(remote.clone(), "../images/My File.png".into())
        .unwrap();
    assert_eq!(request.resolved_path, "~/project/images/My File.png");
    assert_eq!(directory_copy_path(&request), Some("../images/My File.png"));
    assert!(!can_copy_full_path("~/notes.md", &remote));
    assert!(can_copy_full_path("docs/guide.md", &remote));
}
#[test]
fn stale_snapshot_drops_without_releasing_the_new_preview() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("README"), "# Exact\n").unwrap();
    let mut owner = PreviewOwner::default();
    let mut ctx = context("one");
    ctx.base_path = dir.path().to_string_lossy().into_owned();
    let request = owner.begin(ctx.clone(), "README".into()).unwrap();
    let result = load(&PreviewService::default(), &request);
    let path = match &result {
        PreviewResult::Ready(p) => p.snapshot.path.clone(),
        _ => panic!("ready"),
    };
    owner.invalidate();
    let new = owner.begin(ctx, "README".into()).unwrap();
    assert!(owner.complete(&request, result).is_none());
    assert!(!path.exists());
    owner.complete(&new, load(&PreviewService::default(), &new));
    let active = owner.active().unwrap();
    assert!(matches!(&active.content, FileContent::Text(text) if text == "# Exact\n"));
    assert!(active.snapshot.path.exists());
}

#[test]
fn copy_full_path_preserves_posix_backslashes_except_in_document_context() {
    for remote in [false, true] {
        let mut ctx = context("paths");
        ctx.base_path = "/work/project\\root".into();
        ctx.remote_project = remote.then(|| "synthetic-remote".into());
        for (written, expected) in [
            ("a\\b.txt", "/work/project\\root/a\\b.txt"),
            ("./a%5Cb.txt", "/work/project\\root/a%5Cb.txt"),
            ("/absolute\\root/a\\b.txt", "/absolute\\root/a\\b.txt"),
            ("src/../a%20b.txt", "/work/project\\root/a%20b.txt"),
        ] {
            assert_eq!(full_copy_path(written, &ctx).as_deref(), Some(expected));
        }
        let LinkTarget::DocumentPath(decoded) = LinkTarget::explicit("a%5Cb.txt").unwrap() else {
            panic!("explicit file destination")
        };
        assert_eq!(decoded, "a\\b.txt");
        assert_eq!(
            full_copy_path(&decoded, &ctx).as_deref(),
            Some("/work/project\\root/a\\b.txt")
        );
        ctx.document_source_path = Some("/work/project\\root/docs/README.md".into());
        assert_eq!(
            full_copy_path(&decoded, &ctx).as_deref(),
            Some("/work/project/root/docs/a/b.txt")
        );
    }
    let ctx = context("home");
    let home = tau_core::file_preview::resolve_local_path("", "~/").unwrap();
    assert_eq!(
        full_copy_path("~/a\\b%20.txt", &ctx),
        Some(
            crate::components::markdown::file_reference::resolve_file_path(
                "",
                "~/a\\b%20.txt",
                Some(&home.to_string_lossy()),
            )
        ),
    );
}

#[test]
fn copy_full_path_keeps_written_percent_spelling_and_remote_home_is_never_local() {
    let local = context("local");
    assert_eq!(
        full_copy_path("src/a%20b.rs", &local).as_deref(),
        Some("/work/tau/src/a%20b.rs")
    );
    assert_eq!(
        full_copy_path("src/a b.rs", &local).as_deref(),
        Some("/work/tau/src/a b.rs")
    );
    assert!(full_copy_path("bad\nname", &local).is_none());
    let mut remote = context("remote");
    remote.remote_project = Some("synthetic-remote".into());
    assert_eq!(
        full_copy_path("../a%20b.rs", &remote).as_deref(),
        Some("/work/a%20b.rs")
    );
    assert!(full_copy_path("~/a.rs", &remote).is_none());
    remote.document_source_path = Some("/remote/project/docs/README.md".into());
    assert_eq!(
        full_copy_path("../a%20b.md", &remote).as_deref(),
        Some("/remote/project/a%20b.md")
    );
    remote.base_path = "~/remote-project".into();
    remote.document_source_path = None;
    assert!(full_copy_path("a.rs", &remote).is_none());
}

#[test]
fn source_owned_notify_context_and_current_question_do_not_admit_background_links() {
    let (app, mut window, shell) = shell_fixture();
    let (_, receiver) = async_channel::unbounded();
    window.update(|_, window, cx| shell.update(cx, |shell, cx| {
        let live = shell.sessions.get_mut("preview-fixture").unwrap();
        live.controller.handle(serde_json::json!({"type":"extension_ui_request", "id":"notice", "method":"notify", "message":"src/a.rs", "notifyType":"warning"}));
        let notice = live.controller.state().transcript.last().unwrap().clone();
        assert_eq!(notice_file_base(&notice, "/source-owned"), Some("/source-owned"));
        let mut history_notice = notice.clone(); history_notice.source = "history:custom".into();
        assert_eq!(notice_file_base(&history_notice, "/source-owned"), None);
        history_notice.arguments = serde_json::json!({"basePath":"/extension-origin"});
        assert_eq!(notice_file_base(&history_notice, "/source-owned"), Some("/extension-origin"));
        live.controller.handle(serde_json::json!({"type":"extension_ui_request", "id":"question", "method":"input", "title":"src/a.rs", "message":"Read file?"}));
        live.dirty = true;
        shell.sync(window, cx);
        let dialog = shell.sessions["preview-fixture"].extension.as_ref().unwrap().1.clone();
        shell.preview.prepare_receiver = Some(receiver);
        shell.open_source_link("preview-fixture", LinkSource::Project, LinkTarget::WrittenPath("background.rs".into()), window.focused(cx), window.mouse_position(), window, cx);
        assert!(!shell.preview.owner.pending());
        let source = LinkSource::Dialog { id: "question".into(), entity: dialog.clone() };
        shell.open_source_link("preview-fixture", source.clone(), LinkTarget::WrittenPath("question.rs".into()), window.focused(cx), window.mouse_position(), window, cx);
        assert!(shell.preview.owner.pending());
        assert_eq!(shell.sessions["preview-fixture"].shown_draft, "untouched draft");
        shell.close_preview(window, cx);
        shell.sessions.get_mut("preview-fixture").unwrap().controller.begin_generation(66);
        shell.sync(window, cx);
        assert!(!shell.link_source_current("preview-fixture", &source, cx));
    }));
    app.run_until_parked();
}

#[test]
fn viewer_context_never_activates_and_escape_cannot_close_the_document_under_its_menu() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("docs")).unwrap();
    let snapshot = fixture_snapshot(
        directory.path(),
        "docs/README.md",
        "[Related](nested/a%2520b.rs)\n",
    );
    let (app, mut window, shell) = shell_fixture();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .sessions
                .get_mut("preview-fixture")
                .unwrap()
                .project
                .working_directory = directory.path().to_string_lossy().into_owned();
            let snapshot = snapshot.clone();
            shell.preview.prepare_worker = Some(Arc::new(move |_| {
                Ok(PreparedPreview::Ready(snapshot.clone()))
            }));
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::WrittenPath("docs/README.md".into()),
                window.focused(cx),
                window,
                cx,
            );
        })
    });
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    window.simulate_keystroke("cmd-a");
    window.draw();
    let (viewer, at, focus, selected) = window.update(|_, window, cx| {
        let viewer = shell.read(cx).preview_view().unwrap();
        let source = viewer.read(cx).selection_source_for_test(cx).unwrap();
        (
            viewer.clone(),
            actual_source_point(&source, cx),
            window.focused(cx),
            viewer.read(cx).selected_text(cx),
        )
    });
    window.simulate_mouse_down(at, MouseButton::Right);
    window.simulate_mouse_up(at, MouseButton::Right);
    app.run_until_parked();
    window.draw();
    let token = window.update(|_, window, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.preview_view(), Some(viewer.clone()));
        assert!(!shell.preview.owner.pending());
        assert!(shell.preview_menu_open(cx));
        assert_ne!(window.focused(cx), focus);
        assert_eq!(viewer.read(cx).selected_text(cx), selected);
        let (token, capture) = shell.preview.menu.as_ref().unwrap();
        assert_eq!(capture.origin, focus);
        assert_eq!(
            capture.target,
            LinkTarget::DocumentPath("nested/a%20b.rs".into())
        );
        assert_eq!(
            capture.context.document_source_path.as_deref(),
            Some(snapshot.source_path.as_str())
        );
        *token
    });
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.dark = true;
            shell.sync_preview_theme(window, cx);
            assert_eq!(shell.preview_view(), Some(viewer.clone()));
            assert_eq!(viewer.read(cx).selected_text(cx), selected);
            assert!(shell.preview_menu_open(cx));
        })
    });
    window.simulate_keystroke("down");
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    app.run_until_parked();
    window.draw();
    window.update(|_, _, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            format!("{}/docs/nested/a%20b.rs", directory.path().display())
        )
    });
    window.simulate_mouse_down(at, MouseButton::Right);
    window.simulate_mouse_up(at, MouseButton::Right);
    app.run_until_parked();
    window.draw();
    window.simulate_keystroke("escape");
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(shell.read(cx).preview_view(), Some(viewer.clone()));
        assert!(!shell.read(cx).preview_menu_open(cx));
        assert!(shell.read(cx).preview.menu.is_none());
        assert_eq!(window.focused(cx), focus);
    });
    window.simulate_keystroke("escape");
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            assert!(!shell.preview_open());
            cx.write_to_clipboard(ClipboardItem::new_string("unchanged\n".to_owned()));
            shell.copy_link(&CopyLink { token, full: false }, window, cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "unchanged\n"
            );
        })
    });
}

fn production_link_fixture() -> (TestApp, TestAppWindow<Root>, Entity<Shell>) {
    let (app, mut window, shell) = shell_fixture();
    window.update(|_, window, cx| shell.update(cx, |shell, cx| {
        let live = shell.sessions.get_mut("preview-fixture").unwrap();
        live.controller.handle(serde_json::json!({"type":"message_start", "message":{"role":"assistant", "timestamp":1, "content":[]}}));
        live.controller.handle(serde_json::json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0, "delta":"src/a.rs"}}));
        live.dirty = true;
        live.projection_dirty = true;
        shell.sync(window, cx);
    }));
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    (app, window, shell)
}

fn production_context_point(
    window: &mut TestAppWindow<Root>,
    shell: &Entity<Shell>,
) -> Point<Pixels> {
    let at = window.read(|_, cx| {
        let transcript = &shell.read(cx).sessions["preview-fixture"].transcript;
        let source = transcript.read(cx).prose_source_for_test(0).unwrap();
        actual_source_point(&source, cx)
    });
    window.simulate_mouse_down(at, MouseButton::Right);
    window.simulate_mouse_up(at, MouseButton::Right);
    at
}

#[test]
fn production_transcript_root_context_full_copy_and_genuine_activation_delayed_feedback() {
    let (app, mut window, shell) = production_link_fixture();
    let focus = window.update(|_, window, cx| window.focused(cx));
    let anchor = production_context_point(&mut window, &shell);
    let _token = window.update(|_, _, cx| {
        let shell = shell.read(cx);
        assert!(shell.preview_menu_open(cx));
        assert!(!shell.preview.owner.pending());
        let (token, capture) = shell.preview.menu.as_ref().unwrap();
        assert_eq!(capture.origin, focus);
        assert!(matches!(capture.source, LinkSource::Transcript { .. }));
        assert_eq!(capture.target, LinkTarget::WrittenPath("src/a.rs".into()));
        assert_eq!(capture.anchor, anchor);
        assert!(capture.full_allowed);
        *token
    });
    window.draw();
    window.simulate_keystroke("down");
    window.simulate_keystroke("down");
    window.simulate_keystroke("enter");
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .ends_with("/src/a.rs")
        );
        assert_eq!(window.focused(cx), focus);
        assert_eq!(shell.read(cx).preview_feedback(), Some("Copied"));
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
    });
    let (sender, receiver) = async_channel::unbounded();
    window.update(|_, _, cx| {
        shell.update(cx, |shell, _| {
            shell.preview.prepare_receiver = Some(receiver)
        })
    });
    window.simulate_click(anchor, MouseButton::Left);
    let elsewhere = window.update(|_, window, _| window.find("editor").bounds().center());
    window.simulate_mouse_move(elsewhere);
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(199));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview_feedback(), None));
    app.advance_clock(Duration::from_millis(1));
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(shell.read(cx).preview_feedback(), Some("Opening…"));
        assert_eq!(shell.read(cx).preview_anchor(), Some(anchor));
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
        assert!(shell.read(cx).error.is_none());
    });
    sender
        .try_send(Err("private fixture error".into()))
        .unwrap();
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(
            shell.read(cx).preview_feedback(),
            Some("Preview unavailable")
        );
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
        assert!(shell.read(cx).error.is_none());
    });
}

#[test]
fn production_root_context_revision_replacement_denies_delayed_menu_copy() {
    let (app, mut window, shell) = production_link_fixture();
    production_context_point(&mut window, &shell);
    let token = window.update(|_, _, cx| shell.read(cx).preview.menu.as_ref().unwrap().0);
    window.update(|_, _, cx| shell.update(cx, |shell, cx| {
        shell.sessions.get_mut("preview-fixture").unwrap().controller.handle(serde_json::json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0, "delta":" replaced"}}));
        cx.write_to_clipboard(ClipboardItem::new_string("sentinel\n".to_owned()));
    }));
    window.update(|_, window, cx| {
        window.dispatch_action(Box::new(CopyLink { token, full: true }), cx)
    });
    app.run_until_parked();
    window.update(|_, _, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "sentinel\n"
        )
    });
}

struct LinkRootHost {
    shell: Entity<Shell>,
    markdown: Entity<crate::components::markdown::MarkdownView>,
    handoff: FocusHandle,
}
impl gpui_kit::Render for LinkRootHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        use gpui_kit::{InteractiveElement, ParentElement, Styled, div};
        let shell = self.shell.clone();
        let handoff = self.handoff.clone();
        div()
            .size_full()
            .child(self.shell.clone())
            .child(
                div()
                    .absolute()
                    .left(px(400.))
                    .top(px(40.))
                    .w(px(240.))
                    .child(self.markdown.clone()),
            )
            .child(
                gpui_kit::base::Button::new("pointer-handoff")
                    .absolute()
                    .left(px(400.))
                    .top(px(300.))
                    .w(px(120.))
                    .h(px(30.))
                    .track_focus(&self.handoff)
                    .child("Other control")
                    .on_click(move |_, window, cx| handoff.focus(window, cx)),
            )
            .on_action(move |action: &CopyLink, window, cx| {
                shell.update(cx, |shell, cx| shell.copy_link(action, window, cx))
            })
    }
}
fn link_root_fixture(
    source: &str,
) -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Shell>,
    Entity<crate::components::markdown::MarkdownView>,
) {
    use crate::components::markdown::MarkdownView;
    let mut app = super::super::tests::app();
    let (sender, _) = async_channel::bounded(64);
    let shell = app.new_entity(|_| {
        let mut shell = super::super::tests::shell(sender);
        shell.workspace = Some(super::super::WorkspaceSnapshot {
            projects: vec![super::super::tests::project()],
            active_project_path: super::super::tests::project().path,
            pi_path: None,
        });
        shell
    });
    let mut markdown = None;
    let window = app.open_window(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.window = Some(window.window_handle());
            shell.mount(window, cx);
            shell.create_retained(
                "preview-fixture".into(),
                super::super::tests::project(),
                String::new(),
                String::new(),
                "untouched draft".into(),
                window,
                cx,
            );
            shell.active = "preview-fixture".into();
            shell.sessions["preview-fixture"]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        });
        let owner = shell.downgrade();
        let view = cx.new(|cx| {
            let mut view = MarkdownView::new(source, false, |_| {}, cx).natural_height();
            view.set_context_handler(
                move |target, event, window, cx| {
                    let _ = owner.update(cx, |shell, cx| {
                        shell.open_link_menu(
                            "preview-fixture",
                            LinkSource::Project,
                            target,
                            event.position,
                            window,
                            cx,
                        )
                    });
                },
                cx,
            );
            view
        });
        markdown = Some(view.clone());
        let host = cx.new(|cx| LinkRootHost {
            shell: shell.clone(),
            markdown: view,
            handoff: cx.focus_handle(),
        });
        Root::new(host, window, cx)
    });
    (app, window, shell, markdown.unwrap())
}
fn actual_text_point(
    view: &Entity<crate::components::markdown::MarkdownView>,
    cx: &App,
) -> Point<Pixels> {
    actual_source_point(view.read(cx).text(), cx)
}
fn actual_source_point(source: &Entity<gpui_kit::base::TextViewState>, cx: &App) -> Point<Pixels> {
    let text = source.read(cx);
    let bounds = text.bounds();
    for y in 0..f32::from(bounds.size.height).ceil() as usize {
        for x in 0..f32::from(bounds.size.width).ceil() as usize {
            let at = bounds.origin + gpui_kit::point(px(x as f32), px(y as f32));
            if text.rendered_offset_at(at) == Some(1) {
                return at;
            }
        }
    }
    panic!("shaped link text must have actual hit geometry")
}
#[test]
fn root_context_copies_typed_url_and_paths_without_activation_selection_or_focus_changes() {
    for (source, expected, control) in [
        ("src/a.rs:42", "src/a.rs", false),
        ("/tmp/a%20b.rs:42", "/tmp/a%20b.rs", false),
        ("[file](docs/a%2520b.md)", "docs/a%20b.md", false),
        ("[web](https://example.com)", "https://example.com", false),
        ("[web](https:example.com)", "https:example.com", false),
        ("[web](https:/example.com)", "https:/example.com", false),
        ("src/a.rs:42", "src/a.rs", true),
        ("[web](https://example.com)", "https://example.com", true),
    ] {
        let (app, mut window, shell, markdown) = link_root_fixture(source);
        app.run_until_parked();
        window.draw();
        app.run_until_parked();
        window.draw();
        window.update(|_, _, cx| {
            let text = markdown.read(cx).text().clone();
            text.update(cx, |text, cx| text.select_all(cx))
        });
        window.draw();
        let (at, selection, focus) = window.update(|_, window, cx| {
            (
                actual_text_point(&markdown, cx),
                markdown.read(cx).text().read(cx).selected_text(),
                window.focused(cx),
            )
        });
        let button = if control {
            MouseButton::Left
        } else {
            MouseButton::Right
        };
        let modifiers = gpui_kit::Modifiers {
            control,
            ..Default::default()
        };
        window.simulate_event(gpui_kit::MouseDownEvent {
            button,
            position: at,
            modifiers,
            click_count: 1,
            first_mouse: false,
        });
        window.simulate_event(gpui_kit::MouseUpEvent {
            button,
            position: at,
            modifiers,
            click_count: 1,
        });
        app.run_until_parked();
        window.draw();
        let _token = window.update(|_, _, cx| {
            let shell = shell.read(cx);
            assert!(!shell.preview.owner.pending());
            assert!(!shell.preview_open());
            assert!(shell.preview_menu_open(cx));
            let (token, capture) = shell
                .preview
                .menu
                .as_ref()
                .expect("real Root context must reach Shell");
            assert_eq!(capture.anchor, at);
            assert_eq!(capture.origin, focus);
            assert_eq!(markdown.read(cx).text().read(cx).selected_text(), selection);
            *token
        });
        window.simulate_keystroke("down");
        window.simulate_keystroke("enter");
        app.run_until_parked();
        window.draw();
        window.update(|_, window, cx| {
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected);
            assert_eq!(shell.read(cx).preview_feedback(), Some("Copied"));
            assert_eq!(shell.read(cx).preview_anchor(), Some(at));
            assert_eq!(window.focused(cx), focus);
            assert_eq!(markdown.read(cx).text().read(cx).selected_text(), selection);
            assert_eq!(markdown.read(cx).source(), source);
        });
    }
}
#[test]
fn root_full_copy_literal_posix_backslash_names_the_actual_file_and_preserves_source() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().join("project\\root");
    std::fs::create_dir(&base).unwrap();
    std::fs::write(base.join("a\\b.txt"), "literal backslash file").unwrap();
    std::fs::create_dir(base.join("a")).unwrap();
    std::fs::write(base.join("a/b.txt"), "wrong separator file").unwrap();
    for remote in [false, true] {
        for (source, path) in [
            ("[file](a%5Cb.txt)", "a\\b.txt"),
            ("[file](a%255Cb.txt)", "a%5Cb.txt"),
            ("/tmp/a%5Cb.txt", "/tmp/a%5Cb.txt"),
        ] {
            let (app, mut window, shell, markdown) = link_root_fixture(source);
            window.update(|_, _, cx| {
                shell.update(cx, |shell, _| {
                    let live = shell.sessions.get_mut("preview-fixture").unwrap();
                    live.project.working_directory = base.to_str().unwrap().into();
                    live.project.connection_string = remote.then(|| "synthetic-remote".into());
                })
            });
            app.run_until_parked();
            window.draw();
            app.run_until_parked();
            window.draw();
            let (at, focus) = window
                .update(|_, window, cx| (actual_text_point(&markdown, cx), window.focused(cx)));
            window.simulate_mouse_down(at, MouseButton::Right);
            window.simulate_mouse_up(at, MouseButton::Right);
            app.run_until_parked();
            window.draw();
            window.update(|_, _, cx| {
                let capture = &shell.read(cx).preview.menu.as_ref().unwrap().1;
                assert!(capture.full_allowed);
                assert_eq!(capture.context.document_source_path, None);
                assert_eq!(capture.context.base_path, base.to_str().unwrap());
                assert_eq!(
                    capture.target,
                    if source.starts_with('[') {
                        LinkTarget::DocumentPath(path.into())
                    } else {
                        LinkTarget::WrittenPath(path.into())
                    }
                );
            });
            window.simulate_keystroke("down");
            window.simulate_keystroke("down");
            window.simulate_keystroke("enter");
            app.run_until_parked();
            window.draw();
            window.update(|_, window, cx| {
                let expected = if path.starts_with('/') {
                    path.to_owned()
                } else {
                    format!("{}/{path}", base.display())
                };
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected);
                assert_eq!(markdown.read(cx).source(), source);
                assert_eq!(window.focused(cx), focus);
                assert!(!shell.read(cx).preview.owner.pending());
                if path == "a\\b.txt" {
                    assert_eq!(
                        std::fs::read_to_string(&expected).unwrap(),
                        "literal backslash file"
                    );
                    let PreparedPreview::Ready(snapshot) = PreviewService::default()
                        .prepare(None, base.to_str().unwrap(), path)
                        .unwrap()
                    else {
                        panic!("actual file preview")
                    };
                    assert_eq!(snapshot.source_path, expected);
                    assert_eq!(read_text(&snapshot).unwrap().0, "literal backslash file");
                }
            });
        }
    }
}

#[test]
fn native_link_copy_full_path_is_generation_fenced_and_preserves_raw_percent() {
    let (app, mut window, shell, markdown) = link_root_fixture("src/a%20b.rs");
    // Raw percent paths are not generated prose targets; use an explicit typed capture here.
    app.run_until_parked();
    window.draw();
    let anchor = window.update(|_, _, cx| markdown.read(cx).text().read(cx).bounds().origin);
    let token = window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_link_menu(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("src/a%20b.rs".into()),
                anchor,
                window,
                cx,
            );
            shell.preview.menu.as_ref().unwrap().0
        })
    });
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.copy_link(&CopyLink { token, full: true }, window, cx)
        })
    });
    window.update(|_, _, cx| {
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .ends_with("/src/a%20b.rs")
        )
    });
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_link_menu(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("src/a%20b.rs".into()),
                anchor,
                window,
                cx,
            );
            let token = shell.preview.menu.as_ref().unwrap().0;
            shell
                .sessions
                .get_mut("preview-fixture")
                .unwrap()
                .controller
                .begin_generation(999);
            cx.write_to_clipboard(ClipboardItem::new_string("sentinel\n".to_owned()));
            shell.copy_link(&CopyLink { token, full: false }, window, cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "sentinel\n"
            );
        })
    });
}
#[test]
fn root_delayed_opening_and_error_feedback_use_the_captured_pointer_not_latest_pointer() {
    let (app, mut window, shell, markdown) = link_root_fixture("src/a.rs");
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    let anchor = window.update(|_, _, cx| actual_text_point(&markdown, cx));
    let (sender, receiver) = async_channel::unbounded();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.preview.prepare_receiver = Some(receiver);
            shell.open_source_link(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("a.rs".into()),
                window.focused(cx),
                anchor,
                window,
                cx,
            );
        })
    });
    window.simulate_mouse_move(gpui_kit::point(px(2.), px(2.)));
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(199));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview_feedback(), None));
    app.advance_clock(Duration::from_millis(1));
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(shell.read(cx).preview_feedback(), Some("Opening…"));
        assert_eq!(shell.read(cx).preview_anchor(), Some(anchor));
        let bounds = window.find("file-link-feedback").bounds();
        assert_eq!(bounds.left(), anchor.x);
        assert_eq!(bounds.top(), anchor.y + px(8.));
        assert!(shell.read(cx).error.is_none());
    });
    sender
        .try_send(Err("synthetic private error".into()))
        .unwrap();
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(
            shell.read(cx).preview_feedback(),
            Some("Preview unavailable")
        );
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
        assert!(shell.read(cx).error.is_none());
        assert!(!shell.read(cx).preview_open());
    });
    app.advance_clock(Duration::from_millis(1_200));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview_feedback(), None));
}

#[test]
fn remote_copy_only_never_resolves_on_local_home_and_owning_fallback_denies_links() {
    let (app, mut window, shell, markdown) = link_root_fixture("~/docs/a.rs");
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    let anchor = window.update(|_, _, cx| actual_text_point(&markdown, cx));
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            let live = shell.sessions.get_mut("preview-fixture").unwrap();
            live.project.connection_string = Some("synthetic-remote".into());
            live.project.path.clear();
            shell.open_source_link(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("~/docs/a.rs".into()),
                window.focused(cx),
                anchor,
                window,
                cx,
            );
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "~/docs/a.rs"
            );
            assert_eq!(shell.preview_feedback(), Some("Copied"));
            assert_eq!(shell.preview_anchor(), Some(anchor));
            assert!(!shell.preview.owner.pending());
            assert!(!shell.preview_open());
            shell.open_link_menu(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("~/docs/a.rs".into()),
                anchor,
                window,
                cx,
            );
            let token = shell.preview.menu.as_ref().unwrap().0;
            cx.write_to_clipboard(ClipboardItem::new_string("sentinel\n".to_owned()));
            shell.copy_link(&CopyLink { token, full: true }, window, cx);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "sentinel\n"
            );
            shell.error = Some("Synthetic owning failure".into());
            shell.sync(window, cx);
            assert!(
                shell
                    .fallback
                    .as_ref()
                    .is_some_and(|(_, dialog)| dialog.read(cx).is_open())
            );
            shell.error = None;
            let revision = shell.preview.menu_revision;
            let focus = window.focused(cx);
            shell.open_link_menu(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::Web("https://example.com".into()),
                anchor,
                window,
                cx,
            );
            shell.open_source_link(
                "preview-fixture",
                LinkSource::Project,
                LinkTarget::WrittenPath("a.rs".into()),
                focus.clone(),
                anchor,
                window,
                cx,
            );
            assert_eq!(shell.preview.menu_revision, revision);
            assert!(!shell.preview.owner.pending());
            assert_eq!(window.focused(cx), focus);
        })
    });
}

#[test]
fn root_menu_outside_pointer_hands_focus_to_the_clicked_control() {
    let (app, mut window, shell, markdown) = link_root_fixture("src/a.rs");
    app.run_until_parked();
    window.draw();
    let at = window.update(|_, _, cx| actual_text_point(&markdown, cx));
    window.simulate_mouse_down(at, MouseButton::Right);
    window.simulate_mouse_up(at, MouseButton::Right);
    app.run_until_parked();
    window.draw();
    let (token, origin, handoff) = window.update(|_, window, cx| {
        let shell = shell.read(cx);
        let (token, capture) = shell.preview.menu.as_ref().unwrap();
        assert!(shell.preview_menu_open(cx));
        (
            *token,
            capture.origin.clone(),
            window.find("pointer-handoff").bounds().center(),
        )
    });
    window.simulate_click(handoff, MouseButton::Left);
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert!(window.find("pointer-handoff").focused().unwrap());
        assert_ne!(window.focused(cx), origin);
        assert!(!shell.read(cx).preview_menu_open(cx));
        assert!(shell.read(cx).preview.menu.is_none());
        cx.write_to_clipboard(ClipboardItem::new_string("sentinel\n".to_owned()));
        shell.update(cx, |shell, cx| {
            shell.copy_link(&CopyLink { token, full: false }, window, cx)
        });
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "sentinel\n"
        );
    });
}

#[test]
fn root_keyboard_document_relative_activation_keeps_original_bounds_for_opening_and_error() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("docs")).unwrap();
    let snapshot = fixture_snapshot(
        directory.path(),
        "docs/README.md",
        "[Related](nested/My%20File.md)\n",
    );
    let (mut app, mut window, shell) = shell_fixture();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .sessions
                .get_mut("preview-fixture")
                .unwrap()
                .project
                .working_directory = directory.path().to_string_lossy().into_owned();
            let ready = snapshot.clone();
            shell.preview.prepare_worker =
                Some(Arc::new(move |_| Ok(PreparedPreview::Ready(ready.clone()))));
            shell.open_preview_link(
                "preview-fixture",
                LinkTarget::DocumentPath("docs/README.md".into()),
                window.focused(cx),
                window,
                cx,
            );
        })
    });
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    let viewer = window.update(|_, _, cx| shell.read(cx).preview_view().unwrap());
    let pointer = window.update(|_, window, _| window.find("close").bounds().center());
    window.simulate_mouse_move(pointer);
    let (sender, receiver) = async_channel::unbounded();
    window.update(|_, window, cx| {
        shell.update(cx, |shell, _| {
            shell.preview.prepare_receiver = Some(receiver)
        });
        assert!(viewer.read(cx).close_focused(window));
    });
    // Dispatch outside Root's entity update: TestWindowExt::press renders Root itself.
    let handle = app.windows()[0];
    app.update(|cx| {
        cx.update_window(handle, |_, window, cx| window.press("shift-tab", cx))
            .unwrap();
    });
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| assert!(!viewer.read(cx).close_focused(window)));
    app.update(|cx| {
        cx.update_window(handle, |_, window, cx| window.press("enter", cx))
            .unwrap();
    });
    app.run_until_parked();
    let anchor = window.update(|_, _, cx| {
        let shell = shell.read(cx);
        let request = shell
            .preview
            .owner
            .pending
            .as_ref()
            .expect("real keyboard activation must open the relative document");
        assert_eq!(request.written_path, "nested/My File.md");
        assert_eq!(
            request.resolved_path,
            directory
                .path()
                .join("docs/nested/My File.md")
                .to_string_lossy()
                .into_owned()
        );
        assert_eq!(
            request.context.document_source_path.as_deref(),
            Some(snapshot.source_path.as_str())
        );
        let anchor = shell.preview.request_anchor.unwrap();
        let bounds = viewer
            .read(cx)
            .selection_source_for_test(cx)
            .unwrap()
            .read(cx)
            .bounds();
        assert!(anchor.x >= bounds.left() && anchor.x <= bounds.right());
        assert!(anchor.y > bounds.top() && anchor.y <= bounds.bottom());
        assert_ne!(anchor, pointer);
        anchor
    });
    app.advance_clock(Duration::from_millis(199));
    app.run_until_parked();
    window.update(|_, _, cx| assert_eq!(shell.read(cx).preview_feedback(), None));
    app.advance_clock(Duration::from_millis(1));
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(shell.read(cx).preview_feedback(), Some("Opening…"));
        assert_eq!(shell.read(cx).preview_anchor(), Some(anchor));
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
    });
    sender
        .try_send(Err("private keyboard fixture failure".into()))
        .unwrap();
    app.run_until_parked();
    window.draw();
    window.update(|_, window, cx| {
        assert_eq!(
            shell.read(cx).preview_feedback(),
            Some("Preview unavailable")
        );
        assert_eq!(shell.read(cx).preview_anchor(), Some(anchor));
        assert_eq!(window.find("file-link-feedback").bounds().left(), anchor.x);
        assert_eq!(shell.read(cx).preview_view(), Some(viewer));
        assert!(shell.read(cx).error.is_none());
    });
}

#[test]
fn source_revision_and_switch_away_back_permanently_retire_captured_menu_actions() {
    let (app, mut window, shell) = shell_fixture();
    window.draw();
    window.update(|_, window, cx| shell.update(cx, |shell, cx| {
        let live = shell.sessions.get_mut("preview-fixture").unwrap();
        live.controller.handle(serde_json::json!({"type":"message_start", "message":{"role":"assistant", "timestamp":1, "content":[]}}));
        live.controller.handle(serde_json::json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0, "delta":"src/a.rs"}}));
        let entry = live.controller.state().transcript.last().unwrap();
        let source = LinkSource::Transcript { id: entry.id.clone(), revision: entry.revision };
        let anchor = window.mouse_position();
        shell.open_link_menu("preview-fixture", source.clone(), LinkTarget::WrittenPath("src/a.rs".into()), anchor, window, cx);
        let token = shell.preview.menu.as_ref().unwrap().0;
        shell.sessions.get_mut("preview-fixture").unwrap().controller.handle(serde_json::json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0, "delta":" changed"}}));
        cx.write_to_clipboard(ClipboardItem::new_string("exact sentinel\n".to_owned()));
        shell.copy_link(&CopyLink { token, full: false }, window, cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "exact sentinel\n");
        shell.open_link_menu("preview-fixture", LinkSource::Project, LinkTarget::Web("https://example.com".into()), anchor, window, cx);
        let token = shell.preview.menu.as_ref().unwrap().0;
        shell.active.clear(); shell.sync_preview(cx);
        shell.active = "preview-fixture".into(); shell.sync_preview(cx);
        shell.copy_link(&CopyLink { token, full: false }, window, cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "exact sentinel\n");
    }));
    app.run_until_parked();
}
