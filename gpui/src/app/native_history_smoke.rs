//! Synthetic Pi-owned history through the production worker and Root/Shell.
use super::*;
use gpui_kit::base::test_support::{ElementSnapshot, find, snapshots};
use gpui_kit::{ElementId, ScrollDelta, ScrollWheelEvent, point};
use serde_json::json;

const ID: &str = "d77466dc-cc71-4093-b1de-666124dab541";
const RAW_COMPACT: &str = "TAU_SYNTHETIC_COMPACTION_CONTEXT_ONLY";
const RAW_BRANCH: &str = "TAU_SYNTHETIC_BRANCH_CONTEXT_ONLY";
const PRIOR: &str = "Invented prior layer";
const TAIL: &str = "Invented current reader";

fn seed(root: &std::path::Path, project: &str) -> PathBuf {
    let directory = tau_core::storage::default_session_dir(project).unwrap();
    assert!(directory.starts_with(root.join("pi-sessions")));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("2026-01-01T00-00-00-000Z_{ID}.jsonl"));
    let timestamp = "2026-01-01T00:00:00.000Z";
    let records = [
        json!({"type":"session","version":3,"id":ID,"timestamp":timestamp,"cwd":project}),
        json!({"type":"message","id":"10000001","parentId":null,"timestamp":timestamp,"message":{"role":"user","content":PRIOR,"timestamp":1767225600001u64}}),
        json!({"type":"message","id":"10000002","parentId":"10000001","timestamp":timestamp,"message":{"role":"user","content":format!("{TAIL}\n\n{}", (0..32).map(|i| format!("Invented reader line {i}\n\n")).collect::<String>()),"timestamp":1767225600002u64}}),
        json!({"type":"compaction","id":"10000003","parentId":"10000002","timestamp":timestamp,"summary":format!("{RAW_COMPACT}\n/synthetic/context/alpha.rs\n/synthetic/context/beta.rs"),"firstKeptEntryId":"10000002","tokensBefore":1000}),
        json!({"type":"branch_summary","id":"10000004","parentId":"10000003","timestamp":timestamp,"fromId":"10000001","summary":format!("{RAW_BRANCH}\n/synthetic/branch/context.rs")}),
        json!({"type":"session_info","id":"10000005","parentId":"10000004","timestamp":timestamp,"name":"Invented compacted history"}),
    ];
    let bytes = records.iter().map(|r| format!("{r}\n")).collect::<String>();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn marker_count(root: &std::path::Path, marker: &str) -> usize {
    std::fs::read_to_string(root.join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == marker)
        .count()
}
fn named(node: &ElementSnapshot) -> Option<&str> {
    match node.path().last()? {
        ElementId::Name(name) => Some(name.as_ref()),
        _ => None,
    }
}
fn boundary(w: &Window) -> Option<ElementSnapshot> {
    snapshots(w).into_iter().find(|node| {
        node.visible()
            && node.label() == Some("Load earlier messages")
            && named(node).is_some_and(|id| id.starts_with("compaction-"))
    })
}
fn clean(live: &LiveSession) {
    for row in &live.controller.state().transcript {
        assert!(!row.text.contains(RAW_COMPACT) && !row.text.contains(RAW_BRANCH));
        assert!(!row.text.contains("/synthetic/") && row.kind != "branchSummary");
        if row.kind == "compaction" {
            assert!(row.text.is_empty());
        }
    }
}

pub(super) async fn exercise(
    shell: &Entity<Shell>,
    cx: &AsyncApp,
    root: &std::path::Path,
    first: &str,
) {
    let (project, generation) = cx.update(|cx| {
        let live = &shell.read(cx).sessions[first];
        (live.project.path.clone(), live.controller.generation())
    });
    // Leave the Sidebar naturally before registering an arrival; hover holds are intentional.
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let editor = find(w, &[], &"editor".into())
                    .filter(|node| node.visible())
                    .expect("painted editor for natural Sidebar pointer exit");
                let bounds = editor.bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                w.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: bounds.center(),
                        pressed_button: None,
                        modifiers: Default::default(),
                    }),
                    cx,
                );
            })
            .unwrap()
    });
    let path = seed(root, &project);
    // The real worker validates/persists the synthetic file and returns its row metadata.
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.dispatch(
                Command::Register {
                    runtime: first.into(),
                    generation,
                    project: project.clone(),
                    id: ID.into(),
                    path: path.to_string_lossy().into_owned(),
                    name: "Invented compacted history".into(),
                },
                cx,
            )
        })
    });
    poll(cx, "worker returned synthetic saved row", 5, |cx| {
        shell
            .read(cx)
            .workspace
            .as_ref()
            .unwrap()
            .projects
            .iter()
            .any(|p| {
                p.path == project
                    && p.sessions
                        .iter()
                        .any(|r| r.id == ID && r.path == path.to_string_lossy())
            })
    })
    .await;
    let messages_before = marker_count(root, "history:get_messages:fixture");
    let entries_before = marker_count(root, "history:get_entries:fixture");
    let started = Instant::now();
    click(shell, cx, &format!("session-{ID}")).await;
    let mut loading_seen = false;
    poll(cx, "installed Pi compacted get_messages hydration", 12, |cx| {
        window(shell, cx).update(cx, |_, w, cx| {
            let s = shell.read(cx);
            if s.active_session_loading()
                && let Some(label) = find(w, &[], &"session-loading-label".into()).filter(|n| n.visible()) {
                    assert!(s.loading_indicator_visible());
                    assert!(find(w, &[], &"session-loading".into()).is_some_and(|n| n.visible() && n.bounds().size.height > px(0.)));
                    assert!(label.bounds().size.height > px(0.));
                    if !loading_seen {
                        eprintln!("OBSERVED native-app synthetic cold loader actual label bounds={:?} elapsed_ms={}", label.bounds(), started.elapsed().as_millis());
                        if let Some(directory) = std::env::var_os("TAU_NATIVE_CAPTURE_DIR") {
                            w.render_to_image().expect("owned synthetic loader capture")
                                .save(PathBuf::from(directory).join("compacted-history-cold-loader.png"))
                                .expect("save synthetic loader PNG");
                        }
                    }
                    loading_seen = true;
            }
            let Some(live) = s.sessions.get(&s.active) else { return false; };
            if live.controller.state().identity.id != ID { return false; }
            clean(live);
            live.controller.state().ready && !live.controller.state().history_loading
                && live.controller.state().transcript.iter().any(|r| r.text.starts_with(TAIL))
                // Bootstrap reads messages, then the authoritative get_state schedules a refresh.
                && marker_count(root, "history:get_messages:fixture") == messages_before + 2
        }).unwrap()
    }).await;
    eprintln!(
        "PASS native-app synthetic cold history hydration elapsed_ms={} delayed_loader_seen={loading_seen}",
        started.elapsed().as_millis()
    );
    let runtime = cx.update(|cx| shell.read(cx).active.clone());
    assert_ne!(runtime, first);
    let tail = cx.update(|cx| {
        let live = &shell.read(cx).sessions[&runtime];
        assert_eq!(live.controller.state().transcript.len(), 2);
        assert_eq!(live.controller.state().transcript[0].kind, "compaction");
        assert_eq!(live.controller.state().transcript[1].kind, "user");
        assert!(
            !live
                .controller
                .state()
                .transcript
                .iter()
                .any(|r| r.text == PRIOR)
        );
        live.controller
            .state()
            .transcript
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    r.source.clone(),
                    r.kind.clone(),
                    r.text.clone(),
                )
            })
            .collect::<Vec<_>>()
    });
    // Scroll via the painted transcript slot; never force ListState or readiness.
    poll(
        cx,
        "reader scroll reaches painted compaction divider",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    if boundary(w).is_some() {
                        return true;
                    }
                    let Some(slot) = find(w, &[], &"shell-transcript-slot".into())
                        .filter(|node| node.visible())
                        .map(|node| node.bounds())
                    else {
                        return false;
                    };
                    w.dispatch_event(
                        PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position: slot.center(),
                            delta: ScrollDelta::Pixels(point(px(0.), slot.size.height * 0.5)),
                            ..Default::default()
                        }),
                        cx,
                    );
                    false
                })
                .unwrap()
        },
    )
    .await;
    capture(shell, cx, "compacted-history-before", false).await;
    let (control, rows, transcript_id) = cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let node = boundary(w).unwrap();
                let rows = snapshots(w)
                    .into_iter()
                    .filter(|n| {
                        n.visible() && named(n).is_some_and(|id| id.starts_with("transcript-row-"))
                    })
                    .map(|n| (named(&n).unwrap().to_owned(), n.bounds()))
                    .collect::<Vec<_>>();
                assert!(rows.len() >= 2);
                let t = &shell.read(cx).sessions[&runtime].transcript;
                assert!(!t.read(cx).is_following_tail());
                (named(&node).unwrap().to_owned(), rows, t.entity_id())
            })
            .unwrap()
    });
    let reveal_started = Instant::now();
    click(shell, cx, &control).await;
    poll(
        cx,
        "Load Earlier actual get_entries and prior layer",
        5,
        |cx| {
            let live = &shell.read(cx).sessions[&runtime];
            clean(live);
            marker_count(root, "history:get_entries:fixture") == entries_before + 1
                && live
                    .controller
                    .state()
                    .transcript
                    .iter()
                    .any(|r| r.text == PRIOR)
        },
    )
    .await;
    capture(shell, cx, "compacted-history-after", false).await;
    cx.update(|cx| {
        window(shell, cx)
            .update(cx, |_, w, cx| {
                let live = &shell.read(cx).sessions[&runtime];
                clean(live);
                assert_eq!(live.transcript.entity_id(), transcript_id);
                assert!(!live.transcript.read(cx).is_following_tail());
                let current = &live.controller.state().transcript;
                assert_eq!(current.len(), tail.len() + 1);
                for (row, before) in current[1..].iter().zip(&tail) {
                    assert_eq!(
                        (&row.id, &row.source, &row.kind, &row.text),
                        (&before.0, &before.1, &before.2, &before.3)
                    );
                }
                for (id, before) in &rows {
                    let after =
                        find(w, &[], &id.clone().into()).expect("retained painted current row");
                    assert!(after.visible());
                    assert!((f32::from(after.bounds().top() - before.top())).abs() <= 1.);
                    assert_eq!(after.bounds().size, before.size);
                }
            })
            .unwrap()
    });
    eprintln!(
        "PASS native-app compacted history real get_messages/get_entries + measured Load Earlier prior layer; stable tail identity/content/reader bounds elapsed_ms={}",
        reveal_started.elapsed().as_millis()
    );
    poll(
        cx,
        "revealed prior layer naturally scrolls into paint",
        5,
        |cx| {
            window(shell, cx)
                .update(cx, |_, w, cx| {
                    let prior_painted = snapshots(w).iter().any(|n| {
                        n.visible()
                            && named(n).is_some_and(|id| {
                                id.starts_with("transcript-row-")
                                    && !rows.iter().any(|(old, _)| old == id)
                            })
                    });
                    if prior_painted {
                        return true;
                    }
                    let slot = find(w, &[], &"shell-transcript-slot".into())
                        .unwrap()
                        .bounds();
                    w.dispatch_event(
                        PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position: slot.center(),
                            delta: ScrollDelta::Pixels(point(px(0.), slot.size.height * 0.5)),
                            ..Default::default()
                        }),
                        cx,
                    );
                    false
                })
                .unwrap()
        },
    )
    .await;
    capture(shell, cx, "compacted-history-prior-layer", false).await;
    // Unsaved retained drafts use their runtime key; saved rows use their Pi identity.
    let first_row = cx.update(|cx| {
        let identity = &shell.read(cx).sessions[first]
            .controller
            .state()
            .identity
            .id;
        format!(
            "session-status-{}",
            if identity.is_empty() {
                first
            } else {
                identity.as_str()
            }
        )
    });
    click(shell, cx, &first_row).await;
    poll(cx, "history returns retained first draft", 5, |cx| {
        let s = shell.read(cx);
        s.active == first
            && s.sessions[first].controller.state().draft == "Owned draft café 😀"
            && s.sessions[first]
                .composer
                .read(cx)
                .editor()
                .read(cx)
                .value()
                .as_ref()
                == "Owned draft café 😀"
    })
    .await;
    eprintln!("PASS native-app synthetic history sidebar return retains original draft");
}
