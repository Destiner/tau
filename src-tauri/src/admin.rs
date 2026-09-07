//! Admin mode: the hidden switch behind Tau's diagnostics. It is off in a
//! normal run, which is why telemetry records nothing and the issue
//! reporter is not offered; the frontend cheat code turns it on, and this
//! module persists that choice beside the data it gates so the *next* run
//! knows before it records its first line.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::profile::APP_DIRECTORY_NAME;
use crate::telemetry::{trace_context::TraceContext, Telemetry};

const PREFERENCES_FILE: &str = "preferences.json";
const WRITE_ERROR: &str = "Admin mode could not be saved.";

/// Tau's persisted preferences. `default` on both the container and its
/// fields keeps an older or hand-edited file readable: anything missing or
/// unparsable simply leaves admin mode off, which is the safe direction.
#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Preferences {
    admin_mode: bool,
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
    write_admin_mode_in(&resolve_preferences_dir(), enabled)
}

fn resolve_preferences_dir() -> PathBuf {
    let data_dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
    preferences_dir(&data_dir, APP_DIRECTORY_NAME)
}

fn preferences_dir(data_dir: &Path, app_directory_name: &str) -> PathBuf {
    data_dir.join(app_directory_name)
}

fn read_admin_mode_in(dir: &Path) -> bool {
    fs::read_to_string(dir.join(PREFERENCES_FILE))
        .ok()
        .and_then(|contents| serde_json::from_str::<Preferences>(&contents).ok())
        .unwrap_or_default()
        .admin_mode
}

/// Rewrites the whole preferences file, preserving nothing but the fields
/// `Preferences` knows about — there is exactly one setting today, and a
/// partial update would need a read-modify-write race this switch does not
/// justify.
fn write_admin_mode_in(dir: &Path, enabled: bool) -> Result<(), String> {
    let contents = serde_json::to_vec(&Preferences {
        admin_mode: enabled,
    })
    .map_err(|_| WRITE_ERROR.to_string())?;
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
            preferences_dir(Path::new("/data"), "tau"),
            Path::new("/data/tau")
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
