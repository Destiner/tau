//! Admin mode: the hidden switch behind Tau's diagnostics. It is off in a
//! normal run, which is why telemetry records nothing and the issue
//! reporter is not offered; the frontend cheat code turns it on, and this
//! module persists that choice beside the data it gates so the *next* run
//! knows before it records its first line.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::profile;
use crate::telemetry::{trace_context::TraceContext, Telemetry};

const PREFERENCES_FILE: &str = "preferences.json";
const WRITE_ERROR: &str = "Preferences could not be saved.";
const MAX_DISMISSED_VERSION_LEN: usize = 128;

/// Tau's persisted preferences. `default` on both the container and its
/// fields keeps an older or hand-edited file readable: anything missing or
/// unparsable simply leaves admin mode off, which is the safe direction.
#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Preferences {
    admin_mode: bool,
    dismissed_update_version: Option<String>,
}

/// Reads the persisted setting straight from disk. Called before the Tauri
/// builder exists, so a run starts already knowing whether telemetry may
/// record anything at all.
pub fn admin_mode_enabled() -> bool {
    read_admin_mode_in(&resolve_preferences_dir())
}

#[tauri::command]
pub fn read_admin_mode(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> bool {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_admin_mode"));
    admin_mode_enabled()
}

/// Applies the switch to this run before persisting it, so telemetry starts
/// or stops immediately and a failed write costs the preference, not the
/// behaviour the user just asked for.
#[tauri::command]
pub fn set_admin_mode(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    enabled: bool,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "set_admin_mode"));
    telemetry.set_enabled(enabled);
    let dir = resolve_preferences_dir();
    let mut preferences = read_preferences_in(&dir);
    preferences.admin_mode = enabled;
    write_preferences_in(&dir, &preferences)
}

#[tauri::command]
pub fn get_dismissed_update_version() -> Option<String> {
    read_preferences_in(&resolve_preferences_dir()).dismissed_update_version
}

#[tauri::command]
pub fn set_dismissed_update_version(version: Option<String>) -> Result<(), String> {
    let version = version
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if version
        .as_ref()
        .is_some_and(|value| value.len() > MAX_DISMISSED_VERSION_LEN)
    {
        return Err(WRITE_ERROR.to_string());
    }

    let dir = resolve_preferences_dir();
    let mut preferences = read_preferences_in(&dir);
    preferences.dismissed_update_version = version;
    write_preferences_in(&dir, &preferences)
}

fn resolve_preferences_dir() -> PathBuf {
    profile::app_data_dir()
}

fn read_preferences_in(dir: &Path) -> Preferences {
    fs::read_to_string(dir.join(PREFERENCES_FILE))
        .ok()
        .and_then(|contents| serde_json::from_str::<Preferences>(&contents).ok())
        .unwrap_or_default()
}

fn read_admin_mode_in(dir: &Path) -> bool {
    read_preferences_in(dir).admin_mode
}

#[cfg(test)]
fn write_admin_mode_in(dir: &Path, enabled: bool) -> Result<(), String> {
    let mut preferences = read_preferences_in(dir);
    preferences.admin_mode = enabled;
    write_preferences_in(dir, &preferences)
}

fn write_preferences_in(dir: &Path, preferences: &Preferences) -> Result<(), String> {
    let contents = serde_json::to_vec(preferences).map_err(|_| WRITE_ERROR.to_string())?;
    fs::create_dir_all(dir).map_err(|_| WRITE_ERROR.to_string())?;
    let path = dir.join(PREFERENCES_FILE);
    fs::write(&path, contents).map_err(|_| WRITE_ERROR.to_string())?;
    set_owner_only_file_permissions(&path);
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only_file_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_mode_is_off_until_it_has_been_written() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(!read_admin_mode_in(directory.path()));

        write_admin_mode_in(directory.path(), true).expect("enable admin mode");
        assert!(read_admin_mode_in(directory.path()));

        write_admin_mode_in(directory.path(), false).expect("disable admin mode");
        assert!(!read_admin_mode_in(directory.path()));
    }

    #[test]
    fn preference_updates_preserve_the_other_setting() {
        let directory = tempfile::tempdir().expect("temporary directory");
        write_preferences_in(
            directory.path(),
            &Preferences {
                admin_mode: true,
                dismissed_update_version: Some("2.0.0".into()),
            },
        )
        .expect("write preferences");

        write_admin_mode_in(directory.path(), false).expect("disable admin mode");
        let preferences = read_preferences_in(directory.path());
        assert!(!preferences.admin_mode);
        assert_eq!(
            preferences.dismissed_update_version.as_deref(),
            Some("2.0.0")
        );

        let mut preferences = read_preferences_in(directory.path());
        preferences.dismissed_update_version = Some("2.1.0".into());
        write_preferences_in(directory.path(), &preferences).expect("dismiss update");
        assert!(!read_admin_mode_in(directory.path()));
    }

    #[test]
    fn an_unreadable_or_unexpected_preferences_file_leaves_admin_mode_off() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join(PREFERENCES_FILE);

        fs::write(&path, "{").expect("truncated preferences");
        assert!(!read_admin_mode_in(directory.path()));

        fs::write(&path, r#"{"theme":"dark"}"#).expect("unrelated preferences");
        assert!(!read_admin_mode_in(directory.path()));

        fs::write(&path, r#"{"adminMode":"yes"}"#).expect("mistyped preferences");
        assert!(!read_admin_mode_in(directory.path()));
    }

    #[test]
    fn preferences_live_beside_the_telemetry_and_feedback_directories() {
        assert_eq!(
            resolve_preferences_dir(),
            profile::current().unwrap().data_dir()
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_preferences_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().expect("temporary directory");
        let directory = parent.path().join("tau");
        write_admin_mode_in(&directory, true).expect("enable admin mode");

        let mode = fs::metadata(directory.join(PREFERENCES_FILE))
            .expect("preferences file")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
