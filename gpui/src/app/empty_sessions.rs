//! Empty-session retirement is selection-driven, never a background eviction policy.
use super::*;

const CLEANUP_ERROR: &str = "An empty session could not stop. Retry cleanup to continue.";

impl Shell {
    pub(super) fn discard_empty_before_switch(
        &mut self,
        target: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        // Reselecting cancels an earlier retirement intent, including same-active selection.
        self.empty_candidates.remove(target);
        if target == self.active || self.active.is_empty() {
            return false;
        }
        let key = self.active.clone();
        self.empty_candidates.insert(key.clone());
        self.try_discard_empty(&key, true, cx)
    }

    /// Only revisit sessions explicitly left by a valid selection, never scan blank sessions.
    pub(super) fn reconcile_empty_sessions(&mut self, cx: &mut Context<Self>) {
        let candidates: Vec<_> = self.empty_candidates.iter().cloned().collect();
        let mut discarded = false;
        for key in candidates {
            if key == self.active {
                self.empty_candidates.remove(&key);
                continue;
            }
            discarded |= self.try_discard_empty(&key, false, cx);
        }
        if discarded {
            self.persist(false, cx);
        }
    }

    fn try_discard_empty(&mut self, key: &str, selecting: bool, cx: &mut Context<Self>) -> bool {
        if !selecting && key == self.active {
            return false;
        }
        let Some(live) = self.sessions.get(key) else {
            self.empty_candidates.remove(key);
            return false;
        };
        let state = live.controller.state();
        let registered = self.workspace.as_ref().is_some_and(|workspace| {
            workspace.projects.iter().any(|project| {
                project.path == live.project.path
                    && project.sessions.iter().any(|session| {
                        (!state.identity.id.is_empty() && session.id == state.identity.id)
                            || (!state.identity.path.is_empty()
                                && session.path == state.identity.path)
                    })
            })
        });
        if !live.ephemeral
            || registered
            || state.durable
            || !state.draft.is_empty()
            || !live.shown_draft.is_empty()
            || !live.composer.read(cx).editor().read(cx).value().is_empty()
            || !state.recovered_drafts.is_empty()
            || !live.restored_recovered.is_empty()
            || !state.transcript.is_empty()
            || state.error.is_some()
            || state.working
            || state.compacting
            || state.stopping
            || !state.queue.steering.is_empty()
            || !state.queue.follow_up.is_empty()
            || !state.extension_dialogs.is_empty()
            || live.extension.is_some()
            || live.controller.submission_pending()
            || live.controller.provisional_replacement()
            || live.controller.source_recovery().is_some()
        {
            self.empty_candidates.remove(key);
            return false;
        }
        // These barriers must settle before judging history or durability. Retain the
        // selection intent so the final correlated reply can reconsider the session.
        if live.controller.reconciliation_pending()
            || self.runtime_cleanup_pending(key)
            || state.history_loading
            || live.controller.pending_count() > 0
            || !live.startup_events.is_empty()
            || !live.registration_pending.is_empty()
            || self.register_retries.iter().any(
                |command| matches!(command, Command::Register { runtime, .. } if runtime == key),
            )
        {
            return false;
        }
        let command = Command::DisposeEmpty {
            runtime: key.to_owned(),
            attempt: live.start_attempt,
            generation: live.controller.generation(),
        };
        // Keep acknowledgement ownership separate from failed operations offered for retry.
        self.empty_disposals.insert((
            key.to_owned(),
            live.start_attempt,
            live.controller.generation(),
        ));
        self.empty_candidates.remove(key);
        self.sessions.remove(key);
        self.clear_session_action_identity(key);
        if self.active == key {
            self.active.clear();
        }
        scrub_preferences(&mut self.preferences, key);
        // Old Saved callbacks must not reinstall the retired draft as an unopened draft.
        for preferences in self.pending_saves.values_mut() {
            scrub_preferences(preferences, key);
        }
        self.sidebar_state.invalidate_metadata();
        self.dispatch(command, cx);
        true
    }

    /// Only an authoritative enqueue/worker failure creates a retry.
    pub(super) fn empty_disposal_dispatch_failed(
        &mut self,
        command: Command,
        cx: &mut Context<Self>,
    ) {
        if !self.empty_disposals.contains(&disposal_identity(&command)) {
            return;
        }
        if !self
            .register_retries
            .iter()
            .any(|pending| same_disposal(pending, &command))
        {
            self.register_retries.push(command);
        }
        self.error = Some(CLEANUP_ERROR.into());
        cx.notify();
    }

    pub(super) fn empty_disposed(
        &mut self,
        runtime: String,
        attempt: u64,
        generation: u64,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        if success {
            let owned = self
                .empty_disposals
                .remove(&(runtime.clone(), attempt, generation));
            self.register_retries.retain(|command| {
                !matches!(command, Command::DisposeEmpty { runtime: key, attempt: a, generation: g }
                    if key == &runtime && *a == attempt && *g == generation)
            });
            if owned
                && self.empty_disposals.is_empty()
                && self.error.as_deref() == Some(CLEANUP_ERROR)
            {
                self.error = None;
            }
        } else {
            // No automatic tight loop: retry through the existing save/close path.
            let command = Command::DisposeEmpty {
                runtime,
                attempt,
                generation,
            };
            self.empty_disposal_dispatch_failed(command, cx);
        }
    }

    /// A submitted durability registration is user work, even before the workspace reply.
    pub(super) fn protect_empty_registration(&mut self, command: &Command) {
        if let Command::Register {
            runtime,
            generation,
            id,
            path,
            ..
        } = command
            && let Some(live) = self.sessions.get_mut(runtime)
        {
            live.registration_pending
                .insert((*generation, id.clone(), path.clone()));
        }
    }

    /// A predecessor's accepted registration may finish after Pi replaces its identity.
    /// Workspace snapshots alone are not acknowledgements of this operation.
    pub(super) fn empty_registration_stored(
        &mut self,
        runtime: &str,
        generation: u64,
        id: &str,
        path: &str,
    ) {
        if let Some(live) = self.sessions.get_mut(runtime) {
            live.registration_pending
                .remove(&(generation, id.to_owned(), path.to_owned()));
        }
        self.register_retries.retain(|command| {
            !matches!(command, Command::Register { runtime: r, generation: g, id: i, path: p, .. }
                if r == runtime && *g == generation && i == id && p == path)
        });
    }

    pub(super) fn registration_failure_owned(&self, command: &Command) -> bool {
        let Command::Register {
            runtime,
            generation,
            id,
            path,
            ..
        } = command
        else {
            return false;
        };
        self.sessions.get(runtime).is_some_and(|live| {
            live.registration_pending
                .contains(&(*generation, id.clone(), path.clone()))
        })
    }
    pub(super) fn registration_retry_queued(&self, command: &Command) -> bool {
        let Command::Register {
            runtime,
            generation,
            id,
            path,
            ..
        } = command
        else {
            return false;
        };
        self.register_retries.iter().any(|queued| matches!(queued, Command::Register { runtime: r, generation: g, id: i, path: p, .. } if r == runtime && g == generation && i == id && p == path))
    }
    /// Guard registry and start retries before they enter the FIFO worker.
    pub(super) fn empty_session_retry_is_current(&self, command: &Command) -> bool {
        match command {
            Command::Register {
                runtime,
                generation,
                ..
            } => {
                self.registration_failure_owned(command)
                    || self
                        .sessions
                        .get(runtime)
                        .is_some_and(|live| live.controller.generation() == *generation)
            }
            Command::Start {
                runtime, attempt, ..
            } => self
                .sessions
                .get(runtime)
                .is_some_and(|live| live.start_attempt == *attempt),
            // Cleanup deliberately outlives its UI session, but not its success ack.
            Command::DisposeEmpty { .. } => {
                self.empty_disposals.contains(&disposal_identity(command))
            }
            _ => true,
        }
    }
}

fn disposal_identity(command: &Command) -> (String, u64, u64) {
    let Command::DisposeEmpty {
        runtime,
        attempt,
        generation,
    } = command
    else {
        unreachable!("cleanup ownership requires a disposal command")
    };
    (runtime.clone(), *attempt, *generation)
}

fn scrub_preferences(preferences: &mut Preferences, key: &str) {
    preferences.drafts.retain(|draft| draft.key != key);
    if preferences.selected_draft == key {
        preferences.selected_draft.clear();
    }
}

fn same_disposal(left: &Command, right: &Command) -> bool {
    matches!((left, right), (
        Command::DisposeEmpty { runtime: l, attempt: la, generation: lg },
        Command::DisposeEmpty { runtime: r, attempt: ra, generation: rg }
    ) if l == r && la == ra && lg == rg)
}

#[cfg(test)]
#[path = "empty_sessions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "empty_reconciliation_tests.rs"]
mod reconciliation_tests;

#[cfg(test)]
#[path = "empty_storage_tests.rs"]
mod storage_tests;
