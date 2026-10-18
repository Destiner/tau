use super::*;

pub(super) const CLEANUP_ERROR: &str = "Pi could not be stopped. Tau retained cleanup ownership and blocked reconnecting. Retry cleanup; no submitted work will be replayed.";
pub(super) const REMOTE_CLEANUP_ERROR: &str = "Pi cleanup remains unconfirmed. Tau retained cleanup ownership and blocked reconnecting. Retrying cannot prove remote exit after a transport failure; no submitted work will be replayed.";

pub(super) const RECOVERY_ERROR: &str =
    "The stale session could not be archived. Retry; your unsent source is retained.";

impl Shell {
    pub(in crate::app) fn workflow_failure(
        &self,
        key: &str,
    ) -> Option<(WorkflowErrorToken, &'static str)> {
        let failed = self
            .runtime_lifecycle
            .releases
            .iter()
            .find(|(token, op)| {
                (token.runtime == key || matches!(op.reason, ReleaseReason::Removed(_)))
                    && op.failed
                    && !self
                        .runtime_lifecycle
                        .acknowledged_releases
                        .contains(*token)
            })
            .map(|(token, _)| token.clone());
        if let Some(token) = failed {
            let blocked = self.runtime_lifecycle.blocked_cleanup.contains(&(
                token.runtime.clone(),
                token.attempt,
                Some(token.generation),
            ));
            Some((
                WorkflowErrorToken::Release(token),
                if blocked {
                    REMOTE_CLEANUP_ERROR
                } else {
                    CLEANUP_ERROR
                },
            ))
        } else if let Some(token) = self
            .runtime_lifecycle
            .recoveries
            .iter()
            .find(|(token, op)| {
                op.runtime == key
                    && op.failed
                    && !self
                        .runtime_lifecycle
                        .acknowledged_recoveries
                        .contains(*token)
            })
            .map(|(token, _)| token.clone())
        {
            Some((
                WorkflowErrorToken::Recovery(token.0, token.1, token.2),
                RECOVERY_ERROR,
            ))
        } else {
            self.register_retries.iter().find_map(|command| {
                let (runtime, attempt, generation, message) = match command {
                    Command::CancelStart { runtime, attempt } => {
                        (runtime, *attempt, None, STARTUP_CLEANUP_ERROR)
                    }
                    Command::DisposeEmpty {
                        runtime,
                        attempt,
                        generation,
                    } => (
                        runtime,
                        *attempt,
                        Some(*generation),
                        empty_sessions::CLEANUP_ERROR,
                    ),
                    _ => return None,
                };
                if (runtime == key || !self.sessions.contains_key(runtime))
                    && !self.runtime_lifecycle.acknowledged_cleanup.contains(&(
                        runtime.clone(),
                        attempt,
                        generation,
                    ))
                {
                    Some((
                        WorkflowErrorToken::Cleanup(runtime.clone(), attempt, generation),
                        message,
                    ))
                } else {
                    None
                }
            })
        }
    }
    pub(in crate::app) fn acknowledge_workflow_failure(
        &mut self,
        token: &WorkflowErrorToken,
    ) -> bool {
        if self
            .workflow_failure(&self.active)
            .as_ref()
            .map(|(owner, _)| owner)
            != Some(token)
        {
            return false;
        }
        match token {
            WorkflowErrorToken::Release(token) => {
                self.runtime_lifecycle
                    .acknowledged_releases
                    .insert(token.clone());
            }
            WorkflowErrorToken::Recovery(project, id, revision) => {
                self.runtime_lifecycle.acknowledged_recoveries.insert((
                    project.clone(),
                    id.clone(),
                    *revision,
                ));
            }
            WorkflowErrorToken::Cleanup(runtime, attempt, generation) => {
                self.runtime_lifecycle.acknowledged_cleanup.insert((
                    runtime.clone(),
                    *attempt,
                    *generation,
                ));
            }
        }
        true
    }
    pub(in crate::app) fn defer_release_exit(&mut self, runtime: &str, generation: u64) -> bool {
        let token = self
            .runtime_lifecycle
            .releases
            .iter()
            .find(|(token, op)| {
                token.runtime == runtime && token.generation == generation && !op.failed
            })
            .map(|(token, _)| token.clone());
        if let Some(token) = token {
            // Stop drains stdout before its receipt; its terminal event is not
            // an unexpected transport loss. RPC records still reconcile normally.
            self.runtime_lifecycle.release_exits.insert(token);
            true
        } else {
            false
        }
    }
    pub(in crate::app) fn reconcile_failed_release_exit(
        &mut self,
        runtime: &str,
        generation: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.runtime_lifecycle.releases.iter().any(|(token, op)| {
            token.runtime == runtime && token.generation == generation && op.failed
        }) {
            return false;
        }
        // A retained failed owner has no outstanding receipt to drain this exit.
        // Disconnect now without treating local transport exit as remote proof.
        if let Some(live) = self.sessions.get_mut(runtime)
            && live.controller.generation() == generation
        {
            let effects = live.controller.backend_stopped(generation);
            live.dirty = true;
            live.projection_dirty = true;
            self.effects(runtime, effects, cx);
        }
        true
    }
    pub(in crate::app) fn touch_runtime(&mut self, key: &str) {
        self.runtime_lifecycle.sequence += 1;
        self.runtime_lifecycle
            .activity
            .insert(key.into(), self.runtime_lifecycle.sequence);
    }
    pub(in crate::app) fn runtime_cleanup_pending(&self, key: &str) -> bool {
        self.runtime_lifecycle
            .releases
            .keys()
            .any(|t| t.runtime == key)
            || self
                .runtime_lifecycle
                .cancelled
                .iter()
                .any(|(runtime, _)| runtime == key)
    }
    pub(in crate::app) fn resumable_source(&self, key: &str) -> Option<String> {
        let live = self.sessions.get(key)?;
        let identity = &live.controller.state().identity;
        let (id, path) = if identity.id.is_empty() {
            (&live.source_id, &live.source_path)
        } else {
            (&identity.id, &identity.path)
        };
        if id.is_empty() || path.is_empty() {
            return None;
        }
        self.workspace
            .as_ref()?
            .projects
            .iter()
            .find(|p| p.path == live.project.path)?
            .sessions
            .iter()
            .find(|row| row.id == *id && row.path == *path)
            .map(|row| row.path.clone())
    }
    pub(in crate::app) fn resume_runtime(&mut self, key: &str, cx: &mut Context<Self>) {
        self.touch_runtime(key);
        if self.runtime_cleanup_pending(key) {
            return;
        }
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        let effects = live.controller.resume_reads();
        self.effects(key, effects, cx);
        // Remote disconnects require explicit reconnect; a warm process is left alone.
        if self
            .sessions
            .get(key)
            .is_some_and(|live| live.project.connection_string.is_none())
        {
            self.start_live(key, cx);
        }
    }
    pub(in crate::app) fn release_idle_runtimes(&mut self, cx: &mut Context<Self>) {
        if self.lifecycle.is_some() {
            return;
        }
        let mut idle: Vec<_> = self
            .sessions
            .iter()
            .filter(|(key, live)| {
                key.as_str() != self.active
                    && !live.starting
                    && live.controller.generation() != 0
                    && live.controller.can_release_runtime()
                    && live.registration_pending.is_empty()
                    && !self.runtime_cleanup_pending(key)
                    && live.extension.is_none()
                    && self.saved_session(key).is_some_and(|(_, row, _)| {
                        !row.path.is_empty() && row.path == live.controller.state().identity.path
                    })
                    && !self
                        .runtime_lifecycle
                        .recoveries
                        .values()
                        .any(|op| op.runtime == **key)
            })
            .map(|(key, _)| {
                (
                    self.runtime_lifecycle
                        .activity
                        .get(key)
                        .copied()
                        .unwrap_or(0),
                    key.clone(),
                )
            })
            .collect();
        idle.sort();
        let excess = idle.len().saturating_sub(6);
        for (_, key) in idle.into_iter().take(excess) {
            let live = &self.sessions[&key];
            let token = RuntimeToken {
                runtime: key,
                attempt: live.start_attempt,
                generation: live.controller.generation(),
            };
            self.runtime_lifecycle.releases.insert(
                token.clone(),
                Release {
                    reason: ReleaseReason::Idle(live.controller.state().identity.clone()),
                    failed: false,
                },
            );
            self.dispatch(
                Command::ReleaseRuntime {
                    runtime: token.runtime,
                    attempt: token.attempt,
                    generation: token.generation,
                },
                cx,
            );
        }
    }
    pub(in crate::app) fn finish_runtime_release(
        &mut self,
        runtime: String,
        attempt: u64,
        generation: u64,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        let token = RuntimeToken {
            runtime: runtime.clone(),
            attempt,
            generation,
        };
        let Some(op) = self.runtime_lifecycle.releases.get_mut(&token) else {
            return;
        };
        if !success {
            op.failed = true;
            if self.runtime_lifecycle.release_exits.remove(&token)
                && let Some(live) = self.sessions.get_mut(&runtime)
                && live.controller.generation() == generation
            {
                let effects = live.controller.backend_stopped(generation);
                live.dirty = true;
                live.projection_dirty = true;
                self.effects(&runtime, effects, cx);
            }
            return;
        }
        let identity = match &op.reason {
            ReleaseReason::Idle(identity) => Some(identity.clone()),
            ReleaseReason::Removed(_) => None,
        };
        let mut resume = false;
        if let Some(identity) = identity
            && let Some(live) = self.sessions.get_mut(&runtime)
            && live.controller.generation() == generation
        {
            // Admission to stop is not an acknowledgement: intervening work cannot
            // make this exact, physically stopped generation live again.
            let clean_idle = live.controller.state().identity == identity
                && live.controller.release_generation(generation);
            self.runtime_lifecycle.released.insert(runtime.clone());
            live.dirty = true;
            live.projection_dirty = true;
            if !clean_idle || live.project.connection_string.is_some() {
                let effects = live.controller.backend_stopped(generation);
                live.dirty = true;
                live.projection_dirty = true;
                self.effects(&runtime, effects, cx);
            }
            if !clean_idle {
                self.runtime_lifecycle
                    .reconnect_required
                    .insert(runtime.clone());
            }
            resume = clean_idle && self.resumable_source(&runtime).is_some();
        }
        // Keep the operation owner until the stopped generation is reconciled.
        self.runtime_lifecycle.releases.remove(&token);
        self.runtime_lifecycle.release_exits.remove(&token);
        self.runtime_lifecycle.acknowledged_releases.remove(&token);
        self.runtime_lifecycle.blocked_cleanup.remove(&(
            runtime.clone(),
            attempt,
            Some(generation),
        ));
        if resume && self.active == runtime {
            self.resume_runtime(&runtime, cx);
        }
        self.sidebar_state.invalidate_projection();
    }
}
