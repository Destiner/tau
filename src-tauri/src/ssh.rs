use crate::models::{RemoteDirectoryEntry, RemoteDirectoryListing};
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use tauri::State;

const SSH_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_OPTIONS: [&str; 8] = [
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ConnectionAttempts=1",
    "-o",
    "StrictHostKeyChecking=accept-new",
];
const DIRECTORY_MARKER: &[u8] = b"TAU_REMOTE_DIRECTORY";

pub struct SshConnection {
    executable: PathBuf,
    arguments: Vec<String>,
    connection_string: String,
}

impl SshConnection {
    pub fn parse(connection_string: &str) -> Result<Self, String> {
        let connection_string = connection_string.trim();
        if connection_string.is_empty() {
            return Err("Enter an SSH connection string.".into());
        }

        let mut words = shell_words::split(connection_string)
            .map_err(|error| format!("Could not parse the SSH connection string: {error}"))?;
        if words.is_empty() {
            return Err("Enter an SSH connection string.".into());
        }

        let first = Path::new(&words[0]);
        let starts_with_ssh = first.file_name() == Some(OsStr::new("ssh"));
        let executable = if starts_with_ssh {
            let executable = words.remove(0);
            if executable == "ssh" {
                resolve_ssh_binary()?
            } else {
                let path = PathBuf::from(executable);
                if !path.is_file() {
                    return Err(
                        "The SSH executable in the connection string does not exist.".into(),
                    );
                }
                path
            }
        } else {
            resolve_ssh_binary()?
        };

        if words.is_empty() {
            return Err("The SSH connection string does not include a destination.".into());
        }

        Ok(Self {
            executable,
            arguments: words,
            connection_string: connection_string.to_string(),
        })
    }

    pub fn command(&self, remote_command: &str) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .args(SSH_OPTIONS)
            .args(&self.arguments)
            .arg(remote_command);
        command
    }
}

#[tauri::command]
pub async fn probe_remote_project(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
) -> Result<RemoteDirectoryListing, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "probe_remote_project"));
    tauri::async_runtime::spawn_blocking(move || inspect_remote_directory(&connection_string, None))
        .await
        .map_err(|error| format!("Could not test the SSH connection: {error}"))?
}

#[tauri::command]
pub async fn list_remote_directories(
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    connection_string: String,
    working_directory: String,
) -> Result<RemoteDirectoryListing, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "list_remote_directories"));
    tauri::async_runtime::spawn_blocking(move || {
        inspect_remote_directory(&connection_string, Some(&working_directory))
    })
    .await
    .map_err(|error| format!("Could not read the remote directory: {error}"))?
}

fn inspect_remote_directory(
    connection_string: &str,
    working_directory: Option<&str>,
) -> Result<RemoteDirectoryListing, String> {
    let connection = SshConnection::parse(connection_string)?;
    let command = remote_directory_command(working_directory);
    let output = run_ssh_command(&connection, &command)?;
    parse_directory_listing(&connection.connection_string, &output.stdout)
}

pub fn run_remote_command(connection_string: &str, remote_command: &str) -> Result<Output, String> {
    run_ssh_command(&SshConnection::parse(connection_string)?, remote_command)
}

fn run_ssh_command(connection: &SshConnection, remote_command: &str) -> Result<Output, String> {
    let mut child = connection
        .command(remote_command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start SSH: {error}"))?;
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("Could not wait for SSH: {error}"))?
            .is_some()
        {
            break;
        }
        if started.elapsed() >= SSH_COMMAND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("SSH connection timed out.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Could not read the SSH response: {error}"))?;

    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if detail.is_empty() {
            format!("SSH exited with {}.", output.status)
        } else {
            format!("SSH connection failed. {}", single_line(&detail))
        })
    }
}

fn remote_directory_command(working_directory: Option<&str>) -> String {
    let inspect = r#"printf '\nTAU_REMOTE_DIRECTORY\000%s\000%s\000' "$(hostname)" "$(pwd -P)" && find -L . ! -name . -prune -type d -print0"#;
    working_directory.map_or_else(
        || inspect.to_string(),
        |path| format!("cd {} && {inspect}", shell_words::quote(path)),
    )
}

fn parse_directory_listing(
    connection_string: &str,
    output: &[u8],
) -> Result<RemoteDirectoryListing, String> {
    let fields = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    let marker_index = fields
        .iter()
        .position(|field| field.ends_with(DIRECTORY_MARKER))
        .ok_or_else(|| "SSH connected, but the remote directory was not reported.".to_string())?;
    let host = fields
        .get(marker_index + 1)
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Remote".into());
    let working_directory = fields
        .get(marker_index + 2)
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "SSH connected, but the remote directory was not reported.".to_string())?;
    let mut directories = fields
        .iter()
        .skip(marker_index + 3)
        .filter_map(|value| {
            let value = String::from_utf8_lossy(value);
            let name = value.strip_prefix("./").unwrap_or(&value);
            if name.is_empty() {
                return None;
            }
            let path = if working_directory == "/" {
                format!("/{name}")
            } else {
                format!("{working_directory}/{name}")
            };
            Some(RemoteDirectoryEntry {
                name: name.into(),
                path,
            })
        })
        .collect::<Vec<_>>();
    directories.sort_by_cached_key(|directory| directory.name.to_lowercase());

    Ok(RemoteDirectoryListing {
        connection_string: connection_string.into(),
        working_directory,
        host,
        directories,
    })
}

pub fn remote_pi_command(working_directory: &str, session_path: Option<&str>) -> String {
    let mut command = format!(
        "cd {} && exec pi --mode rpc",
        shell_words::quote(working_directory)
    );
    if let Some(path) = session_path {
        command.push_str(" --session ");
        command.push_str(&shell_words::quote(path));
    }
    format!(
        "exec \"${{SHELL:-/bin/sh}}\" -lc {}",
        shell_words::quote(&command)
    )
}

fn single_line(value: &str) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .chars()
        .take(2_048)
        .collect()
}

fn resolve_ssh_binary() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("TAU_SSH_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Ok(output) = Command::new("which").arg("ssh").output() {
        if output.status.success() {
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    [
        "/usr/bin/ssh",
        "/usr/local/bin/ssh",
        "/opt/homebrew/bin/ssh",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
    .ok_or_else(|| "Could not find ssh. Install OpenSSH or set TAU_SSH_PATH.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_and_destination_only_connections() {
        let full = SshConnection::parse("ssh user@example -p 1234").expect("full command");
        assert_eq!(full.arguments, ["user@example", "-p", "1234"]);

        let destination = SshConnection::parse("user@example").expect("destination");
        assert_eq!(destination.arguments, ["user@example"]);
    }

    #[test]
    fn quotes_remote_paths_and_sessions() {
        let command = remote_pi_command(
            "/home/timur/Project files",
            Some("/home/timur/.pi/session's.jsonl"),
        );
        assert!(command.contains("Project files"));
        assert!(command.contains("session"));
        assert!(command.contains("pi --mode rpc"));
    }

    #[test]
    fn quotes_directories_when_browsing() {
        let command = remote_directory_command(Some("/home/timur/Project files"));
        assert!(command.starts_with("cd '/home/timur/Project files'"));
        assert!(command.contains("TAU_REMOTE_DIRECTORY"));
    }

    #[test]
    fn parses_directory_listings_after_login_output() {
        let listing = parse_directory_listing(
            "ssh build-box",
            b"Welcome\nTAU_REMOTE_DIRECTORY\0build-box\0/home/timur\0./beta\0./Alpha Project\0",
        )
        .expect("directory listing");
        assert_eq!(listing.host, "build-box");
        assert_eq!(listing.working_directory, "/home/timur");
        assert_eq!(listing.directories[0].name, "Alpha Project");
        assert_eq!(listing.directories[0].path, "/home/timur/Alpha Project");
        assert_eq!(listing.directories[1].name, "beta");
    }
}
