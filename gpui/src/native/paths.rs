//! Window-root-owned native path selection. No filesystem operations or persistence.
use gpui_kit::{Context, Render, WeakEntity, Window, WindowHandle};
use std::{cell::Cell, path::PathBuf, rc::Rc};

/// Single-path operations; the host chooses which commands to expose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathOperation {
    ProjectFolder,
    OpenFile,
    SaveDestination {
        directory: PathBuf,
        suggested_name: Option<String>,
    },
}

/// Content-free failures. The host owns operation-specific failure presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFailure {
    Platform,
    Disconnected,
    InvalidSelection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathOutcome {
    Selected(PathBuf),
    Cancelled,
    Failed(PathFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStart {
    Started,
    Busy,
    WrongContext,
}

#[derive(Default)]
struct State {
    epoch: Cell<u64>,
    pending: Cell<bool>,
}

/// One chooser per owning window root/workflow. Does not keep that root alive.
///
/// Call `invalidate` when replacing workflow context. Dropping the chooser also
/// discards delivery. Logical invalidation does not dismiss the OS panel; the
/// chooser stays busy until that panel settles. There is deliberately no timeout.
/// Completion callbacks must not capture strong roots; use the provided root.
pub struct PathChooser<T> {
    owner: WeakEntity<T>,
    target: WindowHandle<T>,
    state: Rc<State>,
}

impl<T: Render + 'static> PathChooser<T> {
    /// Construct from the window's root context (not a child view context).
    pub fn new(window: &Window, cx: &Context<T>) -> Self {
        Self {
            owner: cx.entity().downgrade(),
            target: WindowHandle::new(window.window_handle().window_id()),
            state: Rc::default(),
        }
    }

    pub fn invalidate(&self) {
        self.state.epoch.set(
            self.state
                .epoch
                .get()
                .checked_add(1)
                .expect("path epoch exhausted"),
        );
    }

    /// Rejects all concurrent operations, including equivalent requests. Rejected
    /// requests never launch a panel and never invoke their completion callback.
    pub fn select(
        &self,
        operation: PathOperation,
        window: &mut Window,
        cx: &mut Context<T>,
        complete: impl FnOnce(&mut T, PathOutcome, &mut Window, &mut Context<T>) + 'static,
    ) -> PathStart {
        if window.window_handle().window_id() != self.target.window_id()
            || cx.entity_id() != self.owner.entity_id()
            || window
                .root::<T>()
                .flatten()
                .is_none_or(|root| root.entity_id() != self.owner.entity_id())
        {
            return PathStart::WrongContext;
        }
        if self.state.pending.replace(true) {
            return PathStart::Busy;
        }
        let epoch = self.state.epoch.get();
        let state = Rc::downgrade(&self.state);
        let owner = self.owner.clone();
        let target = self.target;
        // Create the receiver synchronously so duplicate calls cannot launch panels.
        let selection: std::pin::Pin<Box<dyn std::future::Future<Output = PathOutcome>>> =
            match operation {
                PathOperation::ProjectFolder | PathOperation::OpenFile => {
                    let folder = operation == PathOperation::ProjectFolder;
                    let receiver = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
                        files: !folder,
                        directories: folder,
                        multiple: false,
                        prompt: Some(
                            if folder {
                                "Choose a Project Folder"
                            } else {
                                "Open File"
                            }
                            .into(),
                        ),
                    });
                    Box::pin(async move {
                        match receiver.await {
                            Ok(Ok(Some(mut paths))) if paths.len() == 1 => {
                                PathOutcome::Selected(paths.remove(0))
                            }
                            Ok(Ok(Some(_))) => PathOutcome::Failed(PathFailure::InvalidSelection),
                            Ok(Ok(None)) => PathOutcome::Cancelled,
                            Ok(Err(_)) => PathOutcome::Failed(PathFailure::Platform),
                            Err(_) => PathOutcome::Failed(PathFailure::Disconnected),
                        }
                    })
                }
                PathOperation::SaveDestination {
                    directory,
                    suggested_name,
                } => {
                    let receiver = cx.prompt_for_new_path(&directory, suggested_name.as_deref());
                    Box::pin(async move {
                        match receiver.await {
                            Ok(Ok(Some(path))) => PathOutcome::Selected(path),
                            Ok(Ok(None)) => PathOutcome::Cancelled,
                            Ok(Err(_)) => PathOutcome::Failed(PathFailure::Platform),
                            Err(_) => PathOutcome::Failed(PathFailure::Disconnected),
                        }
                    })
                }
            };
        // Use App::spawn rather than Context::spawn: no automatic root retention.
        gpui_kit::App::spawn(cx, async move |cx| {
            let outcome = selection.await;
            let Some(state) = state.upgrade() else { return };
            state.pending.set(false);
            if state.epoch.get() != epoch {
                return;
            }
            let _ = target.update(cx, |root, window, cx| {
                // Same-type root replacement is stale too, even if the old root is retained elsewhere.
                if cx.entity_id() == owner.entity_id() && state.epoch.get() == epoch {
                    complete(root, outcome, window, cx);
                }
            });
        })
        .detach();
        PathStart::Started
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
