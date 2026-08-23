use crate::{
    ssh::{remote_login_shell_command, run_remote_command},
    storage::pi_agent_dir,
    telemetry::{trace_context::TraceContext, Telemetry},
};
use serde::Deserialize;
use std::fs;
use tauri::State;

/// Prints Pi's global settings, which is where `/scoped-models` stores the
/// model scope. A missing file simply means nothing is scoped. The marker
/// separates the settings from whatever the remote login shell greets us with.
const SETTINGS_MARKER: &str = "TAU_PI_SETTINGS";
const SETTINGS_END_MARKER: &str = "TAU_PI_END_SETTINGS";
const REMOTE_SETTINGS_INNER_COMMAND: &str = concat!(
    r#"printf "\nTAU_PI_SETTINGS\n"; "#,
    r#"cat "${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}/settings.json" 2>/dev/null || true; "#,
    r#"printf "\nTAU_PI_END_SETTINGS\n""#
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiSettings {
    enabled_models: Option<Vec<String>>,
}

#[tauri::command]
pub fn read_model_scope(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<Vec<String>, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_model_scope"));
    let path = pi_agent_dir()?.join("settings.json");
    Ok(fs::read_to_string(path)
        .map(|settings| parse_model_scope(&settings))
        .unwrap_or_default())
}

#[tauri::command]
pub async fn read_remote_model_scope(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
) -> Result<Vec<String>, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_remote_model_scope"));
    tauri::async_runtime::spawn_blocking(move || {
        let command = remote_settings_command();
        let output = run_remote_command(&connection_string, &command)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_model_scope(settings_between_markers(&stdout)))
    })
    .await
    .map_err(|error| format!("Could not read the remote Pi settings: {error}"))?
}

fn remote_settings_command() -> String {
    let command = format!(
        "exec /bin/sh -c {}",
        shell_words::quote(REMOTE_SETTINGS_INNER_COMMAND),
    );
    remote_login_shell_command(&command)
}

fn settings_between_markers(output: &str) -> &str {
    output
        .rsplit_once(SETTINGS_MARKER)
        .and_then(|(_, settings)| settings.split_once(SETTINGS_END_MARKER))
        .map_or("", |(settings, _)| settings)
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
    use super::{
        parse_model_scope, remote_settings_command, settings_between_markers,
        REMOTE_SETTINGS_INNER_COMMAND, SETTINGS_END_MARKER, SETTINGS_MARKER,
    };

    #[test]
    fn the_remote_command_bounds_settings_between_markers() {
        assert!(REMOTE_SETTINGS_INNER_COMMAND.contains(SETTINGS_MARKER));
        assert!(REMOTE_SETTINGS_INNER_COMMAND.contains(SETTINGS_END_MARKER));
    }

    #[test]
    fn reads_settings_through_a_csh_remote_environment() {
        let shell = std::path::Path::new("/bin/csh");
        if !shell.is_file() {
            return;
        }
        let directory = tempfile::tempdir().expect("Pi agent directory");
        std::fs::write(
            directory.path().join("settings.json"),
            r#"{"enabledModels":["openai/*"]}"#,
        )
        .expect("Pi settings");
        let output = std::process::Command::new(shell)
            .args(["-c", &remote_settings_command()])
            .env("SHELL", shell)
            .env("PI_CODING_AGENT_DIR", directory.path())
            .output()
            .expect("csh remote settings fixture");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            parse_model_scope(settings_between_markers(&stdout)),
            ["openai/*"],
        );
    }

    #[test]
    fn ignores_remote_shell_output_outside_the_settings_markers() {
        let output = format!(
            "login banner\n{SETTINGS_MARKER}\n{{\"enabledModels\":[\"openai/*\"]}}\n{SETTINGS_END_MARKER}\nlogout banner"
        );
        assert_eq!(
            settings_between_markers(&output).trim(),
            r#"{"enabledModels":["openai/*"]}"#,
        );
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
