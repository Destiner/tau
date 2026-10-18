use super::*;
use tau_core::models::SessionSummary;
use tau_session::session::Identity;
#[path = "lifecycle/session_pruning.rs"]
mod lifecycle_pruning;

#[derive(Default)]
pub(super) struct SessionActions {
    pub(super) archived: bool,
    revision: u64,
    archives: BTreeMap<(String, String), ArchiveIntent>,
    errors: BTreeMap<String, String>,
    names: BTreeMap<String, u64>,
}
struct ArchiveIntent {
    revision: u64,
    confirmed: bool,
    wanted: bool,
    attempted: bool,
}
impl Shell {
    pub(super) fn clear_session_action_identity(&mut self, key: &str) {
        self.session_actions.errors.remove(key);
        self.session_actions.names.remove(key);
    }
    pub(super) fn saved_session(
        &self,
        id: &str,
    ) -> Option<(ProjectSummary, SessionSummary, Option<String>)> {
        self.workspace.as_ref()?.projects.iter().find_map(|p| {
            p.sessions.iter().find_map(|s| {
                let live = self
                    .sessions
                    .iter()
                    .find(|(k, l)| {
                        l.project.path == p.path
                            && (l.source_id == s.id || l.controller.state().identity.id == s.id)
                            && (k.as_str() == id || s.id == id)
                    })
                    .map(|(k, _)| k.clone());
                (s.id == id || live.is_some()).then(|| (p.clone(), s.clone(), live))
            })
        })
    }
    pub(super) fn archived_items(&self) -> Vec<SessionItem> {
        self.project_archived_items()
    }
    pub(super) fn can_archive(&self, id: &str) -> bool {
        self.lifecycle.is_none()
            && self.saved_session(id).is_some_and(|(_, s, key)| {
                !s.archived
                    && key.is_none_or(|k| {
                        let l = &self.sessions[&k];
                        !l.starting
                            && !l.controller.state().working
                            && !l.controller.state().stopping
                    })
            })
    }
    pub(super) fn can_rename(&self) -> bool {
        self.lifecycle.is_none()
            && self.saved_session(&self.active).is_some()
            && self
                .sessions
                .get(&self.active)
                .is_some_and(|l| l.controller.state().ready && !l.controller.rename_pending())
    }
    pub(super) fn session_action_error(&self) -> Option<&str> {
        let saved = self
            .saved_session(&self.active)
            .map(|(p, s, _)| format!("{}\0{}", p.path, s.id));
        self.session_actions
            .errors
            .get(&self.active)
            .or_else(|| {
                saved
                    .as_ref()
                    .and_then(|k| self.session_actions.errors.get(k))
            })
            .map(String::as_str)
    }
    pub(super) fn retry_session_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let runtime = self.active.clone();
        if self.session_actions.errors.remove(&runtime).is_some() {
            if let Some(live) = self.sessions.get(&runtime) {
                self.store_session_name(
                    &runtime,
                    live.controller.generation(),
                    live.controller.state().identity.clone(),
                    cx,
                );
            }
        } else if let Some((p, s, _)) = self.saved_session(&runtime) {
            let binding = (p.path, s.id.clone());
            if let Some(intent) = self.session_actions.archives.get(&binding) {
                let action = if intent.attempted {
                    Action::Archive(s.id)
                } else {
                    Action::Restore(s.id)
                };
                self.session_action(&action, window, cx);
            }
        }
        self.sync(window, cx);
    }
    pub(super) fn session_action(
        &mut self,
        action: &Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            Action::Rename { session_id, title } => {
                // Header events carry the editor's captured identity, never the current selection.
                if session_id != &self.active || !self.can_rename() {
                    return true;
                }
                let retry = self.session_actions.errors.remove(session_id).is_some();
                let live = self.sessions.get_mut(session_id).unwrap();
                let effects = live.controller.rename(title);
                self.sidebar_state.invalidate_metadata();
                live.dirty = true;
                let identity = live.controller.state().identity.clone();
                let generation = live.controller.generation();
                if retry && effects.is_empty() {
                    self.store_session_name(session_id, generation, identity, cx);
                }
                self.effects(session_id, effects, cx);
            }
            Action::ToggleArchived => {
                self.session_actions.archived = !self.session_actions.archived
            }
            Action::Archive(id) | Action::Restore(id) => {
                let archived = matches!(action, Action::Archive(_));
                if self.lifecycle.is_some() || (archived && !self.can_archive(id)) {
                    return true;
                }
                let Some((p, s, key)) = self.saved_session(id) else {
                    return true;
                };
                let binding = (p.path.clone(), s.id.clone());
                self.session_actions.revision += 1;
                let revision = self.session_actions.revision;
                let confirmed = self
                    .session_actions
                    .archives
                    .get(&binding)
                    .map_or(s.archived, |i| i.confirmed);
                self.session_actions.archives.insert(
                    binding,
                    ArchiveIntent {
                        revision,
                        confirmed,
                        wanted: archived,
                        attempted: archived,
                    },
                );
                self.session_actions
                    .errors
                    .remove(&format!("{}\0{}", p.path, s.id));
                self.set_archived(&p.path, &s.id, archived);
                self.dispatch(
                    Command::Archive {
                        project: p.path.clone(),
                        id: s.id,
                        revision,
                        archived,
                    },
                    cx,
                );
                if archived && key.as_ref() == Some(&self.active) {
                    let next = self
                        .workspace
                        .as_ref()
                        .and_then(|w| w.projects.iter().find(|project| project.path == p.path))
                        .and_then(|p| p.sessions.iter().find(|s| !s.archived))
                        .map(|s| s.id.clone());
                    if let Some(next) = next {
                        self.select(&next, window, cx);
                    } else {
                        self.new_session(Some(p.path), window, cx);
                    }
                }
                self.persist(false, cx);
            }
            _ => return false,
        }
        self.sync(window, cx);
        true
    }
    fn set_archived(&mut self, project: &str, id: &str, archived: bool) {
        self.sidebar_state.invalidate_metadata();
        if let Some(w) = &mut self.workspace {
            for p in &mut w.projects {
                if p.path == project {
                    for s in &mut p.sessions {
                        if s.id == id {
                            s.archived = archived;
                            if archived {
                                s.selected = false;
                            }
                        }
                    }
                }
            }
        }
    }
    pub(super) fn reconcile_archive(&self, workspace: &mut WorkspaceSnapshot) {
        for p in &mut workspace.projects {
            for s in &mut p.sessions {
                if let Some(intent) = self
                    .session_actions
                    .archives
                    .get(&(p.path.clone(), s.id.clone()))
                {
                    s.archived = intent.wanted;
                    if s.archived {
                        s.selected = false;
                    }
                }
            }
        }
    }
    pub(super) fn finish_archive(
        &mut self,
        project: String,
        id: String,
        revision: u64,
        archived: bool,
        success: bool,
    ) {
        let Some(intent) = self
            .session_actions
            .archives
            .get_mut(&(project.clone(), id.clone()))
        else {
            return;
        };
        // The worker executes in order: even an older success establishes durable truth.
        if success {
            intent.confirmed = archived;
        }
        if intent.revision != revision {
            return;
        }
        if !success {
            intent.wanted = intent.confirmed;
            let restored = intent.confirmed;
            self.set_archived(&project, &id, restored);
            self.session_actions.errors.insert(
                format!("{project}\0{id}"),
                if archived {
                    "The session could not be archived. Try again.".into()
                } else {
                    "The session could not be restored. Try again.".into()
                },
            );
        }
    }
    pub(super) fn sync_session_name(
        &mut self,
        runtime: &str,
        generation: u64,
        identity: Identity,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_state.invalidate_metadata();
        if self
            .saved_session(runtime)
            .is_some_and(|(_, session, _)| session.title == identity.name)
            && !self.session_actions.errors.contains_key(runtime)
        {
            return;
        }
        self.store_session_name(runtime, generation, identity, cx);
    }
    pub(super) fn store_session_name(
        &mut self,
        runtime: &str,
        generation: u64,
        identity: Identity,
        cx: &mut Context<Self>,
    ) {
        let Some((project, session, _)) = self.saved_session(runtime) else {
            return;
        };
        if session.id != identity.id || session.path != identity.path || identity.name.is_empty() {
            return;
        }
        self.session_actions.revision += 1;
        let revision = self.session_actions.revision;
        self.session_actions.names.insert(runtime.into(), revision);
        self.dispatch(
            Command::StoreName {
                runtime: runtime.into(),
                generation,
                revision,
                project: project.path,
                id: identity.id,
                path: identity.path,
                name: identity.name,
            },
            cx,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_name(
        &mut self,
        runtime: String,
        generation: u64,
        revision: u64,
        id: String,
        path: String,
        name: String,
        success: bool,
    ) {
        let Some(live) = self.sessions.get_mut(&runtime) else {
            return;
        };
        let identity = &live.controller.state().identity;
        if self.session_actions.names.get(&runtime) != Some(&revision)
            || live.controller.generation() != generation
            || identity.id != id
            || identity.path != path
            || identity.name != name
        {
            return;
        }
        if success {
            self.session_actions.errors.remove(&runtime);
            if let Some(w) = &mut self.workspace {
                for p in &mut w.projects {
                    if p.path == live.project.path {
                        for s in &mut p.sessions {
                            if s.id == id && s.path == path {
                                s.title = name.clone();
                                s.title_markdown = Some(name.chars().take(240).collect());
                                s.title_is_derived = false;
                            }
                        }
                    }
                }
            }
            for s in &mut live.project.sessions {
                if s.id == id && s.path == path {
                    s.title = name.clone();
                    s.title_markdown = Some(name.chars().take(240).collect());
                    s.title_is_derived = false;
                }
            }
        } else {
            self.session_actions.errors.insert(
                runtime,
                "The session name could not be saved to the workspace. Try again.".into(),
            );
        }
        live.dirty = true;
        self.sidebar_state.invalidate_metadata();
    }
}

#[cfg(test)]
mod archive_control_tests;
#[cfg(test)]
mod callback_tests;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
mod tests;
