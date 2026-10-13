use super::*;

impl SessionActions {
    pub(in crate::app) fn remove_project(&mut self, project: &str) {
        self.archives.retain(|(path, _), _| path != project);
        let prefix = format!("{project}\0");
        self.errors.retain(|key, _| !key.starts_with(&prefix));
    }
}
