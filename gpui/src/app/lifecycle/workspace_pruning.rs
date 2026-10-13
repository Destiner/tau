use super::*;

impl State {
    pub(in crate::app) fn remove_project(&mut self, project: &str) {
        self.folds.remove(project);
        self.pending.retain(|_, mutation| match mutation {
            Mutation::Collapse { project: path, .. } | Mutation::Select { project: path, .. } => {
                path.as_str() != project
            }
            Mutation::Order(paths) => {
                paths.retain(|path| path != project);
                true
            }
        });
        if self
            .selection
            .as_ref()
            .is_some_and(|(path, _)| path == project)
        {
            self.selection = None;
        }
        if let Some(order) = &mut self.order {
            order.confirmed.retain(|path| path != project);
            order.desired.retain(|path| path != project);
        }
    }
}
