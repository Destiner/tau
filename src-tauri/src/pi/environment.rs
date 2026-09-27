#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, OnceLock},
    thread,
    time::{Duration, Instant},
};

const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_SHELL_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
pub(super) const LOGIN_SHELL_PATH_MARKER: &str = "__TAU_PATH__";
pub(super) const LOGIN_SHELL_AGENT_DIR_MARKER: &str = "__TAU_AGENT_DIR__";
pub(super) const LOGIN_SHELL_END_MARKER: &str = "__TAU_ENV_END__";
/// Shells disagree about flags: tcsh rejects `-l` unless it stands alone, and
/// shells that are not POSIX-like may reject bundling altogether. The
/// combinations are ordered from most to least of the user's configuration.
pub(super) const LOGIN_SHELL_ARGUMENTS: [&[&str]; 4] =
    [&["-lic"], &["-i", "-c"], &["-lc"], &["-c"]];
pub(super) const CSH_LOGIN_SHELL_ARGUMENTS: [&[&str]; 2] = [&["-i", "-c"], &["-c"]];
#[derive(Default)]
pub(super) struct LoginShellEnvironment {
    path: Option<OsString>,
    pub(super) agent_dir: Option<OsString>,
}
pub fn resolve_pi_binary() -> Option<PathBuf> {
    resolve_executable(
        "TAU_PI_PATH",
        "pi",
        &["/opt/homebrew/bin/pi", "/usr/local/bin/pi"],
        &[".local/bin/pi", ".bun/bin/pi"],
    )
}

pub(super) fn resolve_node_binary() -> Option<PathBuf> {
    resolve_executable(
        "TAU_NODE_PATH",
        "node",
        &[
            "/opt/homebrew/bin/node",
            "/usr/local/bin/node",
            "/usr/bin/node",
        ],
        &[
            ".local/bin/node",
            ".nvm/current/bin/node",
            ".volta/bin/node",
        ],
    )
}

fn resolve_executable(
    environment_variable: &str,
    executable: &str,
    absolute_candidates: &[&str],
    home_candidates: &[&str],
) -> Option<PathBuf> {
    if let Some(path) = env::var_os(environment_variable).map(PathBuf::from) {
        if is_executable_file(&path) {
            return Some(path);
        }
    }
    if let Some(path) = find_on_path(executable, env::var_os("PATH").as_deref()) {
        return Some(path);
    }
    if let Some(path) = find_on_login_shell_path(executable) {
        return Some(path);
    }
    for candidate in absolute_candidates {
        let path = PathBuf::from(candidate);
        if is_executable_file(&path) {
            return Some(path);
        }
    }
    if let Some(home) = dirs::home_dir() {
        for candidate in home_candidates {
            let path = home.join(candidate);
            if is_executable_file(&path) {
                return Some(path);
            }
        }
    }
    None
}

pub(super) fn find_on_path(executable: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    path.into_iter()
        .flat_map(env::split_paths)
        .map(|directory| directory.join(executable))
        .find(|path| is_executable_file(path))
}

/// Searching the login shell's PATH ourselves avoids depending on a lookup
/// command whose spelling differs between shells.
fn find_on_login_shell_path(executable: &str) -> Option<PathBuf> {
    find_on_path(executable, login_shell_path().map(OsString::as_os_str))
}

pub(super) fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
pub(super) fn configure_session_arguments(
    command: &mut Command,
    session_dir: Option<&Path>,
    session_path: Option<&str>,
) {
    if let Some(directory) = session_dir {
        command.arg("--session-dir").arg(directory);
    }
    if let Some(path) = session_path {
        command.args(["--session", path]);
    }
}

pub(super) fn validate_runtime_id(runtime_id: &str) -> Result<(), String> {
    if runtime_id.is_empty() || runtime_id.len() > 256 {
        return Err("Tau supplied an invalid runtime id.".into());
    }
    Ok(())
}

pub(super) fn validate_start_paths(
    project_path: &str,
    session_path: Option<&str>,
) -> Result<(), String> {
    if !Path::new(project_path).is_dir() {
        return Err("The project folder no longer exists.".into());
    }
    if session_path.is_some_and(|path| !Path::new(path).is_file()) {
        return Err("The selected session file no longer exists.".into());
    }
    Ok(())
}

pub(super) fn configure_child_path(
    command: &mut Command,
    executables: &[&Path],
) -> Result<(), String> {
    let current_path = env::var_os("PATH");
    let path = build_child_path(
        executables,
        login_shell_path().map(OsString::as_os_str),
        current_path.as_deref(),
    )?;
    command.env("PATH", path);
    if env::var_os("PI_CODING_AGENT_DIR").is_none_or(|path| path.is_empty()) {
        if let Some(agent_dir) = login_shell_pi_agent_dir() {
            command.env("PI_CODING_AGENT_DIR", agent_dir);
        }
    }
    Ok(())
}

/// Launchd gives GUI apps a minimal environment, so toolchains and a custom
/// Pi agent directory configured in shell startup files would otherwise be
/// invisible. The shell must be interactive as well as login: zsh only reads
/// `.zshrc`, where these values are commonly exported, when it is interactive.
fn login_shell_environment() -> &'static LoginShellEnvironment {
    static ENVIRONMENT: OnceLock<LoginShellEnvironment> = OnceLock::new();
    ENVIRONMENT.get_or_init(query_login_shell_environment)
}

fn login_shell_path() -> Option<&'static OsString> {
    login_shell_environment().path.as_ref()
}

pub(crate) fn login_shell_pi_agent_dir() -> Option<&'static OsString> {
    login_shell_environment().agent_dir.as_ref()
}

fn query_login_shell_environment() -> LoginShellEnvironment {
    // Windows processes already inherit a usable environment, and none of
    // these flags mean anything to its shells.
    if cfg!(windows) {
        return LoginShellEnvironment::default();
    }
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // `printenv` reports exported variables in the same representation child
    // processes receive. Interpolating `$PATH` would produce a shell-specific
    // array representation in fish and csh.
    let script = format!(
        "echo {LOGIN_SHELL_PATH_MARKER}; printenv PATH; echo {LOGIN_SHELL_AGENT_DIR_MARKER}; printenv PI_CODING_AGENT_DIR; echo {LOGIN_SHELL_END_MARKER}"
    );
    let csh = Path::new(&shell)
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| matches!(name, "csh" | "tcsh"));
    let script = if csh {
        format!("if ( -r ~/.login ) source ~/.login; {script}")
    } else {
        script
    };
    let arguments: &[&[&str]] = if csh {
        &CSH_LOGIN_SHELL_ARGUMENTS
    } else {
        &LOGIN_SHELL_ARGUMENTS
    };
    let deadline = Instant::now() + LOGIN_SHELL_TIMEOUT;
    arguments
        .iter()
        .find_map(|arguments| {
            let timeout = deadline.checked_duration_since(Instant::now())?;
            let mut command = Command::new(&shell);
            command.args(*arguments).arg(&script);
            let output = capture_stdout(command, timeout)?;
            parse_login_shell_environment(&String::from_utf8_lossy(&output))
        })
        .unwrap_or_default()
}

/// Startup files are free to print banners, so values are read from between
/// markers rather than from the surrounding output.
pub(super) fn parse_login_shell_environment(output: &str) -> Option<LoginShellEnvironment> {
    let path = marked_shell_value(
        output,
        LOGIN_SHELL_PATH_MARKER,
        LOGIN_SHELL_AGENT_DIR_MARKER,
    )?;
    Some(LoginShellEnvironment {
        path: Some(path),
        agent_dir: marked_shell_value(output, LOGIN_SHELL_AGENT_DIR_MARKER, LOGIN_SHELL_END_MARKER),
    })
}

fn marked_shell_value(output: &str, start: &str, end: &str) -> Option<OsString> {
    let value = output.split_once(start)?.1.split_once(end)?.0.trim();
    (!value.is_empty()).then(|| OsString::from(value))
}

#[cfg(test)]
pub(super) fn parse_login_shell_path(output: &str) -> Option<OsString> {
    parse_login_shell_environment(output)?.path
}

/// Interactive startup files can hang, which would otherwise block the caller
/// forever, so the child is killed once the timeout elapses.
pub(super) fn capture_stdout(mut command: Command, timeout: Duration) -> Option<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(read_bounded_stdout(stdout, LOGIN_SHELL_OUTPUT_LIMIT_BYTES));
    });

    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
    let remaining = deadline.checked_duration_since(Instant::now())?;
    receiver.recv_timeout(remaining).ok().flatten()
}

pub(super) fn read_bounded_stdout(mut stdout: impl Read, limit: usize) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut overflow = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stdout.read(&mut buffer).ok()?;
        if read == 0 {
            return (!overflow).then_some(output);
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
        overflow |= read > remaining;
    }
}

pub(super) fn build_child_path(
    executables: &[&Path],
    login_path: Option<&OsStr>,
    current_path: Option<&OsStr>,
) -> Result<OsString, String> {
    let mut directories = Vec::new();
    for executable in executables {
        if let Some(parent) = executable
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            push_directory(&mut directories, parent.to_path_buf());
        }
    }
    for path in [login_path, current_path].into_iter().flatten() {
        for directory in env::split_paths(path) {
            push_directory(&mut directories, directory);
        }
    }
    env::join_paths(directories)
        .map_err(|error| format!("Could not prepare the Pi process PATH: {error}"))
}

fn push_directory(directories: &mut Vec<PathBuf>, directory: PathBuf) {
    if !directories.contains(&directory) {
        directories.push(directory);
    }
}
