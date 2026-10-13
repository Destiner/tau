//! Acknowledgement-owned workflow recovery and bounded warm-runtime lifetime.
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
use super::*;
mod projects;
mod runtimes;
mod source_recovery;
use tau_session::session::Identity;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RuntimeToken {
    runtime: String,
    attempt: u64,
    generation: u64,
}
#[derive(Clone)]
enum ReleaseReason {
    Idle(Identity),
    Removed(String),
}
struct Release {
    reason: ReleaseReason,
    failed: bool,
}
struct Removal {
    token: u64,
    failed: bool,
}
struct Recovery {
    runtime: String,
    generation: u64,
    original: Identity,
    failed: bool,
}
#[derive(Default)]
pub(super) struct State {
    sequence: u64,
    imports: BTreeMap<String, u64>,
    activity: BTreeMap<String, u64>,
    releases: BTreeMap<RuntimeToken, Release>,
    removals: BTreeMap<String, Removal>,
    removed: std::collections::BTreeSet<String>,
    recoveries: BTreeMap<(String, String, u64), Recovery>,
    cancelled: std::collections::BTreeSet<(String, u64)>,
    pub(super) released: std::collections::BTreeSet<String>,
    pub(super) reconnect_required: std::collections::BTreeSet<String>,
    pub(super) exit_wait: Option<(Request, bool)>,
    pub(super) exit_saved: Option<(u64, Request, bool)>,
    pub(super) exit_fence_pending: Option<u64>,
}

impl Shell {
    pub(super) fn exit_phase_owned(&self, request: Request) -> bool {
        self.exit_revision
            .is_some_and(|(_, owner)| owner == request)
            || self
                .runtime_lifecycle
                .exit_wait
                .is_some_and(|(owner, _)| owner == request)
            || self
                .runtime_lifecycle
                .exit_saved
                .is_some_and(|(_, owner, _)| owner == request)
            || self.runtime_lifecycle.exit_fence_pending.is_some()
    }
    pub(super) fn retry_exit(&mut self, request: Request, shutdown: bool, cx: &mut Context<Self>) {
        if self.lifecycle != Some(request)
            || !cx.global::<NativeHost>().lifecycle().is_pending(request)
            || self.exit_revision.is_some()
            || !self
                .quit_error
                .as_ref()
                .is_some_and(|(owner, _)| *owner == request)
        {
            return;
        }
        // Only a deliberate retry may replace a failed attempt's phase owners.
        self.quit_error = None;
        self.runtime_lifecycle.exit_wait = None;
        self.runtime_lifecycle.exit_saved = None;
        self.runtime_lifecycle.exit_fence_pending = None;
        self.begin_exit(request, shutdown, cx);
    }
    pub(super) fn exit_storage_pending(&self) -> bool {
        self.sessions.values().any(|live| {
            !live.registration_pending.is_empty()
                || live.controller.durability_pending()
                || live.controller.source_recovery().is_some()
        })
    }
    fn exit_context_changed(&self) -> bool {
        self.preferences.selected_draft != self.active
            || self.sessions.iter().any(|(key, live)| {
                let state = live.controller.state();
                let recovered = state.recovered_drafts.len() + live.restored_recovered.len();
                state.draft != live.persisted_draft
                    || recovered != live.persisted_recovered
                    || ((!state.draft.is_empty() || recovered != 0)
                        && !self.preferences.drafts.iter().any(|draft| {
                            draft.key == *key
                                && draft.project == live.project.path
                                && draft.source_id == state.identity.id
                                && draft.source_path == state.identity.path
                        }))
            })
    }
    pub(super) fn drain_exit_durability(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<_> = self.sessions.keys().cloned().collect();
        for key in keys {
            let effects = self
                .sessions
                .get_mut(&key)
                .unwrap()
                .controller
                .prepare_exit();
            self.effects(&key, effects, cx);
        }
    }
    pub(super) fn advance_exit(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.lifecycle else {
            self.runtime_lifecycle.exit_wait = None;
            self.runtime_lifecycle.exit_saved = None;
            self.runtime_lifecycle.exit_fence_pending = None;
            return;
        };
        if !cx.global::<NativeHost>().lifecycle().is_pending(request) {
            return;
        }
        if self.runtime_lifecycle.exit_wait.is_none() && self.runtime_lifecycle.exit_saved.is_none()
        {
            return;
        }
        self.drain_exit_durability(cx);
        if self.quit_error.is_some()
            || self.runtime_lifecycle.exit_fence_pending.is_some()
            || self.exit_storage_pending()
            || !self.register_retries.is_empty()
            || !self.empty_disposals.is_empty()
            || self.workflow_operations_pending()
        {
            return;
        }
        if let Some((revision, owner, cleanup_ran)) = self.runtime_lifecycle.exit_saved.take() {
            if owner != request {
                return;
            }
            if revision != self.save_revision || self.exit_context_changed() {
                self.begin_exit(
                    request,
                    cleanup_ran || matches!(request.intent(), Intent::Quit),
                    cx,
                );
            } else {
                cx.global::<NativeHost>()
                    .lifecycle()
                    .resolve(request, true, cx);
            }
            return;
        }
        let Some((owner, shutdown)) = self.runtime_lifecycle.exit_wait.take() else {
            return;
        };
        if owner != request {
            return;
        }
        self.save_failed = false;
        if self.loaded {
            self.persist(shutdown, cx);
        } else {
            self.save_revision += 1;
            self.dispatch(
                Command::Save {
                    revision: self.save_revision,
                    preferences: None,
                    shutdown,
                },
                cx,
            );
        }
        if !self.save_failed {
            self.exit_revision = Some((self.save_revision, request));
        }
    }
    pub(super) fn runtime_was_released(&self, key: &str) -> bool {
        self.runtime_lifecycle.released.contains(key)
    }
    pub(super) fn runtime_started(&mut self, key: &str) {
        self.runtime_lifecycle.released.remove(key);
        self.runtime_lifecycle.reconnect_required.remove(key);
    }
    pub(super) fn runtime_lifecycle_track_cancel(&mut self, runtime: &str, attempt: u64) {
        self.runtime_lifecycle
            .cancelled
            .insert((runtime.into(), attempt));
    }
    pub(super) fn finish_start_cancel(
        &mut self,
        runtime: String,
        attempt: u64,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        if !self
            .runtime_lifecycle
            .cancelled
            .contains(&(runtime.clone(), attempt))
        {
            return;
        }
        if success {
            self.runtime_lifecycle
                .cancelled
                .remove(&(runtime.clone(), attempt));
            self.register_retries.retain(|c| !matches!(c, Command::CancelStart { runtime: r, attempt: a } if r == &runtime && *a == attempt));
        } else {
            if !self.register_retries.iter().any(|c| matches!(c, Command::CancelStart { runtime: r, attempt: a } if r == &runtime && *a == attempt)) {
                self.register_retries.push(Command::CancelStart { runtime, attempt });
            }
            self.error =
                Some("Pi startup cleanup remains unconfirmed; reconnecting is blocked. Retrying cannot prove remote exit after a transport failure.".into());
        }
        self.refresh_window(cx);
    }
    pub(super) fn fallback_is_open(&self, cx: &App) -> bool {
        self.fallback
            .as_ref()
            .is_some_and(|(_, dialog)| dialog.read(cx).is_open())
    }
    pub(super) fn retry_workflow_pending(&self) -> bool {
        self.save_failed
            || !self.register_retries.is_empty()
            || self.runtime_lifecycle.releases.values().any(|op| op.failed)
            || self.runtime_lifecycle.removals.values().any(|op| op.failed)
            || self
                .runtime_lifecycle
                .recoveries
                .values()
                .any(|op| op.failed)
    }
    pub(super) fn retry_workflows(&mut self, cx: &mut Context<Self>) {
        if self.lifecycle.is_some() {
            return;
        }
        self.error = None;
        self.retry_workflow_operations(cx);
        if self.save_failed {
            self.persist(false, cx);
        }
        self.refresh_window(cx);
    }
    pub(super) fn workflow_operations_pending(&self) -> bool {
        !self.runtime_lifecycle.releases.is_empty()
            || !self.runtime_lifecycle.removals.is_empty()
            || !self.runtime_lifecycle.recoveries.is_empty()
            || !self.runtime_lifecycle.cancelled.is_empty()
    }
    pub(super) fn retry_workflow_operations(&mut self, cx: &mut Context<Self>) {
        let retries = std::mem::take(&mut self.register_retries);
        for command in retries {
            self.dispatch(command, cx);
        }
        let releases: Vec<_> = self
            .runtime_lifecycle
            .releases
            .iter_mut()
            .filter_map(|(token, op)| {
                if !op.failed {
                    return None;
                }
                op.failed = false;
                Some(Command::ReleaseRuntime {
                    runtime: token.runtime.clone(),
                    attempt: token.attempt,
                    generation: token.generation,
                })
            })
            .collect();
        let removals: Vec<_> = self
            .runtime_lifecycle
            .removals
            .iter_mut()
            .filter_map(|(project, op)| {
                if !op.failed {
                    return None;
                }
                op.failed = false;
                Some(Command::RemoveProject {
                    project: project.clone(),
                    token: op.token,
                })
            })
            .collect();
        let recoveries: Vec<_> = self
            .runtime_lifecycle
            .recoveries
            .iter_mut()
            .filter_map(|((project, id, revision), op)| {
                if !op.failed {
                    return None;
                }
                op.failed = false;
                Some(Command::ArchiveSource {
                    project: project.clone(),
                    id: id.clone(),
                    path: op.original.path.clone(),
                    token: *revision,
                })
            })
            .collect();
        for command in releases.into_iter().chain(removals).chain(recoveries) {
            self.dispatch(command, cx);
        }
    }
    pub(super) fn lifecycle_dispatch_failed(
        &mut self,
        command: &Command,
        cx: &mut Context<Self>,
    ) -> bool {
        match command {
            Command::ImportProject { path, token } => {
                self.finish_import(path.clone(), *token, Err(()))
            }
            Command::CancelStart { runtime, attempt } => {
                self.finish_start_cancel(runtime.clone(), *attempt, false, cx)
            }
            Command::ReleaseRuntime {
                runtime,
                attempt,
                generation,
            } => self.finish_runtime_release(runtime.clone(), *attempt, *generation, false, cx),
            Command::RemoveProject { project, token } => {
                self.finish_project_removal(project.clone(), *token, false, cx)
            }
            Command::ArchiveSource {
                project,
                id,
                path,
                token,
            } => {
                self.finish_source_recovery(project, id, path, *token, false, cx);
            }
            _ => return false,
        }
        true
    }
}
