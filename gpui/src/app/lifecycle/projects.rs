use super::*;

impl Shell {
    pub(in crate::app) fn project_removal_inflight(&self) -> bool {
        self.runtime_lifecycle
            .removals
            .values()
            .any(|op| !op.failed)
    }
    pub(in crate::app) fn remove_project_available(&self, project: &str) -> bool {
        self.loaded
            && self.lifecycle.is_none()
            && !self.picker_busy
            && !self.remote.is_busy()
            && !self.runtime_lifecycle.removals.contains_key(project)
            && !self.runtime_lifecycle.removed.contains(project)
            && self
                .workspace
                .as_ref()
                .is_some_and(|w| w.projects.iter().any(|p| p.path == project))
    }
    pub(in crate::app) fn remove_project(&mut self, project: &str, cx: &mut Context<Self>) {
        if !self.project_actions_admitted(cx) || !self.remove_project_available(project) {
            return;
        }
        self.runtime_lifecycle.sequence += 1;
        let token = self.runtime_lifecycle.sequence;
        self.runtime_lifecycle.removals.insert(
            project.into(),
            Removal {
                token,
                failed: false,
            },
        );
        self.dispatch(
            Command::RemoveProject {
                project: project.into(),
                token,
            },
            cx,
        );
        self.refresh_window(cx);
    }
    pub(in crate::app) fn prepare_import(&mut self, path: String) -> Option<Command> {
        if self.runtime_lifecycle.imports.contains_key(&path) {
            return None;
        }
        self.runtime_lifecycle.sequence += 1;
        let token = self.runtime_lifecycle.sequence;
        self.runtime_lifecycle.imports.insert(path.clone(), token);
        Some(Command::ImportProject { path, token })
    }
    pub(in crate::app) fn finish_import(
        &mut self,
        path: String,
        token: u64,
        result: Result<ProjectSummary, ()>,
    ) {
        if self.runtime_lifecycle.imports.get(&path) != Some(&token) {
            return;
        }
        self.runtime_lifecycle.imports.remove(&path);
        match result {
            Ok(project) if self.admit_import_reply(&project.path) => {
                self.merge_workspace_project(project)
            }
            Ok(_) => {}
            Err(()) => {
                self.error = Some("The project could not be imported. Try importing again.".into())
            }
        }
    }
    pub(in crate::app) fn import_admitted(&self) -> bool {
        self.runtime_lifecycle.removals.is_empty()
            && !self
                .runtime_lifecycle
                .releases
                .values()
                .any(|op| matches!(op.reason, ReleaseReason::Removed(_)))
    }
    pub(in crate::app) fn project_reply_allowed(&self, project: &str) -> bool {
        !self.runtime_lifecycle.removed.contains(project)
    }
    pub(in crate::app) fn finish_project_removal(
        &mut self,
        project: String,
        token: u64,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(op) = self
            .runtime_lifecycle
            .removals
            .get_mut(&project)
            .filter(|op| op.token == token)
        else {
            return;
        };
        if !success {
            op.failed = true;
            self.error = Some(
                "Project could not be removed. Retry; its sessions and drafts are unchanged."
                    .into(),
            );
            return;
        }
        self.runtime_lifecycle.removals.remove(&project);
        self.runtime_lifecycle.removed.insert(project.clone());
        self.workspace_mutations.remove_project(&project);
        self.session_actions.remove_project(&project);
        self.sidebar_state.remove_project(&project);
        self.runtime_lifecycle.imports.remove(&project);
        if let Some(workspace) = &mut self.workspace {
            workspace.projects.retain(|p| p.path != project);
            if workspace.active_project_path == project {
                workspace.active_project_path.clear();
            }
        }
        scrub_project_preferences(&mut self.preferences, &project);
        for prefs in self.pending_saves.values_mut() {
            scrub_project_preferences(prefs, &project);
        }
        let keys: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, live)| live.project.path == project)
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            let live = self.sessions.remove(&key).unwrap();
            let token = RuntimeToken {
                runtime: key.clone(),
                attempt: live.start_attempt,
                generation: live.controller.generation(),
            };
            self.runtime_lifecycle.activity.remove(&key);
            self.runtime_lifecycle.released.remove(&key);
            self.runtime_lifecycle.reconnect_required.remove(&key);
            self.runtime_lifecycle
                .releases
                .retain(|t, _| t.runtime != key);
            self.runtime_lifecycle.releases.insert(
                token.clone(),
                Release {
                    reason: ReleaseReason::Removed(project.clone()),
                    failed: false,
                },
            );
            self.clear_session_action_identity(&key);
            self.sidebar_state.remove_runtime(&key);
            self.empty_candidates.remove(&key);
            self.register_retries
                .retain(|c| !matches!(c, Command::Register { runtime, .. } if runtime == &key));
            for prefs in self.pending_saves.values_mut() {
                prefs.drafts.retain(|d| d.project != project);
                if prefs.selected_draft == key {
                    prefs.selected_draft.clear();
                }
            }
            if self.preferences.selected_draft == key {
                self.preferences.selected_draft.clear();
            }
            if self.active == key {
                self.active.clear();
            }
            self.dispatch(
                Command::ReleaseRuntime {
                    runtime: key,
                    attempt: token.attempt,
                    generation: token.generation,
                },
                cx,
            );
        }
        self.runtime_lifecycle
            .recoveries
            .retain(|(p, _, _), _| p != &project);
        self.sidebar_state.invalidate_metadata();
        self.persist(false, cx);
    }
    pub(in crate::app) fn admit_import_reply(&mut self, project: &str) -> bool {
        if self.runtime_lifecycle.removals.contains_key(project)
            || self
                .runtime_lifecycle
                .releases
                .values()
                .any(|op| matches!(&op.reason, ReleaseReason::Removed(p) if p == project))
        {
            return false;
        }
        self.runtime_lifecycle.removed.remove(project);
        true
    }
}

fn scrub_project_preferences(preferences: &mut Preferences, project: &str) {
    if preferences
        .drafts
        .iter()
        .any(|draft| draft.project == project && draft.key == preferences.selected_draft)
    {
        preferences.selected_draft.clear();
    }
    preferences.drafts.retain(|draft| draft.project != project);
}
