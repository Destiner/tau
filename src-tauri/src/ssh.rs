use crate::models::{RemoteDirectoryEntry, RemoteDirectoryListing};
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use std::{
    env,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tauri::State;

const SSH_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_OUTPUT_LIMIT_BYTES: usize = 8 * 1024 * 1024;
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
const REMOTE_PI_KEEPALIVE_OPTIONS: [&str; 4] = [
    "-o",
    "ServerAliveInterval=15",
    "-o",
    "ServerAliveCountMax=3",
];
const DIRECTORY_MARKER: &[u8] = b"TAU_REMOTE_DIRECTORY";
const DIRECTORY_END_MARKER: &[u8] = b"TAU_REMOTE_END_DIRECTORY";
const SSH_OPTIONS_WITH_ARGUMENTS: [char; 22] = [
    'B', 'b', 'c', 'D', 'E', 'e', 'F', 'I', 'i', 'J', 'L', 'l', 'm', 'O', 'o', 'P', 'p', 'Q', 'R',
    'S', 'W', 'w',
];
const REMOTE_SHELL_LAUNCHER: &str = r#"case "${SHELL##*/}" in csh|tcsh) command='if ( -r ~/.login ) source ~/.login; '"$1"; exec "$SHELL" -i -c "$command" ;; *) exec "$SHELL" -lic "$1" ;; esac"#;

pub struct SshConnection {
    executable: PathBuf,
    arguments: Vec<String>,
    option_names: Vec<char>,
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

        let (arguments, option_names) = normalize_ssh_arguments(words)?;

        Ok(Self {
            executable,
            arguments,
            option_names,
            connection_string: connection_string.to_string(),
        })
    }

    pub fn command(&self, remote_command: &str) -> Command {
        self.command_with_options(remote_command, &[])
    }

    pub(crate) fn transfer_command(&self, remote_command: &str) -> Result<Command, String> {
        let mut command = Command::new(&self.executable);
        let (destination, options) = self
            .arguments
            .split_last()
            .expect("validated SSH connection arguments");
        if self.option_names.iter().any(|flag| {
            matches!(
                flag,
                'f' | 'G' | 'M' | 'N' | 'O' | 'Q' | 's' | 't' | 'V' | 'W'
            )
        }) {
            return Err("The SSH connection uses a mode that cannot transfer previews.".into());
        }
        // OpenSSH uses first-value-wins for most -o settings, while short flags
        // such as -A/-X and -S are effectively last-value-wins. Keep both sides
        // of user arguments intentional so saved connection options cannot undo
        // the transfer sandbox.
        command
            .args(SSH_OPTIONS)
            .args([
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ForwardAgent=no",
                "-o",
                "ForwardX11=no",
                "-o",
                "ForwardX11Trusted=no",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "RequestTTY=no",
                "-o",
                "ForkAfterAuthentication=no",
                "-o",
                "SessionType=default",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPersist=no",
                "-o",
                "ControlPath=none",
            ])
            .args(options)
            .args(["-a", "-x", "-T", "-S", "none"])
            .arg(destination)
            .arg(remote_command);
        Ok(command)
    }

    pub fn pi_command(&self, remote_command: &str) -> Command {
        let mut command = Command::new(&self.executable);
        let (destination, options) = self
            .arguments
            .split_last()
            .expect("validated SSH connection arguments");
        command
            .args(REMOTE_PI_KEEPALIVE_OPTIONS)
            .args(options)
            .args(["-S", "none"])
            .args(SSH_OPTIONS)
            .arg(destination)
            .arg(remote_command);
        command
    }

    fn command_with_options(&self, remote_command: &str, enforced_options: &[&str]) -> Command {
        let mut command = Command::new(&self.executable);
        let (destination, options) = self
            .arguments
            .split_last()
            .expect("validated SSH connection arguments");
        command
            .args(enforced_options)
            .args(options)
            .args(SSH_OPTIONS)
            .arg(destination)
            .arg(remote_command);
        command
    }
}

fn normalize_ssh_arguments(words: Vec<String>) -> Result<(Vec<String>, Vec<char>), String> {
    let mut options = Vec::new();
    let mut option_names = Vec::new();
    let mut destination = None;
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        if word == "--" {
            if destination.is_some() {
                return Err("The SSH connection string has more than one destination.".into());
            }
            destination = words.next();
            if words.next().is_some() {
                return Err("The SSH connection string includes a remote command.".into());
            }
            break;
        }
        if let Some(option) = word.strip_prefix('-').filter(|value| !value.is_empty()) {
            if option.starts_with('-') {
                return Err("Use OpenSSH short options in the connection string.".into());
            }
            let mut separate_argument = false;
            for (index, name) in option.char_indices() {
                option_names.push(name);
                if SSH_OPTIONS_WITH_ARGUMENTS.contains(&name) {
                    separate_argument = index + name.len_utf8() == option.len();
                    break;
                }
            }
            options.push(word);
            if separate_argument {
                options.push(words.next().ok_or_else(|| {
                    "An SSH option in the connection string is missing its value.".to_string()
                })?);
            }
            continue;
        }
        if destination.replace(word).is_some() {
            return Err("The SSH connection string includes a remote command.".into());
        }
    }

    let destination = destination
        .filter(|destination| !destination.is_empty())
        .ok_or_else(|| "The SSH connection string does not include a destination.".to_string())?;
    options.push(destination);
    Ok((options, option_names))
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "SSH did not expose an output stream.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "SSH did not expose an error stream.".to_string())?;
    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_sender.send(read_bounded(stdout, SSH_OUTPUT_LIMIT_BYTES));
    });
    thread::spawn(move || {
        let _ = stderr_sender.send(read_bounded(stderr, SSH_OUTPUT_LIMIT_BYTES));
    });

    let deadline = Instant::now() + SSH_COMMAND_TIMEOUT;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Could not wait for SSH: {error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("SSH connection timed out.".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    let (stdout, stdout_overflow) = receive_ssh_output(&stdout_receiver, deadline)?;
    let (stderr, stderr_overflow) = receive_ssh_output(&stderr_receiver, deadline)?;
    if stdout_overflow || stderr_overflow {
        return Err("The SSH response was too large.".into());
    }
    let output = Output {
        status,
        stdout,
        stderr,
    };

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

fn receive_ssh_output(
    receiver: &mpsc::Receiver<std::io::Result<(Vec<u8>, bool)>>,
    deadline: Instant,
) -> Result<(Vec<u8>, bool), String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| "SSH connection timed out.".to_string())?;
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => "SSH connection timed out.".to_string(),
            mpsc::RecvTimeoutError::Disconnected => "Could not read the SSH response.".to_string(),
        })?
        .map_err(|error| format!("Could not read the SSH response: {error}"))
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut overflow = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok((output, overflow));
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        overflow |= read > remaining;
    }
}

fn remote_directory_command(working_directory: Option<&str>) -> String {
    let inspect = r#"printf '\nTAU_REMOTE_DIRECTORY\000%s\000%s\000' "$(hostname)" "$(pwd -P)" && find -L . ! -name . -prune -type d -print0 && printf 'TAU_REMOTE_END_DIRECTORY\000'"#;
    let (script, argument) = working_directory.map_or((inspect.to_string(), None), |path| {
        (format!("cd \"$1\" && {inspect}"), Some(path))
    });
    let mut command = format!("exec /bin/sh -c {} tau", shell_words::quote(&script));
    if let Some(path) = argument {
        command.push(' ');
        command.push_str(&shell_words::quote(path));
    }
    command
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
    let directory_start = marker_index + 3;
    let directory_end = fields
        .iter()
        .skip(directory_start)
        .position(|field| *field == DIRECTORY_END_MARKER)
        .map(|index| directory_start + index)
        .ok_or_else(|| {
            "SSH connected, but the remote directory list was incomplete.".to_string()
        })?;
    let mut directories = fields[directory_start..directory_end]
        .iter()
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
    remote_login_shell_command(&command)
}

pub fn remote_login_shell_command(command: &str) -> String {
    format!(
        "exec /bin/sh -c {} tau {}",
        shell_words::quote(REMOTE_SHELL_LAUNCHER),
        shell_words::quote(command),
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
    fn drains_oversized_ssh_output_without_storing_it_all() {
        let (output, overflow) = read_bounded(&b"0123456789"[..], 4).expect("bounded output");
        assert_eq!(output, b"0123");
        assert!(overflow);
    }

    #[test]
    fn parses_full_and_destination_only_connections() {
        let full = SshConnection::parse("ssh user@example -p 1234").expect("full command");
        assert_eq!(full.arguments, ["-p", "1234", "user@example"]);
        let conventional =
            SshConnection::parse("ssh -p 1234 user@example").expect("conventional command");
        assert_eq!(conventional.arguments, full.arguments);
        let clustered =
            SshConnection::parse("ssh -vp 1234 user@example").expect("clustered options");
        assert_eq!(clustered.arguments, ["-vp", "1234", "user@example"]);

        let destination = SshConnection::parse("user@example").expect("destination");
        assert_eq!(destination.arguments, ["user@example"]);

        for connection in [
            "ssh -i/tmp/id_ed25519 user@example",
            "ssh -F/tmp/ssh_config user@example",
            "ssh -Jjump-host user@example",
            "ssh -vi/tmp/identity-with-fnst user@example",
        ] {
            let parsed = SshConnection::parse(connection).expect("attached option value");
            assert!(parsed.transfer_command("preview").is_ok(), "{connection}");
        }
    }

    #[test]
    fn preview_transfers_disable_forwarding_and_interactive_modes() {
        let connection = SshConnection::parse(
            "ssh -A -X -L 9000:localhost:9000 -J jump -i key -p 2222 user@example",
        )
        .expect("preview connection");
        let arguments = connection
            .transfer_command("preview")
            .expect("safe transfer")
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        for setting in [
            "ClearAllForwardings=yes",
            "ForwardAgent=no",
            "ForwardX11=no",
            "ForwardX11Trusted=no",
            "RequestTTY=no",
        ] {
            assert!(arguments.iter().any(|argument| argument == setting));
        }
        assert!(arguments.windows(2).any(|pair| pair == ["-J", "jump"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-i", "key"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-p", "2222"]));
    }

    #[test]
    fn preview_transfers_enforce_effective_openssh_settings() {
        let ssh = Path::new("/usr/bin/ssh");
        if !ssh.is_file() {
            return;
        }
        let connection = SshConnection::parse(
            "ssh -A -X -S /tmp/shared.sock -o ForkAfterAuthentication=yes -o SessionType=none -J jump user@example",
        )
        .expect("preview connection");
        let command = connection
            .transfer_command("preview")
            .expect("safe transfer");
        let mut arguments = command
            .get_args()
            .map(|argument| argument.to_os_string())
            .collect::<Vec<_>>();
        arguments.pop();
        let output = Command::new(ssh)
            .arg("-G")
            .args(arguments)
            .output()
            .expect("ssh -G");
        assert!(output.status.success());
        let settings = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        for setting in [
            "forwardagent no",
            "forwardx11 no",
            "requesttty false",
            "forkafterauthentication no",
            "sessiontype default",
            "controlmaster false",
            "clearallforwardings yes",
        ] {
            assert!(settings.contains(setting), "missing {setting}: {settings}");
        }
        assert!(settings.contains("proxyjump jump"));
        assert!(!settings.contains("controlpath /tmp/shared.sock"));
    }

    #[test]
    fn preview_transfers_reject_forced_tty_and_background_modes() {
        for mode in [
            "-t",
            "-tt",
            "-N",
            "-f",
            "-s",
            "-M",
            "-Ocheck",
            "-Wlocalhost:22",
        ] {
            let connection = SshConnection::parse(&format!("ssh {mode} user@example"))
                .expect("parsed connection");
            assert!(connection.transfer_command("preview").is_err(), "{mode}");
        }
    }

    #[test]
    fn remote_pi_connections_enforce_keepalives_and_disable_sharing() {
        let connection = SshConnection::parse(
            "ssh -o ServerAliveInterval=0 -o ServerAliveCountMax=9 -S /tmp/shared.sock -J jump -p 2222 user@example",
        )
        .expect("remote Pi connection");
        let arguments = connection
            .pi_command("remote-pi")
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            &arguments[..4],
            [
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
            ]
        );
        let supplied_socket = arguments
            .windows(2)
            .position(|pair| pair == ["-S", "/tmp/shared.sock"])
            .expect("supplied control socket");
        let disabled_socket = arguments
            .windows(2)
            .position(|pair| pair == ["-S", "none"])
            .expect("disabled control socket");
        assert!(disabled_socket > supplied_socket);
        assert_eq!(arguments.last().map(String::as_str), Some("remote-pi"));
        assert_eq!(
            arguments.get(arguments.len() - 2).map(String::as_str),
            Some("user@example")
        );
        assert!(arguments.windows(2).any(|pair| pair == ["-J", "jump"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-p", "2222"]));
    }

    #[test]
    fn short_lived_connections_do_not_use_remote_pi_lifetime_options() {
        let connection =
            SshConnection::parse("ssh -p 2222 user@example").expect("short-lived connection");
        let arguments = connection
            .command("true")
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(!arguments
            .iter()
            .any(|argument| argument == "ServerAliveInterval=15"));
        assert!(!arguments
            .iter()
            .any(|argument| argument == "ServerAliveCountMax=3"));
        assert!(!arguments.windows(2).any(|pair| pair == ["-S", "none"]));
        assert_eq!(&arguments[..2], ["-p", "2222"]);
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
        assert!(command.contains("/bin/sh -c"));
    }

    #[test]
    fn supports_csh_remote_login_environments() {
        let shell = Path::new("/bin/csh");
        if !shell.is_file() {
            return;
        }
        let output = Command::new(shell)
            .args([
                "-c",
                &remote_login_shell_command("printf tau-remote-shell-ok"),
            ])
            .env("SHELL", shell)
            .output()
            .expect("csh remote shell fixture");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"tau-remote-shell-ok");
    }

    #[test]
    fn rejects_remote_commands_in_connection_strings() {
        let error = SshConnection::parse("ssh build-box uptime")
            .err()
            .expect("remote command rejection");
        assert!(error.contains("remote command"));
    }

    #[test]
    fn quotes_directories_when_browsing() {
        let command = remote_directory_command(Some("/home/timur/Project files"));
        assert!(command.contains("/bin/sh -c"));
        assert!(command.contains("Project files"));
        assert!(command.contains("TAU_REMOTE_DIRECTORY"));
        assert!(command.contains("TAU_REMOTE_END_DIRECTORY"));
    }

    #[test]
    fn browses_directories_through_a_csh_remote_environment() {
        let shell = Path::new("/bin/csh");
        if !shell.is_file() {
            return;
        }
        let directory = tempfile::tempdir().expect("remote directory fixture");
        std::fs::create_dir(directory.path().join("Alpha Project")).expect("remote child");
        let output = Command::new(shell)
            .args([
                "-c",
                &remote_directory_command(Some(
                    directory.path().to_str().expect("UTF-8 fixture path"),
                )),
            ])
            .output()
            .expect("csh remote directory fixture");
        assert!(output.status.success());
        let listing = parse_directory_listing("ssh build-box", &output.stdout)
            .expect("remote directory listing");
        assert_eq!(listing.directories.len(), 1);
        assert_eq!(listing.directories[0].name, "Alpha Project");
    }

    #[test]
    fn parses_directory_listings_after_login_output() {
        let listing = parse_directory_listing(
            "ssh build-box",
            b"Welcome\nTAU_REMOTE_DIRECTORY\0build-box\0/home/timur\0./beta\0./Alpha Project\0TAU_REMOTE_END_DIRECTORY\0logout banner",
        )
        .expect("directory listing");
        assert_eq!(listing.host, "build-box");
        assert_eq!(listing.working_directory, "/home/timur");
        assert_eq!(listing.directories[0].name, "Alpha Project");
        assert_eq!(listing.directories[0].path, "/home/timur/Alpha Project");
        assert_eq!(listing.directories[1].name, "beta");
    }
}
