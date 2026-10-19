use super::*;

impl State {
    pub(in crate::app) fn remove_runtime(&mut self, runtime: &str) {
        self.created.remove(runtime);
        self.runtime_identity.remove(runtime);
    }
    pub(in crate::app) fn remove_project(&mut self, project: &str) {
        self.unread.retain(|identity| identity.project != project);
        self.last_user
            .retain(|identity, _| identity.project != project);
        let retired: HashSet<_> = self
            .runtime_identity
            .iter()
            .filter(|(_, identity)| identity.project == project)
            .map(|(runtime, _)| runtime.clone())
            .collect();
        self.runtime_identity
            .retain(|_, identity| identity.project != project);
        self.created.retain(|runtime, _| !retired.contains(runtime));
        self.row_sources.clear();
        self.archive_source.clear();
        self.archive_rows.clear();
        self.archive_details.clear();
        self.archive_day = None;
        self.invalidate_metadata();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_project_read_state_does_not_leak_into_reimport_or_other_projects() {
        let removed = SessionIdentity::saved("removed", "s");
        let retained = SessionIdentity::saved("retained", "s");
        let mut state = State::default();
        state.unread.extend([removed.clone(), retained.clone()]);
        state.last_user.insert(removed.clone(), 10);
        state.last_user.insert(retained.clone(), 20);
        state.remove_project("removed");
        assert!(!state.unread.contains(&removed));
        assert!(!state.last_user.contains_key(&removed));
        assert!(state.unread.contains(&retained));
        assert_eq!(state.last_user[&retained], 20);
    }
}
