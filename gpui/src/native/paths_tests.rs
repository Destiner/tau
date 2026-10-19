use super::*;
use gpui_kit::{AppContext, Entity, IntoElement, TestAppContext, div};

struct Owner {
    chooser: PathChooser<Self>,
    outcomes: Vec<PathOutcome>,
    draft: String,
}
impl Render for Owner {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
fn owner(window: &Window, cx: &Context<Owner>) -> Owner {
    Owner {
        chooser: PathChooser::new(window, cx),
        outcomes: vec![],
        draft: "unsent draft".into(),
    }
}
fn open(cx: &mut TestAppContext) -> (WindowHandle<Owner>, Entity<Owner>) {
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |w, cx| cx.new(|cx| owner(w, cx)))
            .unwrap()
    });
    let root = handle.root(cx).unwrap();
    (handle, root)
}
fn start(
    target: WindowHandle<Owner>,
    operation: PathOperation,
    cx: &mut TestAppContext,
) -> PathStart {
    target
        .update(cx, |root, w, cx| {
            root.chooser
                .select(operation, w, cx, |root, outcome, _, _| {
                    root.outcomes.push(outcome)
                })
        })
        .unwrap()
}
fn outcomes(target: WindowHandle<Owner>, cx: &mut TestAppContext) -> Vec<PathOutcome> {
    target
        .read_with(cx, |root, _| {
            assert_eq!(root.draft, "unsent draft");
            root.outcomes.clone()
        })
        .unwrap()
}
fn save() -> PathOperation {
    PathOperation::SaveDestination {
        directory: PathBuf::from("/synthetic/destination"),
        suggested_name: Some("report.html".into()),
    }
}

#[gpui_kit::test]
fn project_options_duplicates_and_owning_window(cx: &mut TestAppContext) {
    let (first, _) = open(cx);
    let (second, _) = open(cx);
    assert_eq!(
        start(first, PathOperation::ProjectFolder, cx),
        PathStart::Started
    );
    assert_eq!(
        start(first, PathOperation::ProjectFolder, cx),
        PathStart::Busy
    );
    assert_eq!(start(first, save(), cx), PathStart::Busy);
    second.update(cx, |_, w, _| w.activate_window()).unwrap();
    cx.run_until_parked();
    assert!(outcomes(first, cx).is_empty());
    cx.simulate_path_prompt_response(|options| {
        assert!(!options.files && options.directories && !options.multiple);
        assert_eq!(options.prompt.as_deref(), Some("Choose a Project Folder"));
        Some(vec![PathBuf::from("/synthetic/project")])
    });
    cx.run_until_parked();
    assert_eq!(
        outcomes(first, cx),
        [PathOutcome::Selected(PathBuf::from("/synthetic/project"))]
    );
    assert!(outcomes(second, cx).is_empty());
    assert!(!cx.did_prompt_for_paths());
    assert_eq!(
        start(second, PathOperation::OpenFile, cx),
        PathStart::Started
    );
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from("/synthetic/open.txt")]));
    cx.run_until_parked();
    assert_eq!(
        outcomes(second, cx),
        [PathOutcome::Selected(PathBuf::from("/synthetic/open.txt"))]
    );
}

#[gpui_kit::test]
fn open_save_selection_and_cancel_are_distinct(cx: &mut TestAppContext) {
    let (target, _) = open(cx);
    start(target, PathOperation::OpenFile, cx);
    cx.simulate_path_prompt_response(|options| {
        assert!(options.files && !options.directories && !options.multiple);
        assert_eq!(options.prompt.as_deref(), Some("Open File"));
        None
    });
    cx.run_until_parked();
    start(target, save(), cx);
    cx.simulate_new_path_selection(|directory| {
        assert_eq!(directory, std::path::Path::new("/synthetic/destination"));
        Some(PathBuf::from("/synthetic/destination/report.html"))
    });
    cx.run_until_parked();
    start(target, save(), cx);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert_eq!(
        outcomes(target, cx),
        [
            PathOutcome::Cancelled,
            PathOutcome::Selected(PathBuf::from("/synthetic/destination/report.html")),
            PathOutcome::Cancelled
        ]
    );
}

#[gpui_kit::test]
fn errors_disconnect_and_empty_selection_allow_retry(cx: &mut TestAppContext) {
    let (target, _) = open(cx);
    for operation in [PathOperation::OpenFile, save()] {
        for disconnect in [false, true] {
            assert_eq!(start(target, operation.clone(), cx), PathStart::Started);
            cx.simulate_path_prompt_failure(
                matches!(operation, PathOperation::SaveDestination { .. }),
                disconnect,
            );
            cx.run_until_parked();
        }
    }
    start(target, PathOperation::ProjectFolder, cx);
    cx.simulate_path_prompt_response(|_| Some(vec![]));
    cx.run_until_parked();
    assert_eq!(
        outcomes(target, cx),
        [
            PathOutcome::Failed(PathFailure::Platform),
            PathOutcome::Failed(PathFailure::Disconnected),
            PathOutcome::Failed(PathFailure::Platform),
            PathOutcome::Failed(PathFailure::Disconnected),
            PathOutcome::Failed(PathFailure::InvalidSelection)
        ]
    );
    assert_eq!(
        start(target, PathOperation::OpenFile, cx),
        PathStart::Started
    );
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn invalidation_discards_all_results_but_does_not_dismiss_panel(cx: &mut TestAppContext) {
    let (target, _) = open(cx);
    for response in 0..3 {
        start(target, PathOperation::ProjectFolder, cx);
        target
            .update(cx, |root, _, _| root.chooser.invalidate())
            .unwrap();
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        assert_eq!(
            start(target, PathOperation::ProjectFolder, cx),
            PathStart::Busy
        );
        match response {
            0 => {
                cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from("/synthetic/stale")]))
            }
            1 => cx.simulate_path_prompt_response(|_| None),
            _ => cx.simulate_path_prompt_failure(false, false),
        }
        cx.run_until_parked();
        assert!(outcomes(target, cx).is_empty());
    }
    start(target, PathOperation::ProjectFolder, cx);
    cx.simulate_path_prompt_response(|_| None);
    // Invalidation after channel resolution, before the task is dispatched, is stale too.
    target
        .update(cx, |root, _, _| root.chooser.invalidate())
        .unwrap();
    cx.run_until_parked();
    assert!(outcomes(target, cx).is_empty());
}

#[gpui_kit::test]
fn same_type_root_replacement_and_close_discard_late_results(cx: &mut TestAppContext) {
    let (target, old) = open(cx);
    start(target, PathOperation::OpenFile, cx);
    target
        .update(cx, |_, w, cx| {
            w.replace_root(cx, |w, cx| owner(w, cx));
        })
        .unwrap();
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from("/synthetic/stale")]));
    cx.run_until_parked();
    assert!(outcomes(target, cx).is_empty());
    assert!(cx.read(|cx| old.read(cx).outcomes.is_empty()));
    // Old root retained externally still cannot initiate a chooser for the replacement.
    target
        .update(cx, |_, w, cx| {
            old.update(cx, |old, cx| {
                assert_eq!(
                    old.chooser
                        .select(PathOperation::OpenFile, w, cx, |_, _, _, _| panic!("stale")),
                    PathStart::WrongContext
                )
            });
        })
        .unwrap();
    let (closed, retained) = open(cx);
    start(closed, save(), cx);
    closed.update(cx, |_, w, _| w.remove_window()).unwrap();
    cx.simulate_new_path_selection(|_| Some(PathBuf::from("/synthetic/stale")));
    cx.run_until_parked();
    assert!(cx.read(|cx| retained.read(cx).outcomes.is_empty()));
}

#[gpui_kit::test]
fn service_and_pending_task_do_not_keep_root_alive(cx: &mut TestAppContext) {
    let (target, root) = open(cx);
    let weak = root.downgrade();
    start(target, PathOperation::ProjectFolder, cx);
    drop(root);
    target.update(cx, |_, w, _| w.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(weak.upgrade().is_none());
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn long_wait_is_not_a_timeout_and_completion_can_retry(cx: &mut TestAppContext) {
    let (target, _) = open(cx);
    target
        .update(cx, |root, w, cx| {
            assert_eq!(
                root.chooser.select(
                    PathOperation::ProjectFolder,
                    w,
                    cx,
                    |root, outcome, w, cx| {
                        root.outcomes.push(outcome);
                        assert_eq!(
                            root.chooser.select(
                                PathOperation::OpenFile,
                                w,
                                cx,
                                |root, outcome, _, _| root.outcomes.push(outcome)
                            ),
                            PathStart::Started
                        );
                    }
                ),
                PathStart::Started
            );
        })
        .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(60 * 60 * 24));
    cx.run_until_parked();
    assert!(outcomes(target, cx).is_empty());
    assert!(cx.did_prompt_for_paths());
    assert_eq!(
        start(target, PathOperation::ProjectFolder, cx),
        PathStart::Busy
    );
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(
        outcomes(target, cx),
        [PathOutcome::Cancelled, PathOutcome::Cancelled]
    );
}

#[gpui_kit::test]
fn foreign_window_and_dropped_service_never_deliver(cx: &mut TestAppContext) {
    let (first, old) = open(cx);
    let (second, _) = open(cx);
    second
        .update(cx, |_, w, cx| {
            old.update(cx, |root, cx| {
                assert_eq!(
                    root.chooser
                        .select(PathOperation::ProjectFolder, w, cx, |_, _, _, _| panic!(
                            "foreign completion"
                        )),
                    PathStart::WrongContext
                )
            });
        })
        .unwrap();
    assert!(!cx.did_prompt_for_paths());
    start(first, PathOperation::ProjectFolder, cx);
    // Replacing the service drops its delivery scope; it is not an OS cancel.
    first
        .update(cx, |root, w, cx| root.chooser = PathChooser::new(w, cx))
        .unwrap();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from("/synthetic/abandoned")]));
    cx.run_until_parked();
    assert!(outcomes(first, cx).is_empty());
}
