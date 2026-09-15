use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[cfg(not(dev))]
pub const APP_DIRECTORY_NAME: &str = "tau";

#[cfg(dev)]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau-dev.json";
#[cfg(not(dev))]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau.json";

/// The OTel `deployment.environment.name` resource attribute value.
#[cfg(dev)]
pub const ENVIRONMENT_NAME: &str = "development";
#[cfg(not(dev))]
pub const ENVIRONMENT_NAME: &str = "production";

/// Storage lifetime and locations are independent of workspace mutations and
/// Pi configuration. Pi still owns transcripts, so both profiles use files.
pub struct StorageProfile {
    data_dir: PathBuf,
    session_root: Option<PathBuf>,
    #[cfg(any(dev, test))]
    temporary: Option<tempfile::TempDir>,
}

impl StorageProfile {
    #[cfg(any(not(dev), test))]
    fn persistent(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            session_root: None,
            #[cfg(any(dev, test))]
            temporary: None,
        }
    }

    #[cfg(any(dev, test))]
    pub(crate) fn ephemeral() -> Result<Self, String> {
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

    #[cfg(any(dev, test))]
    pub fn temporary_root(&self) -> Option<&Path> {
        self.temporary.as_ref().map(tempfile::TempDir::path)
    }
}

static PROFILE: LazyLock<Result<StorageProfile, String>> = LazyLock::new(|| {
    #[cfg(dev)]
    {
        StorageProfile::ephemeral()
    }
    #[cfg(not(dev))]
    {
        dirs::data_dir()
            .map(|path| StorageProfile::persistent(path.join(APP_DIRECTORY_NAME)))
            .ok_or_else(|| "Could not locate the application data folder.".into())
    }
});

pub fn current() -> Result<&'static StorageProfile, String> {
    PROFILE.as_ref().map_err(Clone::clone)
}

pub fn app_data_dir() -> PathBuf {
    // Startup validates the profile before diagnostics or commands can use it.
    current()
        .expect("storage profile must be initialized")
        .data_dir
        .clone()
}

pub fn initialize() -> Result<(), String> {
    let _profile = current()?;
    #[cfg(dev)]
    {
        crate::dev_workspace::seed(_profile)?;
        eprintln!(
            "Disposable development storage: {}",
            _profile.data_dir().display()
        );
    }
    Ok(())
}

pub fn cleanup() {
    #[cfg(dev)]
    if let Ok(profile) = current() {
        if let Some(root) = profile.temporary_root() {
            // Tauri exits without dropping statics. All Pi writers and telemetry
            // must be stopped before removing this run's disposable files.
            if let Err(error) = std::fs::remove_dir_all(root) {
                eprintln!("Could not remove temporary development storage: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_keeps_existing_paths_and_pi_sessions() {
        let profile = StorageProfile::persistent(PathBuf::from("/app-data/tau"));
        assert_eq!(profile.data_dir(), Path::new("/app-data/tau"));
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
