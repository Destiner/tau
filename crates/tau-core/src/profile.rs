use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const APP_DIRECTORY_NAME: &str = "tau";
pub const PRODUCTION_SESSION_REGISTRY_FILENAME: &str = ".tau.json";
pub const ISOLATED_SESSION_REGISTRY_FILENAME: &str = ".tau-dev.json";

pub struct StorageProfile {
    data_dir: PathBuf,
    session_root: Option<PathBuf>,
    temporary: Option<tempfile::TempDir>,
}

impl StorageProfile {
    pub fn production() -> Result<Self, String> {
        dirs::data_dir()
            .map(|path| Self::persistent(path.join(APP_DIRECTORY_NAME)))
            .ok_or_else(|| "Could not locate the application data folder.".into())
    }

    pub fn persistent(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            session_root: None,
            temporary: None,
        }
    }

    /// Explicit shell-owned isolation; does not seed projects or inspect production data.
    pub fn isolated(data_dir: PathBuf, session_root: PathBuf) -> Self {
        Self {
            data_dir,
            session_root: Some(session_root),
            temporary: None,
        }
    }

    pub fn ephemeral() -> Result<Self, String> {
        let temporary = tempfile::Builder::new()
            .prefix("tau-dev-")
            .tempdir()
            .map_err(|error| format!("Could not create temporary development storage: {error}"))?;
        // macOS /var is a symlink; Pi and Tau must agree on project identities.
        let root = temporary
            .path()
            .canonicalize()
            .map_err(|error| format!("Could not resolve temporary development storage: {error}"))?;
        Ok(Self {
            data_dir: root.join("tau"),
            session_root: Some(root.join("sessions")),
            temporary: Some(temporary),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn session_dir(&self, project_path: &str, pi_agent_dir: &Path) -> PathBuf {
        let root = self
            .session_root
            .clone()
            .unwrap_or_else(|| pi_agent_dir.join("sessions"));
        let safe_path = project_path
            .trim_start_matches(['/', '\\'])
            .replace(['/', '\\', ':'], "-");
        root.join(format!("--{safe_path}--"))
    }

    pub fn session_registry_filename(&self) -> &'static str {
        if self.session_root.is_some() {
            ISOLATED_SESSION_REGISTRY_FILENAME
        } else {
            PRODUCTION_SESSION_REGISTRY_FILENAME
        }
    }

    pub fn temporary_root(&self) -> Option<&Path> {
        self.temporary.as_ref().map(tempfile::TempDir::path)
    }
}

static PROFILE: OnceLock<Result<StorageProfile, String>> = OnceLock::new();

/// Must run before any service. A profile can never be replaced beneath live workers.
pub fn initialize(profile: StorageProfile) -> Result<(), String> {
    PROFILE
        .set(Ok(profile))
        .map_err(|_| "Tau storage profile is already initialized.".to_string())
}

pub fn current() -> Result<&'static StorageProfile, String> {
    PROFILE
        .get()
        .ok_or_else(|| "Tau storage profile is not initialized.".to_string())?
        .as_ref()
        .map_err(Clone::clone)
}

pub fn session_registry_filename() -> Result<&'static str, String> {
    Ok(current()?.session_registry_filename())
}

pub fn app_data_dir() -> PathBuf {
    current()
        .expect("storage profile must be initialized")
        .data_dir()
        .to_path_buf()
}

#[cfg(test)]
pub(crate) fn initialize_test_profile() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| {
        initialize(StorageProfile::ephemeral().expect("isolated test profile"))
            .expect("initialize test profile");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_paths_and_registry_are_selected_by_profile() {
        let root = tempfile::tempdir().unwrap();
        let profile =
            StorageProfile::isolated(root.path().join("data"), root.path().join("sessions"));
        assert_eq!(
            profile.session_registry_filename(),
            ISOLATED_SESSION_REGISTRY_FILENAME
        );
        assert_eq!(
            profile.session_dir("/work/project", Path::new("/unused/pi")),
            root.path().join("sessions/--work-project--")
        );
        assert!(profile.temporary_root().is_none());
        assert!(!profile.data_dir().exists());
    }

    #[test]
    fn production_keeps_existing_paths_and_pi_sessions() {
        let profile = StorageProfile::persistent(PathBuf::from("/app-data/tau"));
        assert_eq!(profile.data_dir(), Path::new("/app-data/tau"));
        assert_eq!(
            profile.session_registry_filename(),
            PRODUCTION_SESSION_REGISTRY_FILENAME
        );
        assert_eq!(
            profile.session_dir("/Users/timur/code/tau", Path::new("/custom/pi")),
            PathBuf::from("/custom/pi/sessions/--Users-timur-code-tau--")
        );
        assert!(profile.temporary_root().is_none());
    }

    #[test]
    fn dev_instances_isolate_every_project_and_drop_their_own_files() {
        let first = StorageProfile::ephemeral().expect("first profile");
        let second = StorageProfile::ephemeral().expect("second profile");
        let first_root = first.temporary_root().unwrap().to_path_buf();
        let second_root = second.temporary_root().unwrap().to_path_buf();
        let project = "/same/imported/project";
        let pi = Path::new("/real/pi");
        assert_ne!(first.data_dir(), second.data_dir());
        assert_ne!(
            first.session_dir(project, pi),
            second.session_dir(project, pi)
        );
        assert!(!first.session_dir(project, pi).starts_with(pi));
        assert_ne!(
            first.session_dir(project, pi),
            first.session_dir("/different/project", pi)
        );
        std::fs::create_dir_all(first.data_dir()).unwrap();
        std::fs::write(first.data_dir().join("preferences.json"), "changed").unwrap();
        assert!(!second.data_dir().join("preferences.json").exists());
        drop(first);
        assert!(!first_root.exists());
        assert!(second_root.exists());
        drop(second);
        assert!(!second_root.exists());
    }
}
