use crate::{
    models::{ProjectRegistry, RemoteProjectRecord},
    pi::login_shell_pi_agent_dir,
    profile::{self, SESSION_REGISTRY_FILENAME},
};
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, MutexGuard},
};

static STORAGE_WRITE_LOCK: Mutex<()> = Mutex::new(());
static ARCHIVE_INTENTS: LazyLock<Mutex<HashMap<(String, String), u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn archive_revision(project: &str, session: &str) -> Result<u64, String> {
    ARCHIVE_INTENTS
        .lock()
        .map(|intents| {
            intents
                .get(&(project.to_owned(), session.to_owned()))
                .copied()
                .unwrap_or(0)
        })
        .map_err(|_| "Tau storage is unavailable.".to_string())
}

pub(super) fn adoption_is_current(
    project: &str,
    session: &str,
    revision: u64,
) -> Result<bool, String> {
    Ok(archive_revision(project, session)? == revision)
}

pub(super) fn mark_archive_intent(project: &str, session: &str) -> Result<(), String> {
    let mut intents = ARCHIVE_INTENTS
        .lock()
        .map_err(|_| "Tau storage is unavailable.".to_string())?;
    let revision = intents
        .entry((project.to_owned(), session.to_owned()))
        .or_default();
    *revision = revision.wrapping_add(1);
    Ok(())
}

pub(super) fn mutate_projects(
    mutation: impl FnOnce(&mut ProjectRegistry) -> Result<(), String>,
) -> Result<ProjectRegistry, String> {
    let _write_guard = lock_storage_writes()?;
    let mut registry = load_project_registry()?;
    let before = serde_json::to_vec(&registry).map_err(|error| error.to_string())?;
    mutation(&mut registry)?;
    if serde_json::to_vec(&registry).map_err(|error| error.to_string())? != before {
        write_json_atomic(&project_registry_path()?, &registry)?;
    }
    Ok(registry)
}

pub(super) fn lock_storage_writes() -> Result<MutexGuard<'static, ()>, String> {
    STORAGE_WRITE_LOCK
        .lock()
        .map_err(|_| "Tau storage is unavailable.".to_string())
}

pub(super) fn project_registry_path() -> Result<PathBuf, String> {
    Ok(profile::current()?.data_dir().join("projects.json"))
}

pub(crate) fn remote_project(project_path: &str) -> Result<RemoteProjectRecord, String> {
    load_project_registry()?
        .projects
        .into_iter()
        .find(|project| project.path == project_path)
        .and_then(|project| project.remote)
        .ok_or_else(|| "The remote project is no longer available.".to_string())
}

pub(super) fn load_project_registry() -> Result<ProjectRegistry, String> {
    read_json_or_default(&project_registry_path()?)
}
pub fn pi_agent_dir() -> Result<PathBuf, String> {
    if let Some(path) = configured_pi_agent_dir(std::env::var_os("PI_CODING_AGENT_DIR"), None) {
        return Ok(path);
    }
    if let Some(path) =
        configured_pi_agent_dir(None, login_shell_pi_agent_dir().map(OsString::as_os_str))
    {
        return Ok(path);
    }
    dirs::home_dir()
        .map(|path| path.join(".pi/agent"))
        .ok_or_else(|| "Could not locate the home folder.".into())
}

pub(super) fn configured_pi_agent_dir(
    inherited: Option<OsString>,
    login_shell: Option<&OsStr>,
) -> Option<PathBuf> {
    inherited
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            login_shell
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
}

pub(super) fn local_registry_path(project_path: &str) -> Result<PathBuf, String> {
    Ok(default_session_dir(project_path)?.join(SESSION_REGISTRY_FILENAME))
}

pub(crate) fn default_session_dir(project_path: &str) -> Result<PathBuf, String> {
    Ok(profile::current()?.session_dir(project_path, &pi_agent_dir()?))
}

pub(super) fn read_json_or_default<T: DeserializeOwned + Default>(
    path: &Path,
) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}

pub(super) fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "The settings path is invalid.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Could not encode Tau settings: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}
