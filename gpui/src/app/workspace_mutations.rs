use super::*;
use tau_core::models::SessionSummary;
#[path = "lifecycle/workspace_pruning.rs"]
mod lifecycle_pruning;

#[derive(Clone, Debug)]
pub enum Mutation {
    Collapse { project: String, collapsed: bool },
    Order(Vec<String>),
    Select { project: String, session: String },
}

#[derive(Default)]
struct Intent<T> {
    revision: u64,
    confirmed_revision: u64,
    confirmed: T,
    desired: T,
    rolled_back: bool,
}
impl<T: Clone> Intent<T> {
    fn begin(&mut self, revision: u64, desired: T) {
        self.revision = revision;
        self.desired = desired;
        self.rolled_back = false;
    }
    fn finish(&mut self, revision: u64, value: T, success: bool) {
        if success && revision > self.confirmed_revision {
            self.confirmed_revision = revision;
            self.confirmed = value;
            if self.rolled_back {
                self.desired = self.confirmed.clone();
            }
        }
        if revision == self.revision && !success {
            self.desired = self.confirmed.clone();
            self.rolled_back = true;
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    revision: u64,
    pending: BTreeMap<u64, Mutation>,
    folds: BTreeMap<String, Intent<bool>>,
    order: Option<Intent<Vec<String>>>,
    selection: Option<(String, String)>,
}
impl State {
    fn apply(&self, workspace: &mut WorkspaceSnapshot) {
        for project in &mut workspace.projects {
            if let Some(intent) = self.folds.get(&project.path) {
                project.collapsed = intent.desired;
            }
            if let Some((path, id)) = &self.selection {
                project.selected = project.path == *path;
                if project.selected {
                    for session in &mut project.sessions {
                        session.selected = session.id == *id && !session.archived;
                    }
                }
            }
        }
        if let Some((path, _)) = &self.selection {
            workspace.active_project_path = path.clone();
        }
        if let Some(order) = &self.order {
            // Unknown projects from an import stay in their relative order.
            let ranks: std::collections::HashMap<_, _> = order
                .desired
                .iter()
                .enumerate()
                .map(|(rank, path)| (path.as_str(), rank))
                .collect();
            workspace.projects.sort_by_key(|project| {
                ranks
                    .get(project.path.as_str())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
    }
}

impl Shell {
    pub(super) fn initialize_workspace_session(
        &mut self,
        project: &ProjectSummary,
        source_path: &str,
        cx: &mut Context<Self>,
    ) {
        if !source_path.is_empty() {
            return;
        }
        let collapsed = self
            .workspace
            .as_ref()
            .and_then(|workspace| {
                workspace
                    .projects
                    .iter()
                    .find(|row| row.path == project.path)
            })
            .is_some_and(|row| row.collapsed);
        if collapsed {
            self.mutate_workspace(
                Mutation::Collapse {
                    project: project.path.clone(),
                    collapsed: false,
                },
                cx,
            );
        }
        self.select_workspace(project.path.clone(), String::new(), cx);
    }

    pub(super) fn mutate_workspace(&mut self, mutation: Mutation, cx: &mut Context<Self>) {
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        self.workspace_mutations.revision += 1;
        let revision = self.workspace_mutations.revision;
        let state = &mut self.workspace_mutations;
        match &mutation {
            Mutation::Collapse { project, collapsed } => {
                let Some(row) = workspace.projects.iter().find(|row| row.path == *project) else {
                    return;
                };
                let intent = state
                    .folds
                    .entry(project.clone())
                    .or_insert_with(|| Intent {
                        confirmed: row.collapsed,
                        desired: row.collapsed,
                        ..Default::default()
                    });
                intent.begin(revision, *collapsed);
            }
            Mutation::Order(paths) => {
                let unique: std::collections::HashSet<_> =
                    paths.iter().map(String::as_str).collect();
                if paths.len() != workspace.projects.len()
                    || unique.len() != paths.len()
                    || workspace
                        .projects
                        .iter()
                        .any(|row| !unique.contains(row.path.as_str()))
                {
                    return;
                }
                let intent = state.order.get_or_insert_with(|| {
                    let order = workspace
                        .projects
                        .iter()
                        .map(|row| row.path.clone())
                        .collect::<Vec<_>>();
                    Intent {
                        confirmed: order.clone(),
                        desired: order,
                        ..Default::default()
                    }
                });
                intent.begin(revision, paths.clone());
            }
            Mutation::Select { project, session } => {
                // Storage errors never navigate away from a retained editor or its draft.
                state.selection = Some((project.clone(), session.clone()));
            }
        }
        state.pending.insert(revision, mutation.clone());
        state.apply(workspace);
        self.sidebar_state.invalidate_metadata();
        if self
            .commands
            .try_send(Command::WorkspaceMutation { revision, mutation })
            .is_err()
        {
            self.finish_workspace_mutation(revision, false);
        }
        cx.notify();
    }

    pub(super) fn select_workspace(
        &mut self,
        project: String,
        session: String,
        cx: &mut Context<Self>,
    ) {
        self.mutate_workspace(Mutation::Select { project, session }, cx);
    }

    pub(super) fn finish_workspace_mutation(&mut self, revision: u64, success: bool) {
        let state = &mut self.workspace_mutations;
        let Some(mutation) = state.pending.remove(&revision) else {
            if revision == 0 && !success {
                self.error = Some("Workspace changes could not be saved. Try again.".into());
            }
            return;
        };
        match mutation {
            Mutation::Collapse { project, collapsed } => {
                if let Some(intent) = state.folds.get_mut(&project) {
                    intent.finish(revision, collapsed, success);
                }
            }
            Mutation::Order(paths) => {
                if let Some(intent) = &mut state.order {
                    intent.finish(revision, paths, success);
                }
            }
            Mutation::Select { .. } => {}
        }
        if let Some(workspace) = &mut self.workspace {
            state.apply(workspace);
        }
        self.sidebar_state.invalidate_metadata();
        if !success {
            self.error = Some("Workspace changes could not be saved. Try again.".into());
        }
    }

    pub(super) fn reconcile_workspace_mutations(&self, workspace: &mut WorkspaceSnapshot) {
        self.workspace_mutations.apply(workspace);
    }

    pub(super) fn merge_workspace_project(&mut self, project: ProjectSummary) {
        let Some(workspace) = &mut self.workspace else {
            return;
        };
        if project.selected {
            workspace.active_project_path = project.path.clone();
            for existing in &mut workspace.projects {
                existing.selected = false;
            }
        }
        if let Some(existing) = workspace
            .projects
            .iter_mut()
            .find(|existing| existing.path == project.path)
        {
            *existing = project;
        } else {
            workspace.projects.push(project);
        }
        self.workspace_mutations.apply(workspace);
        self.sidebar_state.invalidate_metadata();
    }

    pub(super) fn merge_registered_row(&mut self, project: String, row: SessionSummary) {
        if let Some(workspace) = &mut self.workspace {
            if let Some(project) = workspace
                .projects
                .iter_mut()
                .find(|item| item.path == project)
            {
                if let Some(existing) = project.sessions.iter_mut().find(|item| item.id == row.id) {
                    *existing = row;
                } else {
                    project.sessions.insert(0, row);
                }
            }
            self.workspace_mutations.apply(workspace);
        }
        // Archive intent owns visibility even if registration completed late.
        if let Some(mut workspace) = self.workspace.take() {
            self.reconcile_archive(&mut workspace);
            self.workspace = Some(workspace);
        }
        self.sidebar_state.invalidate_metadata();
    }
}

#[cfg(test)]
#[path = "workspace_mutations_tests.rs"]
mod tests;
