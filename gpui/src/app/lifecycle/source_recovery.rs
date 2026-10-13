use super::*;

impl Shell {
    pub(in crate::app) fn source_recoveries(&mut self, cx: &mut Context<Self>) {
        let pending: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(key, live)| {
                let original = live.controller.source_recovery()?.clone();
                if self
                    .runtime_lifecycle
                    .recoveries
                    .values()
                    .any(|op| op.runtime == *key)
                {
                    return None;
                }
                // Typed bootstrap provenance plus the exact stale registry row; copy is never evidence.
                if !self.workspace.as_ref().is_some_and(|workspace| {
                    workspace.projects.iter().any(|project| {
                        project.path == live.project.path
                            && project
                                .sessions
                                .iter()
                                .any(|row| row.id == original.id && row.path == original.path)
                    })
                }) {
                    return None;
                }
                if original.id != live.source_id
                    && live.source_id != live.controller.state().identity.id
                {
                    return None;
                }
                Some((
                    key.clone(),
                    live.project.path.clone(),
                    live.controller.generation(),
                    original,
                ))
            })
            .collect();
        for (runtime, project, generation, original) in pending {
            self.runtime_lifecycle.sequence += 1;
            let revision = self.runtime_lifecycle.sequence;
            if let Some(live) = self.sessions.get_mut(&runtime) {
                live.ephemeral = true;
                live.source_id = live.controller.state().identity.id.clone();
                live.source_path = live.controller.state().identity.path.clone();
            }
            self.runtime_lifecycle.recoveries.insert(
                (project.clone(), original.id.clone(), revision),
                Recovery {
                    runtime,
                    generation,
                    original: original.clone(),
                    failed: false,
                },
            );
            self.dispatch(
                Command::ArchiveSource {
                    project,
                    id: original.id,
                    path: original.path,
                    token: revision,
                },
                cx,
            );
        }
    }
    pub(in crate::app) fn finish_source_recovery(
        &mut self,
        project: &str,
        id: &str,
        path: &str,
        revision: u64,
        success: bool,
        _cx: &mut Context<Self>,
    ) -> bool {
        let token = (project.into(), id.into(), revision);
        let Some(op) = self
            .runtime_lifecycle
            .recoveries
            .get_mut(&token)
            .filter(|op| op.original.path == path)
        else {
            return false;
        };
        if !success {
            op.failed = true;
            if self.active == op.runtime {
                self.error = Some("The stale session could not be archived. Retry; your unsent source is retained.".into());
            }
            return true;
        }
        let op = self.runtime_lifecycle.recoveries.remove(&token).unwrap();
        if let Some(workspace) = &mut self.workspace
            && let Some(row) = workspace
                .projects
                .iter_mut()
                .find(|p| p.path == project)
                .and_then(|p| {
                    p.sessions
                        .iter_mut()
                        .find(|s| s.id == id && s.path == op.original.path)
                })
        {
            row.archived = true;
        }
        if let Some(live) = self.sessions.get_mut(&op.runtime)
            && live.controller.generation() == op.generation
        {
            live.controller.acknowledge_source_recovery(&op.original);
        }
        self.sidebar_state.invalidate_metadata();
        true
    }
}
