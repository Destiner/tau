use crate::{ssh::run_remote_command, storage::pi_agent_dir};
use serde::Deserialize;
use std::fs;

/// Prints Pi's global settings, which is where `/scoped-models` stores the
/// model scope. A missing file simply means nothing is scoped. The marker
/// separates the settings from whatever the remote login shell greets us with.
const SETTINGS_MARKER: &str = "TAU_PI_SETTINGS";
const REMOTE_SETTINGS_COMMAND: &str = concat!(
    r#"exec "${SHELL:-/bin/sh}" -lc 'printf "\nTAU_PI_SETTINGS\n"; "#,
    r#"cat "${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}/settings.json" 2>/dev/null || true'"#
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiSettings {
    enabled_models: Option<Vec<String>>,
}

#[tauri::command]
pub fn read_model_scope() -> Result<Vec<String>, String> {
    let path = pi_agent_dir()?.join("settings.json");
    Ok(fs::read_to_string(path)
        .map(|settings| parse_model_scope(&settings))
        .unwrap_or_default())
}

#[tauri::command]
pub async fn read_remote_model_scope(connection_string: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output = run_remote_command(&connection_string, REMOTE_SETTINGS_COMMAND)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let settings = stdout
            .rsplit_once(SETTINGS_MARKER)
            .map_or(stdout.as_ref(), |(_, settings)| settings);
        Ok(parse_model_scope(settings))
    })
    .await
    .map_err(|error| format!("Could not read the remote Pi settings: {error}"))?
}

fn parse_model_scope(settings: &str) -> Vec<String> {
    serde_json::from_str::<PiSettings>(settings)
        .ok()
        .and_then(|settings| settings.enabled_models)
        .unwrap_or_default()
        .into_iter()
        .map(|pattern| pattern.trim().to_string())
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_model_scope, REMOTE_SETTINGS_COMMAND, SETTINGS_MARKER};

    #[test]
    fn the_remote_command_prints_the_settings_marker() {
        assert!(REMOTE_SETTINGS_COMMAND.contains(SETTINGS_MARKER));
    }

    #[test]
    fn reads_the_configured_model_scope() {
        let settings = r#"{"theme":"dark","enabledModels":["openai/gpt-5.5"," anthropic/* ",""]}"#;
        assert_eq!(
            parse_model_scope(settings),
            vec!["openai/gpt-5.5".to_string(), "anthropic/*".to_string()]
        );
    }

    #[test]
    fn treats_missing_or_invalid_settings_as_unscoped() {
        assert!(parse_model_scope("").is_empty());
        assert!(parse_model_scope("{").is_empty());
        assert!(parse_model_scope(r#"{"theme":"dark"}"#).is_empty());
        assert!(parse_model_scope(r#"{"enabledModels":null}"#).is_empty());
    }
}
