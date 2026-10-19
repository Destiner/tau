//! Service receipts and Pi records are applied only under their captured owners.
use super::*;

impl Shell {
    pub(super) fn ingest(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::ExitDrained { revision } => {
                if self.runtime_lifecycle.exit_fence_pending == Some(revision) {
                    self.runtime_lifecycle.exit_fence_pending = None;
                }
            }
            Event::RegistrationStored {
                runtime,
                generation,
                id,
                path,
            } => self.empty_registration_stored(&runtime, generation, &id, &path),
            Event::EmptyDisposed {
                runtime,
                attempt,
                generation,
                success,
            } => {
                let owned = self
                    .empty_disposals
                    .contains(&(runtime.clone(), attempt, generation));
                let previous = self.error.clone();
                self.empty_disposed(runtime, attempt, generation, success, cx);
                if !success
                    && owned
                    && let Some(request) = self.lifecycle
                {
                    if let Some(error) = self.error.take() {
                        self.quit_error = Some((request, error));
                    }
                    self.error = previous;
                }
            }
            Event::Admin(reply) => self.admin_event(reply, cx),
            Event::Remote(event) => {
                let allowed = match &event.result {
                    Ok(service::RemoteResult::Imported(project)) => {
                        self.admit_import_reply(&project.path)
                    }
                    _ => true,
                };
                if allowed {
                    self.remote_event(event, cx);
                }
            }
            Event::ModelScope {
                runtime,
                token,
                patterns,
            } => self.model_scope_result(runtime, token, patterns, cx),
            Event::WorkspaceMutation { revision, success } => {
                self.finish_workspace_mutation(revision, success)
            }
            Event::StartCancelled {
                runtime,
                attempt,
                success,
            } => self.finish_start_cancel(runtime, attempt, success, cx),
            Event::CleanupBlocked {
                runtime,
                attempt,
                generation,
            } => {
                self.block_cleanup(runtime, attempt, generation);
            }
            Event::RuntimeReleased {
                runtime,
                attempt,
                generation,
                success,
            } => self.finish_runtime_release(runtime, attempt, generation, success, cx),
            Event::ProjectRemoved {
                project,
                token,
                success,
            } => self.finish_project_removal(project, token, success, cx),
            Event::RegisteredRuntimeRow {
                runtime,
                generation,
                project,
                row,
            } => {
                if self.project_reply_allowed(&project)
                    && self.sessions.get(&runtime).is_some_and(|live| {
                        live.project.path == project
                            && (live.controller.generation() == generation
                                || live.registration_pending.contains(&(
                                    generation,
                                    row.id.clone(),
                                    row.path.clone(),
                                )))
                    })
                {
                    self.merge_registered_row(project, row);
                }
            }
            Event::ProjectImported {
                path,
                token,
                result,
            } => self.finish_import(path, token, result),
            Event::ImportedProject(Ok(project)) => {
                if self.admit_import_reply(&project.path) {
                    self.merge_workspace_project(project);
                }
            }
            Event::Loaded(Ok((mut workspace, preferences))) => {
                self.reconcile_workspace_mutations(&mut workspace);
                self.workspace = Some(workspace);
                self.preferences = preferences;
                if let Some(sidebar) = &self.sidebar {
                    sidebar.update(cx, |sidebar, cx| {
                        sidebar.set_width(self.preferences.sidebar_width.unwrap_or(260.), cx);
                        self.preferences.sidebar_width = Some(sidebar.width());
                    });
                }
                self.loaded = true;
                self.sidebar_state.invalidate_metadata();
            }
            Event::Loaded(Err(())) => {
                self.error = Some(
                    "Tau could not load its workspace. Try again; saved data was not changed."
                        .into(),
                )
            }
            #[cfg(test)]
            Event::Workspace(Ok(mut workspace)) => {
                self.reconcile_workspace_mutations(&mut workspace);
                self.reconcile_archive(&mut workspace);
                for live in self.sessions.values_mut() {
                    if let Some(project) = workspace
                        .projects
                        .iter()
                        .find(|p| p.path == live.project.path)
                    {
                        live.project = project.clone();
                    }
                }
                self.workspace = Some(workspace);
                self.sidebar_state.invalidate_metadata();
            }
            Event::NameStored {
                runtime,
                generation,
                revision,
                id,
                path,
                name,
                success,
            } => self.finish_name(runtime, generation, revision, id, path, name, success),
            Event::SourceArchived {
                project,
                id,
                path,
                token,
                success,
            } => {
                self.finish_source_recovery(&project, &id, &path, token, success, cx);
            }
            Event::Archived {
                project,
                id,
                revision,
                archived,
                success,
            } => {
                if self.project_reply_allowed(&project) {
                    self.finish_archive(project, id, revision, archived, success);
                }
            }
            Event::RegistrationFailed(command) => {
                if !self.empty_session_retry_is_current(&command)
                    || !self.registration_failure_owned(&command)
                {
                    return;
                }
                if !self.registration_retry_queued(&command) {
                    self.register_retries.push(command);
                }
                let error =
                    "A completed session could not be added to the workspace. Retry saving it."
                        .into();
                if let Some(request) = self.lifecycle {
                    self.quit_error = Some((request, error));
                } else {
                    self.error = Some(error);
                }
            }
            Event::ImportedProject(Err(())) => {
                self.error = Some("Workspace changes could not be saved. Try again.".into())
            }
            #[cfg(test)]
            Event::Workspace(Err(())) => {
                self.error = Some("Workspace changes could not be saved. Try again.".into())
            }
            Event::Started {
                runtime,
                attempt,
                result,
            } => {
                if let Some(live) = self.sessions.get_mut(&runtime) {
                    if attempt != live.start_attempt {
                        return;
                    }
                    live.starting = false;
                    self.sidebar_state.invalidate_projection();
                    live.dirty = true;
                    live.projection_dirty = true;
                    live.controller
                        .tick(self.epoch.elapsed().as_millis() as u64);
                    let succeeded = result.is_ok();
                    let effects = match result {
                        Ok(generation) => live.controller.begin_generation(generation),
                        Err(()) => live
                            .controller
                            .transport_failed(live.controller.generation()),
                    };
                    let buffered = std::mem::take(&mut live.startup_events);
                    self.effects(&runtime, effects, cx);
                    if succeeded {
                        self.runtime_started(&runtime);
                        for event in buffered {
                            self.ingest(Event::Pi(event), cx);
                        }
                    }
                }
            }
            Event::Written {
                runtime,
                generation,
                request,
                success,
            } => {
                if let Some(live) = self.sessions.get_mut(&runtime) {
                    if live.controller.generation() != generation {
                        return;
                    }
                    if !success || request["type"] == "extension_ui_response" {
                        self.sidebar_state.invalidate_projection();
                    }
                    live.dirty |= !success || request["type"] == "extension_ui_response";
                    live.projection_dirty |= !success;
                    let id = request["id"].as_str().unwrap_or_default();
                    let effects = if request["type"] == "extension_ui_response" {
                        if live
                            .controller
                            .state()
                            .extension_dialogs
                            .iter()
                            .any(|r| r.id == id && r.submitting)
                        {
                            live.extension_error = (!success).then(|| {
                                (
                                    generation,
                                    id.into(),
                                    "The extension response could not be sent. Try again.".into(),
                                )
                            });
                        }
                        live.controller.extension_sent(id, success)
                    } else if !success {
                        live.controller.request_failed(id)
                    } else {
                        live.controller.request_write_accepted(generation, id);
                        vec![]
                    };
                    self.effects(&runtime, effects, cx);
                }
            }
            Event::Pi(event) => {
                if event.kind == "exited"
                    && (self.reconcile_failed_release_exit(&event.runtime_id, event.generation, cx)
                        || self.defer_release_exit(&event.runtime_id, event.generation))
                {
                    return;
                }
                if self.runtime_was_released(&event.runtime_id)
                    && !self
                        .sessions
                        .get(&event.runtime_id)
                        .is_some_and(|live| live.starting)
                {
                    return;
                }
                if let Some(live) = self.sessions.get_mut(&event.runtime_id) {
                    if live.starting {
                        if live.startup_events.len() < 512 {
                            live.startup_events.push_back(event);
                        } else {
                            let runtime = event.runtime_id;
                            let attempt = live.start_attempt;
                            live.starting = false;
                            live.start_attempt += 1;
                            live.startup_events.clear();
                            live.controller
                                .transport_failed(live.controller.generation());
                            live.dirty = true;
                            live.projection_dirty = true;
                            self.dispatch(Command::CancelStart { runtime, attempt }, cx);
                        }
                        return;
                    }
                    if live.controller.generation() != event.generation {
                        return;
                    }
                    live.dirty = true;
                    live.projection_dirty = true;
                    let effects = if event.kind == "rpc" {
                        if let Some(value) =
                            event.line.and_then(|line| serde_json::from_str(&line).ok())
                        {
                            self.observe_sidebar_event(&event.runtime_id, event.generation, &value);
                            if value["type"] == "response" {
                                self.sidebar_state.invalidate_metadata();
                            }
                            let live = self.sessions.get_mut(&event.runtime_id).unwrap();
                            let before = (
                                live.controller.state().working,
                                live.controller.state().stopping,
                                live.controller.state().extension_dialogs.len(),
                            );
                            let effects = live.controller.ingest_output(
                                &event.runtime_id,
                                event.generation,
                                value,
                            );
                            let after = (
                                live.controller.state().working,
                                live.controller.state().stopping,
                                live.controller.state().extension_dialogs.len(),
                            );
                            if before != after {
                                self.sidebar_state.invalidate_projection();
                            }
                            effects
                        } else {
                            vec![]
                        }
                    } else if event.kind == "input_closed" {
                        self.sidebar_state.invalidate_projection();
                        live.controller.input_closed(event.generation)
                    } else if matches!(event.kind, "exited" | "error") {
                        self.sidebar_state.invalidate_projection();
                        live.controller.transport_failed(event.generation)
                    } else {
                        vec![]
                    };
                    self.effects(&event.runtime_id, effects, cx);
                }
            }
            Event::Saved {
                revision,
                success,
                stopped,
            } => {
                self.sidebar_state.invalidate_projection();
                let snapshot = self.pending_saves.remove(&revision);
                let advances_baseline =
                    success && snapshot.is_some() && revision > self.saved_revision;
                if advances_baseline {
                    self.saved_revision = revision;
                }
                if let Some(mut saved) = snapshot
                    && advances_baseline
                {
                    for draft in &saved.drafts {
                        if let Some(live) = self.sessions.get_mut(&draft.key) {
                            live.persisted_draft = draft.text.clone();
                            live.persisted_recovered = draft.recovered.len();
                        }
                    }
                    for (key, live) in &mut self.sessions {
                        if !saved.drafts.iter().any(|draft| &draft.key == key) {
                            live.persisted_draft.clear();
                            live.persisted_recovered = 0;
                        }
                    }
                    // A newer attempt can fail after this write succeeded. Keep
                    // its durable draft baseline without reinstalling old UI state.
                    if revision == self.save_revision {
                        saved.sidebar_width = self.preferences.sidebar_width;
                        self.preferences = saved;
                    }
                }
                // A physical stop receipt is not itself a storage receipt.
                // Handoff observed completed output before clearing protocol state.
                if self.exit_revision.is_some() {
                    self.drain_exit_durability(cx);
                }
                let cleanup_ran = !stopped.is_empty();
                for (runtime, generation) in stopped {
                    if let Some(live) = self.sessions.get_mut(&runtime) {
                        let effects = live.controller.backend_stopped(generation);
                        if live.controller.generation() == generation {
                            self.runtime_lifecycle.released.insert(runtime.clone());
                            self.runtime_lifecycle
                                .reconnect_required
                                .insert(runtime.clone());
                        }
                        live.projection_dirty = true;
                        self.effects(&runtime, effects, cx);
                    }
                }
                for live in self.sessions.values_mut() {
                    live.dirty = true;
                }
                if !success {
                    if revision == self.save_revision {
                        self.save_failed = true;
                    }
                    if let Some((expected, request)) = self
                        .exit_revision
                        .filter(|(expected, _)| *expected == revision)
                    {
                        let _ = expected;
                        self.quit_error = Some((request, "Draft saving or Pi cleanup remains unconfirmed. Tau will not reconnect automatically. Retrying cannot prove remote exit after a transport failure; closing remains blocked.".into()));
                        self.exit_revision = None;
                    } else if revision == self.save_revision {
                        self.error = Some("Drafts could not be saved. Try again.".into());
                    }
                } else if revision == self.save_revision {
                    self.save_failed = false;
                    if self.error.as_deref() == Some("Drafts could not be saved. Try again.") {
                        self.error = None;
                    }
                }
                if success
                    && let Some((expected, request)) = self.exit_revision
                    && expected == revision
                {
                    self.exit_revision = None;
                    self.runtime_lifecycle.exit_saved = Some((revision, request, cleanup_ran));
                    self.runtime_lifecycle.exit_fence_pending = Some(revision);
                    self.dispatch(Command::ExitFence { revision }, cx);
                    if !self.register_retries.is_empty() {
                        self.quit_error = Some((
                            request,
                            "Session changes still need saving. Retry before closing Tau.".into(),
                        ));
                    }
                    // In-flight storage receipts are awaited, never treated as success
                    // (or converted to a fabricated failure/deadline).
                }
            }
        }
        self.reconcile_empty_sessions(cx);
        self.source_recoveries(cx);
        self.release_idle_runtimes(cx);
        if self.exit_revision.is_some() {
            self.drain_exit_durability(cx);
        }
        self.advance_exit(cx);
        if !self.save_failed
            && self.exit_revision.is_none()
            && self.runtime_lifecycle.exit_wait.is_none()
            && self.runtime_lifecycle.exit_saved.is_none()
            && self.sessions.values().any(|live| {
                live.controller.state().draft != live.persisted_draft
                    || live.controller.state().recovered_drafts.len()
                        + live.restored_recovered.len()
                        != live.persisted_recovered
            })
        {
            self.persist(false, cx);
        }
        cx.notify();
    }
}
