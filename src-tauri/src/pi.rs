use crate::ssh::{remote_pi_command, SshConnection};
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use opentelemetry::Value as TelemetryValue;
use serde::Serialize;
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::{HashMap, VecDeque},
    env,
    ffi::{OsStr, OsString},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

const MAX_RPC_LINE_BYTES: usize = 64 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 8;
const STDERR_LINE_CHARS: usize = 2048;
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_SHELL_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;
const PI_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const PI_STOP_TIMEOUT: Duration = Duration::from_secs(3);
const PI_STOP_GRACE_PERIOD: Duration = Duration::from_secs(1);
const PI_WRITER_QUEUE_CAPACITY: usize = 32;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const LOGIN_SHELL_PATH_MARKER: &str = "__TAU_PATH__";
const LOGIN_SHELL_AGENT_DIR_MARKER: &str = "__TAU_AGENT_DIR__";
const LOGIN_SHELL_END_MARKER: &str = "__TAU_ENV_END__";
/// Shells disagree about flags: tcsh rejects `-l` unless it stands alone, and
/// shells that are not POSIX-like may reject bundling altogether. The
/// combinations are ordered from most to least of the user's configuration.
const LOGIN_SHELL_ARGUMENTS: [&[&str]; 4] = [&["-lic"], &["-i", "-c"], &["-lc"], &["-c"]];
const CSH_LOGIN_SHELL_ARGUMENTS: [&[&str]; 2] = [&["-i", "-c"], &["-c"]];

type StderrTail = Arc<Mutex<VecDeque<String>>>;
type PiWriter = Arc<Mutex<Option<mpsc::SyncSender<PiWriteRequest>>>>;

#[derive(Default)]
struct LoginShellEnvironment {
    path: Option<OsString>,
    agent_dir: Option<OsString>,
}

#[derive(Default)]
pub struct PiState {
    inner: Arc<Mutex<PiManager>>,
}

impl PiState {
    pub fn shutdown(&self) {
        let processes = self
            .inner
            .lock()
            .map(|mut manager| {
                manager.ownership = PiOwnership::ShuttingDown;
                manager
                    .processes
                    .drain()
                    .map(|(_, process)| process)
                    .collect()
            })
            .unwrap_or_default();
        stop_all_processes(processes);
    }
}

impl Drop for PiState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Default)]
struct PiManager {
    ownership_revision: u64,
    ownership: PiOwnership,
    generation: u64,
    processes: HashMap<String, PiProcess>,
}

#[derive(Default)]
enum PiOwnership {
    #[default]
    Unclaimed,
    Claiming {
        owner_id: String,
        expected_revision: u64,
    },
    Active {
        owner_id: String,
    },
    ShuttingDown,
}

impl PiManager {
    fn require_owner(&self, owner_id: &str) -> Result<(), String> {
        match &self.ownership {
            PiOwnership::Active {
                owner_id: active_owner,
            } if active_owner == owner_id => Ok(()),
            PiOwnership::ShuttingDown => Err("Tau is shutting down.".into()),
            _ => Err(
                "This Tau window no longer owns the Pi runtime. Reload Tau and try again.".into(),
            ),
        }
    }
}

#[derive(Clone)]
struct PiProcess {
    owner_id: String,
    generation: u64,
    child: Arc<Mutex<Child>>,
    writer: PiWriter,
    usable: Arc<AtomicBool>,
}

impl PiProcess {
    fn close_stdin(&self) {
        self.usable.store(false, Ordering::Release);
        // Closing the shared sender also fences retained process handles. The
        // writer owns stdin and releases it without holding a process lock.
        self.writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}

struct PiWriteRequest {
    line: Vec<u8>,
    result: mpsc::SyncSender<std::io::Result<()>>,
}

struct PiReaderContext<R: Runtime> {
    app: AppHandle<R>,
    manager: Arc<Mutex<PiManager>>,
    child: Arc<Mutex<Child>>,
    runtime_id: String,
    generation: u64,
}

impl<R: Runtime> Clone for PiReaderContext<R> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            manager: Arc::clone(&self.manager),
            child: Arc::clone(&self.child),
            runtime_id: self.runtime_id.clone(),
            generation: self.generation,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiEvent<'a> {
    runtime_id: &'a str,
    generation: u64,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<i32>,
}

pub fn resolve_pi_binary() -> Option<PathBuf> {
    if let Some(path) = env::var_os("TAU_PI_PATH").map(PathBuf::from) {
        if is_executable_file(&path) {
            return Some(path);
        }
    }
    if let Some(path) = find_on_path("pi", env::var_os("PATH").as_deref()) {
        return Some(path);
    }
    if let Some(path) = find_on_login_shell_path("pi") {
        return Some(path);
    }

    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/pi"),
        PathBuf::from("/usr/local/bin/pi"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/pi"));
        candidates.push(home.join(".bun/bin/pi"));
    }
    candidates.into_iter().find(|path| is_executable_file(path))
}

fn resolve_node_binary() -> Option<PathBuf> {
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

fn find_on_path(executable: &str, path: Option<&OsStr>) -> Option<PathBuf> {
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

fn is_executable_file(path: &Path) -> bool {
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipCommandError {
    kind: &'static str,
    message: String,
}

#[derive(Debug)]
struct StoppedRuntime {
    runtime_id: String,
    generation: u64,
}

#[derive(Debug)]
struct ClaimOutcome {
    stopped: Vec<StoppedRuntime>,
    replacement: bool,
}

#[derive(Debug)]
struct ClaimFailure {
    kind: &'static str,
    message: String,
    stopped: Vec<StoppedRuntime>,
}

#[tauri::command]
pub fn read_pi_frontend_revision(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<u64, String> {
    let _command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_pi_frontend_revision"));
    let manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    if matches!(manager.ownership, PiOwnership::ShuttingDown) {
        return Err("Tau is shutting down.".into());
    }
    Ok(manager.ownership_revision)
}

#[tauri::command]
pub async fn claim_pi_frontend(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    expected_revision: u64,
) -> Result<(), OwnershipCommandError> {
    let command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "claim_pi_frontend"));
    validate_owner_id(&owner_id).map_err(|message| OwnershipCommandError {
        kind: "retryable",
        message,
    })?;
    let inner = Arc::clone(&state.inner);
    let result = tauri::async_runtime::spawn_blocking(move || {
        claim_pi_frontend_inner(&inner, owner_id, expected_revision)
    })
    .await
    .map_err(|_| OwnershipCommandError {
        kind: "retryable",
        message: "Tau could not prepare the Pi runtime. Try again.".into(),
    })?;

    match result {
        Ok(outcome) => {
            record_ownership_cleanup(&telemetry, command_span.as_ref(), &outcome.stopped);
            telemetry.record_ownership_event(
                if outcome.replacement {
                    "replacement"
                } else {
                    "initial"
                },
                "success",
                outcome.stopped.len(),
                command_span.as_ref().map(|span| span.span_context()),
            );
            Ok(())
        }
        Err(failure) => {
            record_ownership_cleanup(&telemetry, command_span.as_ref(), &failure.stopped);
            telemetry.record_ownership_event(
                "replacement",
                if failure.kind == "conflict" {
                    "rejected"
                } else {
                    "cleanup_failed"
                },
                failure.stopped.len(),
                command_span.as_ref().map(|span| span.span_context()),
            );
            Err(OwnershipCommandError {
                kind: failure.kind,
                message: failure.message,
            })
        }
    }
}

fn record_ownership_cleanup(
    telemetry: &Telemetry,
    command_span: Option<&crate::telemetry::CommandSpan>,
    stopped: &[StoppedRuntime],
) {
    let span_context = command_span.map(|span| span.span_context());
    for runtime in stopped {
        telemetry.record_process_lifecycle(
            "pi.process.stopped",
            &runtime.runtime_id,
            Some(runtime.generation),
            span_context.as_ref(),
            &[(
                "tau.process.stop_reason",
                TelemetryValue::String("ownership_replaced".into()),
            )],
        );
        telemetry.record_pi_process_exit(true);
    }
    if !stopped.is_empty() {
        telemetry.force_flush_logs();
        telemetry.force_flush_metrics();
    }
}

fn claim_pi_frontend_inner(
    inner: &Arc<Mutex<PiManager>>,
    owner_id: String,
    expected_revision: u64,
) -> Result<ClaimOutcome, ClaimFailure> {
    let mut manager = inner.lock().map_err(|_| ClaimFailure {
        kind: "retryable",
        message: "Pi process state is unavailable.".into(),
        stopped: Vec::new(),
    })?;
    let replacement;
    match &manager.ownership {
        PiOwnership::Active {
            owner_id: active_owner,
        } if active_owner == &owner_id => {
            return Ok(ClaimOutcome {
                stopped: Vec::new(),
                replacement: false,
            });
        }
        PiOwnership::Claiming {
            owner_id: claiming_owner,
            expected_revision: claiming_revision,
        } if claiming_owner == &owner_id && *claiming_revision == expected_revision => {
            replacement = true;
        }
        PiOwnership::ShuttingDown => {
            return Err(ClaimFailure {
                kind: "retryable",
                message: "Tau is shutting down.".into(),
                stopped: Vec::new(),
            });
        }
        _ => {
            if expected_revision != manager.ownership_revision {
                return Err(ClaimFailure {
                    kind: "conflict",
                    message:
                        "Another Tau window already owns the Pi runtime. Reload Tau and try again."
                            .into(),
                    stopped: Vec::new(),
                });
            }
            replacement = !matches!(manager.ownership, PiOwnership::Unclaimed);
            manager.ownership_revision =
                manager
                    .ownership_revision
                    .checked_add(1)
                    .ok_or_else(|| ClaimFailure {
                        kind: "retryable",
                        message: "Tau could not advance Pi ownership. Restart Tau.".into(),
                        stopped: Vec::new(),
                    })?;
            manager.ownership = PiOwnership::Claiming {
                owner_id: owner_id.clone(),
                expected_revision,
            };
        }
    }

    let stopped =
        stop_stale_processes(&mut manager).map_err(|(message, stopped)| ClaimFailure {
            kind: "retryable",
            message,
            stopped,
        })?;
    manager.ownership = PiOwnership::Active { owner_id };
    Ok(ClaimOutcome {
        stopped,
        replacement,
    })
}

fn validate_owner_id(owner_id: &str) -> Result<(), String> {
    if owner_id.is_empty() || owner_id.len() > 128 {
        return Err("Tau supplied an invalid frontend owner id.".into());
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_pi(
    app: AppHandle,
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    runtime_id: String,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    let command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "start_pi"));
    let span_context = command_span.as_ref().map(|span| span.span_context());

    start_pi_with(
        app,
        &state,
        &telemetry,
        span_context.as_ref(),
        owner_id,
        runtime_id,
        project_path,
        session_path,
    )
}

#[allow(clippy::too_many_arguments)]
fn start_pi_with<R: Runtime>(
    app: AppHandle<R>,
    state: &PiState,
    telemetry: &Telemetry,
    span_context: Option<&opentelemetry::trace::SpanContext>,
    owner_id: String,
    runtime_id: String,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    validate_start_paths(&project_path, session_path.as_deref())?;
    let resolved_pi_path = resolve_pi_binary();
    telemetry.record_process_lifecycle(
        "pi.process.resolved",
        &runtime_id,
        None,
        span_context,
        &[(
            "tau.process.resolution",
            TelemetryValue::String(
                if resolved_pi_path.is_some() {
                    "found"
                } else {
                    "not_found"
                }
                .into(),
            ),
        )],
    );
    let pi_path = resolved_pi_path
        .ok_or_else(|| "Could not find pi. Install it or set TAU_PI_PATH.".to_string())?;
    let node_path = resolve_node_binary();
    let mut executable_paths = vec![pi_path.as_path()];
    if let Some(path) = node_path.as_deref() {
        executable_paths.push(path);
    }
    let mut command = Command::new(&pi_path);
    configure_child_path(&mut command, &executable_paths)?;
    command.args(["--mode", "rpc"]).current_dir(&project_path);
    #[cfg(dev)]
    let session_dir = Some(crate::storage::default_session_dir(&project_path)?);
    #[cfg(not(dev))]
    let session_dir: Option<PathBuf> = None;
    configure_session_arguments(
        &mut command,
        session_dir.as_deref(),
        session_path.as_deref(),
    );
    spawn_command(
        app,
        state,
        telemetry,
        span_context,
        owner_id,
        runtime_id,
        command,
    )
}

fn configure_session_arguments(
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

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_pi_remote(
    app: AppHandle,
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    runtime_id: String,
    connection_string: String,
    working_directory: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    let command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "start_pi_remote"));
    let span_context = command_span.as_ref().map(|span| span.span_context());
    let connection = SshConnection::parse(&connection_string)?;
    let remote_command = remote_pi_command(&working_directory, session_path.as_deref());
    spawn_command(
        app,
        &state,
        &telemetry,
        span_context.as_ref(),
        owner_id,
        runtime_id,
        connection.pi_command(&remote_command),
    )
}

fn validate_runtime_id(runtime_id: &str) -> Result<(), String> {
    if runtime_id.is_empty() || runtime_id.len() > 256 {
        return Err("Tau supplied an invalid runtime id.".into());
    }
    Ok(())
}

fn validate_start_paths(project_path: &str, session_path: Option<&str>) -> Result<(), String> {
    if !Path::new(project_path).is_dir() {
        return Err("The project folder no longer exists.".into());
    }
    if session_path.is_some_and(|path| !Path::new(path).is_file()) {
        return Err("The selected session file no longer exists.".into());
    }
    Ok(())
}

fn configure_child_path(command: &mut Command, executables: &[&Path]) -> Result<(), String> {
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
fn parse_login_shell_environment(output: &str) -> Option<LoginShellEnvironment> {
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
fn parse_login_shell_path(output: &str) -> Option<OsString> {
    parse_login_shell_environment(output)?.path
}

/// Interactive startup files can hang, which would otherwise block the caller
/// forever, so the child is killed once the timeout elapses.
fn capture_stdout(mut command: Command, timeout: Duration) -> Option<Vec<u8>> {
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

fn read_bounded_stdout(mut stdout: impl Read, limit: usize) -> Option<Vec<u8>> {
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

fn build_child_path(
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

/// Emits a `pi-event`, recording (and force-flushing) a `pi.reader`
/// telemetry log if the emission itself fails — e.g. the webview is gone.
/// `app.emit`'s own error is never recorded: only the bounded, reviewed
/// event kind that failed to reach the frontend.
fn emit_pi_event<R: Runtime>(app: &AppHandle<R>, event: PiEvent<'_>) {
    let runtime_id = event.runtime_id.to_string();
    let generation = event.generation;
    let kind = event.kind;
    if app.emit("pi-event", event).is_err() {
        let telemetry = app.state::<Telemetry>();
        telemetry.record_reader_event(
            "pi.event.emit_failed",
            &runtime_id,
            Some(generation),
            &[(
                "tau.event.kind",
                TelemetryValue::String(kind.to_string().into()),
            )],
        );
        telemetry.force_flush_logs();
    }
}

/// Maps an `io::ErrorKind` a Pi stdout/stderr reader can observe to the
/// bounded, reviewed category `tau.reader.error_kind` allows. Never the
/// error's own message, which can include OS-specific detail.
fn io_error_kind_category(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::BrokenPipe => "broken_pipe",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        _ => "other",
    }
}

fn line_drop_reason(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_RPC_LINE_BYTES {
        return Some("oversized");
    }
    if std::str::from_utf8(bytes).is_err() {
        return Some("invalid_utf8");
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(_)) => None,
        _ => Some("malformed"),
    }
}

fn spawn_command<R: Runtime>(
    app: AppHandle<R>,
    state: &PiState,
    telemetry: &Telemetry,
    span_context: Option<&opentelemetry::trace::SpanContext>,
    owner_id: String,
    runtime_id: String,
    mut command: Command,
) -> Result<u64, String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    manager.require_owner(&owner_id)?;
    let replaced_generation = manager.processes.get(&runtime_id).map(|p| p.generation);
    stop_runtime_process(&mut manager, &runtime_id)?;
    if let Some(generation) = replaced_generation {
        telemetry.record_process_lifecycle(
            "pi.process.stopped",
            &runtime_id,
            Some(generation),
            span_context,
            &[(
                "tau.process.stop_reason",
                TelemetryValue::String("replaced".into()),
            )],
        );
        telemetry.record_pi_process_exit(true);
    }
    manager.generation = manager.generation.wrapping_add(1).max(1);
    let generation = manager.generation;

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start pi: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Pi did not expose an input stream.".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Pi did not expose an output stream.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Pi did not expose an error stream.".to_string())?;
    let child = Arc::new(Mutex::new(child));
    let writer = match spawn_stdin_writer(stdin) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = stop_child(&child);
            return Err(error);
        }
    };
    let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));

    manager.processes.insert(
        runtime_id.clone(),
        PiProcess {
            owner_id,
            generation,
            child: Arc::clone(&child),
            writer,
            usable: Arc::new(AtomicBool::new(true)),
        },
    );

    let reader_context = PiReaderContext {
        app: app.clone(),
        manager: Arc::clone(&state.inner),
        child,
        runtime_id: runtime_id.clone(),
        generation,
    };
    let stderr_reader =
        spawn_stderr_reader(reader_context.clone(), stderr, Arc::clone(&stderr_tail));
    spawn_stdout_reader(reader_context, stdout, stderr_tail, stderr_reader);
    telemetry.record_process_lifecycle(
        "pi.process.started",
        &runtime_id,
        Some(generation),
        span_context,
        &[],
    );
    telemetry.record_pi_process_start();
    emit_pi_event(
        &app,
        PiEvent {
            runtime_id: &runtime_id,
            generation,
            kind: "started",
            line: None,
            message: None,
            code: None,
        },
    );
    Ok(generation)
}

#[tauri::command]
pub fn send_pi<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, PiState>,
    owner_id: String,
    runtime_id: String,
    request: Value,
) -> Result<(), String> {
    let result = send_pi_to_state(&state, &owner_id, runtime_id.clone(), request);
    if result.is_err() {
        let failed_generation = state.inner.lock().ok().and_then(|manager| {
            manager.require_owner(&owner_id).ok()?;
            manager.processes.get(&runtime_id).and_then(|process| {
                (process.owner_id == owner_id && !process.usable.load(Ordering::Acquire))
                    .then_some(process.generation)
            })
        });
        if let Some(generation) = failed_generation {
            emit_pi_event(
                &app,
                PiEvent {
                    runtime_id: &runtime_id,
                    generation,
                    kind: "exited",
                    line: None,
                    message: None,
                    code: Some(1),
                },
            );
        }
    }
    result
}

fn spawn_stdin_writer(mut stdin: ChildStdin) -> Result<PiWriter, String> {
    let (sender, receiver) = mpsc::sync_channel::<PiWriteRequest>(PI_WRITER_QUEUE_CAPACITY);
    thread::Builder::new()
        .name("tau-pi-stdin".into())
        .spawn(move || {
            while let Ok(request) = receiver.recv() {
                let result = stdin.write_all(&request.line).and_then(|_| stdin.flush());
                let failed = result.is_err();
                let _ = request.result.send(result);
                if failed {
                    break;
                }
            }
        })
        .map_err(|_| {
            "Tau could not start the Pi input stream. Reopen the session and try again.".to_string()
        })?;
    Ok(Arc::new(Mutex::new(Some(sender))))
}

fn send_pi_to_state(
    state: &PiState,
    owner_id: &str,
    runtime_id: String,
    request: Value,
) -> Result<(), String> {
    let mut line = serde_json::to_vec(&request)
        .map_err(|error| format!("Could not encode the Pi request: {error}"))?;
    if line.len() > MAX_RPC_LINE_BYTES {
        return Err("The Pi request is too large.".into());
    }
    line.push(b'\n');

    let process = {
        let manager = state
            .inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        manager.require_owner(owner_id)?;
        manager
            .processes
            .get(&runtime_id)
            .filter(|process| process.owner_id == owner_id)
            .cloned()
            .ok_or_else(|| "The selected Pi runtime is not running.".to_string())?
    };
    // A blocked write must not prevent shutdown from closing this runtime.
    send_pi_line(&process, line, PI_WRITE_TIMEOUT)
}

fn send_pi_line(process: &PiProcess, line: Vec<u8>, timeout: Duration) -> Result<(), String> {
    if !process.usable.load(Ordering::Acquire) {
        return Err("Pi is no longer accepting requests. Restart Tau and try again.".into());
    }
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    process
        .writer
        .lock()
        .map_err(|_| {
            "The Pi input stream is unavailable. Reopen the session and try again.".to_string()
        })?
        .as_ref()
        .ok_or_else(|| {
            "Pi is no longer accepting requests. Restart Tau and try again.".to_string()
        })?
        .try_send(PiWriteRequest {
            line,
            result: result_sender,
        })
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => {
                "Pi is still accepting another request. Try again.".to_string()
            }
            mpsc::TrySendError::Disconnected(_) => {
                "The Pi input stream is unavailable. Reopen the session and try again.".to_string()
            }
        })?;

    match result_receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => {
            process.usable.store(false, Ordering::Release);
            Err("Pi could not accept the request. Reopen the session and try again.".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            process.usable.store(false, Ordering::Release);
            Err("The Pi input stream closed before accepting the request. Try again.".into())
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            process.usable.store(false, Ordering::Release);
            if stop_process(process).is_err() {
                return Err(
                    "Pi stopped responding and Tau could not stop it. Restart Tau and try again."
                        .into(),
                );
            }
            Err("Pi did not accept the request in time. Try again.".into())
        }
    }
}

#[tauri::command]
pub fn stop_pi(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    runtime_id: String,
) -> Result<(), String> {
    let command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "stop_pi"));
    let span_context = command_span.as_ref().map(|span| span.span_context());
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    manager.require_owner(&owner_id)?;
    let stopped_generation = manager
        .processes
        .get(&runtime_id)
        .filter(|process| process.owner_id == owner_id)
        .map(|process| process.generation);
    stop_runtime_process(&mut manager, &runtime_id)?;
    if let Some(generation) = stopped_generation {
        telemetry.record_process_lifecycle(
            "pi.process.stopped",
            &runtime_id,
            Some(generation),
            span_context.as_ref(),
            &[(
                "tau.process.stop_reason",
                TelemetryValue::String("explicit_stop".into()),
            )],
        );
        telemetry.record_pi_process_exit(true);
    }
    Ok(())
}

fn stop_runtime_process(manager: &mut PiManager, runtime_id: &str) -> Result<(), String> {
    let Some(process) = manager.processes.remove(runtime_id) else {
        return Ok(());
    };
    if let Err(error) = stop_process(&process) {
        manager.processes.insert(runtime_id.to_string(), process);
        return Err(error);
    }
    Ok(())
}

fn stop_stale_processes(
    manager: &mut PiManager,
) -> Result<Vec<StoppedRuntime>, (String, Vec<StoppedRuntime>)> {
    let runtimes = manager
        .processes
        .iter()
        .map(|(id, process)| (id.clone(), process.clone()))
        .collect::<Vec<_>>();
    let processes = runtimes
        .iter()
        .map(|(_, process)| process)
        .collect::<Vec<_>>();
    let results = stop_processes(&processes);
    let mut first_error = None;
    let mut stopped = Vec::new();
    for ((runtime_id, process), result) in runtimes.into_iter().zip(results) {
        match result {
            Ok(()) => {
                manager.processes.remove(&runtime_id);
                stopped.push(StoppedRuntime {
                    runtime_id,
                    generation: process.generation,
                });
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match first_error {
        Some(error) => Err((error, stopped)),
        None => Ok(stopped),
    }
}

fn stop_all_processes(processes: Vec<PiProcess>) {
    let _ = stop_processes(&processes.iter().collect::<Vec<_>>());
}

fn stop_process(process: &PiProcess) -> Result<(), String> {
    stop_processes(&[process]).remove(0)
}

fn stop_processes(processes: &[&PiProcess]) -> Vec<Result<(), String>> {
    for process in processes {
        process.close_stdin();
    }
    let children = processes
        .iter()
        .map(|process| &process.child)
        .collect::<Vec<_>>();
    stop_children(&children, PI_STOP_GRACE_PERIOD)
}

fn stop_child(child: &Arc<Mutex<Child>>) -> Result<(), String> {
    stop_children(&[child], Duration::ZERO).remove(0)
}

fn stop_children(
    children: &[&Arc<Mutex<Child>>],
    grace_period: Duration,
) -> Vec<Result<(), String>> {
    let started = Instant::now();
    let kill_after = started + grace_period;
    let deadline = started + PI_STOP_TIMEOUT;
    let mut results = vec![None; children.len()];
    let mut errors = vec![None; children.len()];
    let mut kill_sent = vec![false; children.len()];

    // All children share a deadline: quitting with many warm runtimes must not
    // wait through one grace period per SSH connection. A blocked stdin writer
    // cannot deliver EOF, so killing the direct child remains the fallback.
    loop {
        for (index, child) in children.iter().enumerate() {
            if results[index].is_some() {
                continue;
            }
            match child.try_lock() {
                Ok(mut child) => match child.try_wait() {
                    Ok(Some(_)) => results[index] = Some(Ok(())),
                    Ok(None) if !kill_sent[index] && Instant::now() >= kill_after => {
                        match child.kill() {
                            Ok(()) => kill_sent[index] = true,
                            Err(_) => {
                                errors[index] =
                                    Some("Tau could not stop Pi. Try again.".to_string());
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        errors[index] =
                            Some("Tau could not check whether Pi stopped. Try again.".to_string());
                    }
                },
                Err(std::sync::TryLockError::WouldBlock) => {}
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    results[index] =
                        Some(Err("Pi process state is unavailable. Try again.".into()));
                }
            }
        }
        if results.iter().all(Option::is_some) || Instant::now() >= deadline {
            return results
                .into_iter()
                .zip(errors)
                .map(|(result, error)| {
                    result.unwrap_or_else(|| {
                        Err(error.unwrap_or_else(|| "Pi did not stop in time. Try again.".into()))
                    })
                })
                .collect();
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn wait_for_child(child: &Arc<Mutex<Child>>) -> Option<ExitStatus> {
    loop {
        match child.try_lock() {
            Ok(mut child) => match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            },
            Err(std::sync::TryLockError::WouldBlock) => {}
            Err(std::sync::TryLockError::Poisoned(_)) => return None,
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn spawn_stdout_reader<R: Runtime>(
    context: PiReaderContext<R>,
    stdout: impl std::io::Read + Send + 'static,
    stderr_tail: StderrTail,
    stderr_reader: thread::JoinHandle<()>,
) {
    thread::spawn(move || {
        let PiReaderContext {
            app,
            manager,
            child,
            runtime_id,
            generation,
        } = context;
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    if bytes.last() == Some(&b'\n') {
                        bytes.pop();
                    }
                    if bytes.last() == Some(&b'\r') {
                        bytes.pop();
                    }
                    if bytes.is_empty() {
                        continue;
                    }
                    if let Some(reason) = line_drop_reason(&bytes) {
                        app.state::<Telemetry>().record_reader_event(
                            "pi.reader.line_dropped",
                            &runtime_id,
                            Some(generation),
                            &[(
                                "tau.reader.drop_reason",
                                TelemetryValue::String(reason.into()),
                            )],
                        );
                        continue;
                    }
                    let line = String::from_utf8_lossy(&bytes);
                    emit_pi_event(
                        &app,
                        PiEvent {
                            runtime_id: &runtime_id,
                            generation,
                            kind: "rpc",
                            line: Some(&line),
                            message: None,
                            code: None,
                        },
                    );
                }
                Err(error) => {
                    let message = error.to_string();
                    let telemetry = app.state::<Telemetry>();
                    telemetry.record_reader_event(
                        "pi.reader.failed",
                        &runtime_id,
                        Some(generation),
                        &[(
                            "tau.reader.error_kind",
                            TelemetryValue::String(io_error_kind_category(error.kind()).into()),
                        )],
                    );
                    telemetry.force_flush_logs();
                    emit_pi_event(
                        &app,
                        PiEvent {
                            runtime_id: &runtime_id,
                            generation,
                            kind: "error",
                            line: None,
                            message: Some(&message),
                            code: None,
                        },
                    );
                    break;
                }
            }
        }

        let status = wait_for_child(&child);
        // Descendants may inherit stderr after Pi exits. The tail is
        // best-effort, so they must not delay process cleanup or failure UI.
        drop(stderr_reader);
        let code = status.as_ref().and_then(|status| status.code());
        let succeeded = status.as_ref().is_some_and(|status| status.success());
        let should_emit = manager.lock().is_ok_and(|mut current| {
            if current
                .processes
                .get(&runtime_id)
                .is_some_and(|process| process.generation == generation)
            {
                current.processes.remove(&runtime_id);
                true
            } else {
                false
            }
        });
        if !should_emit {
            return;
        }
        let stderr = stderr_tail
            .lock()
            .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        let message = (!succeeded).then(|| pi_exit_message(code, &stderr));
        let exit_attributes: Vec<(&'static str, TelemetryValue)> = code
            .map(|code| vec![("tau.process.exit_code", TelemetryValue::I64(code as i64))])
            .unwrap_or_default();
        let telemetry = app.state::<Telemetry>();
        telemetry.record_process_lifecycle(
            "pi.process.exited",
            &runtime_id,
            Some(generation),
            None,
            &exit_attributes,
        );
        telemetry.record_pi_process_exit(false);
        telemetry.force_flush_logs();
        emit_pi_event(
            &app,
            PiEvent {
                runtime_id: &runtime_id,
                generation,
                kind: "exited",
                line: None,
                message: message.as_deref(),
                code,
            },
        );
    });
}

fn spawn_stderr_reader<R: Runtime>(
    context: PiReaderContext<R>,
    stderr: impl std::io::Read + Send + 'static,
    stderr_tail: StderrTail,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let PiReaderContext {
            app,
            runtime_id,
            generation,
            ..
        } = context;
        let mut reader = BufReader::new(stderr);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let line = line.trim_end_matches(['\n', '\r']);
                    if line.is_empty() {
                        continue;
                    }
                    let line = truncate_stderr_line(line);
                    if let Ok(mut tail) = stderr_tail.lock() {
                        if tail.len() == STDERR_TAIL_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line.clone());
                    }
                    emit_pi_event(
                        &app,
                        PiEvent {
                            runtime_id: &runtime_id,
                            generation,
                            kind: "stderr",
                            line: None,
                            message: Some(&line),
                            code: None,
                        },
                    );
                }
                Err(error) => {
                    let telemetry = app.state::<Telemetry>();
                    telemetry.record_reader_event(
                        "pi.reader.failed",
                        &runtime_id,
                        Some(generation),
                        &[(
                            "tau.reader.error_kind",
                            TelemetryValue::String(io_error_kind_category(error.kind()).into()),
                        )],
                    );
                    telemetry.force_flush_logs();
                    break;
                }
            }
        }
    })
}

fn truncate_stderr_line(line: &str) -> String {
    let mut characters = line.chars();
    let truncated = characters
        .by_ref()
        .take(STDERR_LINE_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn pi_exit_message(code: Option<i32>, stderr: &str) -> String {
    let summary = code.map_or_else(
        || "Pi terminated without an exit code.".to_string(),
        |code| format!("Pi exited with status {code}."),
    );
    let stderr = stderr.trim();
    if stderr.is_empty() {
        summary
    } else {
        format!("{summary} {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_session_arguments_keep_new_and_resumed_sessions_in_the_profile() {
        let profile = crate::profile::StorageProfile::ephemeral().unwrap();
        let directory = profile.session_dir("/imported/project", Path::new("/real/pi"));
        let mut fresh = Command::new("pi");
        configure_session_arguments(&mut fresh, Some(&directory), None);
        assert_eq!(
            fresh.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--session-dir"), directory.as_os_str()]
        );
        let saved = directory.join("saved.jsonl");
        let mut resumed = Command::new("pi");
        configure_session_arguments(&mut resumed, Some(&directory), saved.to_str());
        assert_eq!(
            resumed.get_args().collect::<Vec<_>>(),
            vec![
                OsStr::new("--session-dir"),
                directory.as_os_str(),
                OsStr::new("--session"),
                saved.as_os_str()
            ]
        );
    }

    #[test]
    fn production_session_arguments_preserve_pi_default_storage() {
        let mut fresh = Command::new("pi");
        configure_session_arguments(&mut fresh, None, None);
        assert_eq!(fresh.get_args().count(), 0);
        let mut resumed = Command::new("pi");
        configure_session_arguments(&mut resumed, None, Some("/real/pi/saved.jsonl"));
        assert_eq!(
            resumed.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("--session"), OsStr::new("/real/pi/saved.jsonl")]
        );
    }
    use serde_json::json;
    use std::sync::mpsc::{Receiver, TryRecvError};
    use tauri::{Listener, Manager};

    const TEST_SCENARIO_ENV: &str = "TAU_PI_TEST_SCENARIO";
    const TEST_FAULT_ENV: &str = "TAU_PI_TEST_FAULT";
    const TEST_PROJECT_ENV: &str = "TAU_PI_TEST_PROJECT_PATH";
    const TEST_SESSION_ENV: &str = "TAU_PI_TEST_SESSION_PATH";
    const EVENT_TIMEOUT: Duration = Duration::from_secs(10);
    static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

    struct EnvironmentGuard {
        previous: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvironmentGuard {
        fn set(values: &[(&'static str, Option<&OsStr>)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, env::var_os(key)))
                .collect();
            for (key, value) in values {
                if let Some(value) = value {
                    env::set_var(key, value);
                } else {
                    env::remove_var(key);
                }
            }
            Self { previous }
        }
    }

    impl Drop for EnvironmentGuard {
        fn drop(&mut self) {
            for (key, value) in self.previous.drain(..) {
                if let Some(value) = value {
                    env::set_var(key, value);
                } else {
                    env::remove_var(key);
                }
            }
        }
    }

    #[test]
    fn configured_pi_path_takes_priority() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let current = std::env::current_exe().expect("current executable");
        let _environment = EnvironmentGuard::set(&[("TAU_PI_PATH", Some(current.as_os_str()))]);
        assert_eq!(resolve_pi_binary(), Some(current));
    }

    #[test]
    fn fake_pi_round_trips_jsonl_and_cleans_up_after_successful_exit() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let fixture = NativePiFixture::new(None);
        let (app, events) = fixture.app_and_events();
        let handle = app.handle().clone();
        let state = handle.state::<PiState>();
        let telemetry = handle.state::<Telemetry>();
        claim_for_test(&state, "owner-a");

        let generation = start_pi_with(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-a".into(),
            "native-main".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
        )
        .expect("start fake Pi");
        assert_eq!(generation, 1);
        let started = expect_outer_event(&events, "started", generation);
        assert_eq!(started["runtimeId"], "native-main");

        for (id, request_type) in [
            ("models", "get_available_models"),
            ("commands", "get_commands"),
            ("state", "get_state"),
            ("efforts", "get_available_thinking_levels"),
            ("messages", "get_messages"),
        ] {
            send_pi_to_state(
                &state,
                "owner-a",
                "native-main".into(),
                json!({
                    "id": id,
                    "type": request_type,
                }),
            )
            .expect("send bootstrap request");
            expect_rpc(&events, generation, "response", Some(id));
        }

        send_pi_to_state(
            &state,
            "owner-a",
            "native-main".into(),
            json!({"id": "prompt", "type": "prompt", "message": "Explain the fixture"}),
        )
        .expect("send prompt");
        expect_rpc(&events, generation, "agent_start", None);

        send_pi_to_state(
            &state,
            "owner-a",
            "native-main".into(),
            json!({"id": "run-state", "type": "get_state"}),
        )
        .expect("send running state request");
        expect_rpc(&events, generation, "response", Some("run-state"));
        expect_rpc(&events, generation, "message_update", None);
        expect_rpc(&events, generation, "message_update", None);
        expect_rpc(&events, generation, "response", Some("prompt"));
        expect_rpc(&events, generation, "agent_settled", None);

        for (id, request_type) in [
            ("settled-state", "get_state"),
            ("settled-efforts", "get_available_thinking_levels"),
            ("settled-messages", "get_messages"),
        ] {
            send_pi_to_state(
                &state,
                "owner-a",
                "native-main".into(),
                json!({
                    "id": id,
                    "type": request_type,
                }),
            )
            .expect("send settlement request");
            expect_rpc(&events, generation, "response", Some(id));
        }

        let exited = expect_outer_event(&events, "exited", generation);
        assert_eq!(exited["runtimeId"], "native-main");
        assert_eq!(exited["code"], 0);
        assert!(exited.get("message").is_none());
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
    }

    #[test]
    fn frontend_takeover_stops_stale_runtime_before_replacement_starts() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let fixture = NativePiFixture::new(None);
        let (app, events) = fixture.app_and_events();
        let handle = app.handle().clone();
        let state = handle.state::<PiState>();
        let telemetry = handle.state::<Telemetry>();
        claim_for_test(&state, "owner-a");

        let first_generation = start_pi_with(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-a".into(),
            "runtime-a".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
        )
        .expect("start first frontend runtime");
        expect_outer_event(&events, "started", first_generation);
        let old_child =
            Arc::clone(&state.inner.lock().expect("Pi manager").processes["runtime-a"].child);

        let outcome = claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1)
            .expect("replace frontend owner");
        record_ownership_cleanup(&telemetry, None, &outcome.stopped);
        telemetry.record_ownership_event("replacement", "success", outcome.stopped.len(), None);
        assert!(old_child
            .lock()
            .expect("old child")
            .try_wait()
            .expect("old child status")
            .is_some());
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());

        let second_generation = start_pi_with(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-b".into(),
            "runtime-b".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
        )
        .expect("start replacement frontend runtime");
        assert!(second_generation > first_generation);
        // EOF lets the scenario adapter report its intentionally incomplete
        // script on stderr before exiting; these belong to the old generation.
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let event = events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("replacement started event");
            if event["generation"] == first_generation {
                assert_eq!(event["kind"], "stderr");
                continue;
            }
            assert_eq!(event["generation"], second_generation);
            assert_eq!(event["kind"], "started");
            break;
        }
        assert!(send_pi_to_state(
            &state,
            "owner-a",
            "runtime-b".into(),
            json!({"id": "stale", "type": "get_state"}),
        )
        .expect_err("stale owner must be fenced")
        .contains("no longer owns"));
        assert!(state
            .inner
            .lock()
            .expect("Pi manager")
            .processes
            .contains_key("runtime-b"));
        let logs = std::fs::read_to_string(fixture._root.path().join("telemetry/logs.jsonl"))
            .expect("ownership telemetry logs");
        assert_eq!(
            logs.lines()
                .filter(|line| line.contains("pi.process.stopped"))
                .count(),
            1
        );
        assert!(logs.contains("ownership_replaced"));
        assert!(logs.contains("pi.ownership.claimed"));
        assert!(!logs.contains("owner-a"));
        state.shutdown();
    }

    #[test]
    fn replacing_a_runtime_waits_for_eof_before_starting_its_successor() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let fixture = NativePiFixture::new(None);
        let (app, _events) = fixture.app_and_events();
        let handle = app.handle().clone();
        let state = handle.state::<PiState>();
        let telemetry = handle.state::<Telemetry>();
        claim_for_test(&state, "owner-a");
        let start = || {
            let mut command = Command::new("sh");
            command.args(["-c", "while IFS= read -r line; do :; done; exit 0"]);
            spawn_command(
                handle.clone(),
                &state,
                &telemetry,
                None,
                "owner-a".into(),
                "runtime-a".into(),
                command,
            )
            .expect("start EOF-aware runtime")
        };
        let first_generation = start();
        let first = state.inner.lock().expect("Pi manager").processes["runtime-a"].clone();

        let second_generation = start();

        assert!(second_generation > first_generation);
        assert_clean_exit(&first);
        let second = state.inner.lock().expect("Pi manager").processes["runtime-a"].clone();
        assert!(second.usable.load(Ordering::Acquire));
        state.shutdown();
        assert_clean_exit(&second);
    }

    #[test]
    fn frontend_takeover_retries_kill_after_child_lock_contention() {
        let state = PiState::default();
        claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0).expect("first claim");
        let process = sleeping_process(1);
        let child = Arc::clone(&process.child);
        state
            .inner
            .lock()
            .expect("Pi manager")
            .processes
            .insert("runtime-a".into(), process);
        let (locked_sender, locked_receiver) = mpsc::channel();
        let holder = thread::spawn(move || {
            let _guard = child.lock().expect("child lock");
            locked_sender.send(()).expect("report child lock");
            thread::sleep(PI_STOP_GRACE_PERIOD + Duration::from_millis(50));
        });
        locked_receiver.recv().expect("wait for child lock");

        claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1)
            .expect("takeover after child lock contention");

        holder.join().expect("child lock holder");
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
    }

    #[test]
    fn delayed_claim_cannot_roll_back_current_ownership() {
        let state = PiState::default();
        claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0).expect("first claim");
        claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1).expect("second claim");

        let error = claim_pi_frontend_inner(&state.inner, "owner-a".into(), 0)
            .expect_err("old revision must be rejected");
        assert_eq!(error.kind, "conflict");
        assert!(error.message.contains("already owns"));
        let manager = state.inner.lock().expect("Pi manager");
        assert!(manager.require_owner("owner-b").is_ok());
        assert!(manager.require_owner("owner-a").is_err());
    }

    #[test]
    fn fake_pi_malformed_stdout_is_dropped_and_lifecycle_is_cleaned_up() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let fixture = NativePiFixture::new(Some(OsStr::new("malformed-stdout")));
        let (app, events) = fixture.app_and_events();
        let handle = app.handle().clone();
        let state = handle.state::<PiState>();
        let telemetry = handle.state::<Telemetry>();
        claim_for_test(&state, "owner-a");

        let generation = start_pi_with(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-a".into(),
            "native-malformed".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
        )
        .expect("start fake Pi");
        let started = expect_outer_event(&events, "started", generation);
        assert_eq!(started["runtimeId"], "native-malformed");
        let exited = expect_outer_event(&events, "exited", generation);
        assert_eq!(exited["runtimeId"], "native-malformed");
        assert_eq!(exited["code"], 0);
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
        assert_eq!(events.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn fake_pi_nonzero_exit_forwards_bounded_failure_and_cleans_up() {
        let _lock = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let fixture = NativePiFixture::new(Some(OsStr::new("nonzero-exit")));
        let (app, events) = fixture.app_and_events();
        let handle = app.handle().clone();
        let state = handle.state::<PiState>();
        let telemetry = handle.state::<Telemetry>();
        claim_for_test(&state, "owner-a");

        let generation = start_pi_with(
            handle.clone(),
            &state,
            &telemetry,
            None,
            "owner-a".into(),
            "native-failure".into(),
            fixture.project_string(),
            Some(fixture.session_string()),
        )
        .expect("start fake Pi");
        let started = expect_outer_event(&events, "started", generation);
        assert_eq!(started["runtimeId"], "native-failure");
        let stderr = expect_outer_event(&events, "stderr", generation);
        assert_eq!(stderr["message"], "scripted transport failure");
        let exited = expect_outer_event(&events, "exited", generation);
        assert_eq!(exited["runtimeId"], "native-failure");
        assert_eq!(exited["code"], 23);
        assert_eq!(
            exited["message"],
            "Pi exited with status 23. scripted transport failure"
        );
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
    }

    struct NativePiFixture {
        _root: tempfile::TempDir,
        project: PathBuf,
        session: PathBuf,
        _environment: EnvironmentGuard,
    }

    impl NativePiFixture {
        fn new(fault: Option<&OsStr>) -> Self {
            let root = tempfile::tempdir().expect("native Pi fixture directory");
            let project = root.path().join("project");
            std::fs::create_dir(&project).expect("fixture project");
            let project = project.canonicalize().expect("canonical fixture project");
            let session = project.join("session.jsonl");
            std::fs::write(&session, "").expect("fixture session");
            let session = session.canonicalize().expect("canonical fixture session");
            let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../scripts/pi-stdio-adapter.ts")
                .canonicalize()
                .expect("fake Pi adapter");
            #[cfg(dev)]
            let session_dir =
                Some(crate::storage::default_session_dir(&project.to_string_lossy()).unwrap());
            #[cfg(not(dev))]
            let session_dir: Option<PathBuf> = None;
            let environment = EnvironmentGuard::set(&[
                ("TAU_PI_PATH", Some(adapter.as_os_str())),
                (
                    TEST_SCENARIO_ENV,
                    Some(OsStr::new("saved-session-conversation")),
                ),
                (TEST_FAULT_ENV, fault),
                (TEST_PROJECT_ENV, Some(project.as_os_str())),
                (TEST_SESSION_ENV, Some(session.as_os_str())),
                (
                    "TAU_PI_TEST_SESSION_DIR",
                    session_dir.as_deref().map(Path::as_os_str),
                ),
            ]);
            Self {
                _root: root,
                project,
                session,
                _environment: environment,
            }
        }

        fn app_and_events(
            &self,
        ) -> (
            tauri::App<tauri::test::MockRuntime>,
            Receiver<serde_json::Value>,
        ) {
            let telemetry_dir = self._root.path().join("telemetry");
            let app = tauri::test::mock_builder()
                .manage(PiState::default())
                .manage(Telemetry::for_test(telemetry_dir))
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .expect("mock Tauri app");
            let (sender, receiver) = mpsc::channel();
            app.listen("pi-event", move |event| {
                let value = serde_json::from_str(event.payload()).expect("serialized Pi event");
                let _ = sender.send(value);
            });
            (app, receiver)
        }

        fn project_string(&self) -> String {
            self.project.to_string_lossy().into_owned()
        }

        fn session_string(&self) -> String {
            self.session.to_string_lossy().into_owned()
        }
    }

    fn claim_for_test(state: &PiState, owner_id: &str) {
        claim_pi_frontend_inner(&state.inner, owner_id.into(), 0).expect("claim Pi owner");
    }

    fn expect_outer_event(
        events: &Receiver<serde_json::Value>,
        kind: &str,
        generation: u64,
    ) -> serde_json::Value {
        let event = events
            .recv_timeout(EVENT_TIMEOUT)
            .unwrap_or_else(|error| panic!("waiting for {kind} Pi event: {error}"));
        assert_eq!(event["generation"], generation);
        assert_eq!(event["kind"], kind);
        event
    }

    fn expect_rpc(
        events: &Receiver<serde_json::Value>,
        generation: u64,
        rpc_type: &str,
        id: Option<&str>,
    ) {
        let event = expect_outer_event(events, "rpc", generation);
        let line: serde_json::Value =
            serde_json::from_str(event["line"].as_str().expect("RPC event line"))
                .expect("production-shaped RPC JSON");
        assert_eq!(line["type"], rpc_type);
        if let Some(id) = id {
            assert_eq!(line["id"], id);
        }
    }

    #[test]
    fn child_path_prepends_and_deduplicates_executable_directories() {
        let path = build_child_path(
            &[
                Path::new("/opt/homebrew/bin/pi"),
                Path::new("/opt/homebrew/bin/node"),
            ],
            None,
            Some(OsStr::new("/usr/bin:/bin")),
        )
        .expect("child PATH");
        assert_eq!(
            env::split_paths(&path).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ],
        );
    }

    #[test]
    fn child_path_places_login_shell_directories_before_the_inherited_path() {
        let path = build_child_path(
            &[Path::new("/opt/homebrew/bin/pi")],
            Some(OsStr::new("/Users/tau/.bun/bin:/usr/bin")),
            Some(OsStr::new("/usr/bin:/bin")),
        )
        .expect("child PATH");
        assert_eq!(
            env::split_paths(&path).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/Users/tau/.bun/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ],
        );
    }

    #[test]
    fn login_shell_path_is_read_from_between_the_markers() {
        let output = format!(
            "startup banner\n{LOGIN_SHELL_PATH_MARKER}\n/Users/tau/.bun/bin:/usr/bin\n{LOGIN_SHELL_AGENT_DIR_MARKER}\n/Users/tau/.config/pi\n{LOGIN_SHELL_END_MARKER}\n"
        );
        assert_eq!(
            parse_login_shell_path(&output),
            Some(OsString::from("/Users/tau/.bun/bin:/usr/bin")),
        );
        assert_eq!(
            parse_login_shell_environment(&output)
                .expect("login shell environment")
                .agent_dir,
            Some(OsString::from("/Users/tau/.config/pi")),
        );
    }

    #[test]
    fn every_login_shell_argument_set_ends_with_a_command_flag() {
        for arguments in LOGIN_SHELL_ARGUMENTS
            .into_iter()
            .chain(CSH_LOGIN_SHELL_ARGUMENTS)
        {
            let last = arguments.last().expect("argument");
            assert!(
                last.ends_with('c'),
                "{last} would not treat the next argument as a command",
            );
        }
    }

    #[test]
    fn executables_are_found_on_the_login_shell_path() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let executable = directory.path().join("pi");
        std::fs::write(&executable, "").expect("executable");
        #[cfg(unix)]
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable permissions");
        let path =
            env::join_paths([Path::new("/nonexistent"), directory.path()]).expect("search PATH");

        assert_eq!(find_on_path("pi", Some(&path)), Some(executable));
    }

    #[cfg(unix)]
    #[test]
    fn files_without_execute_permission_are_not_executables() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("pi");
        std::fs::write(&path, "").expect("plain file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("plain file permissions");
        assert!(!is_executable_file(&path));
    }

    #[test]
    fn login_shell_path_ignores_output_without_markers() {
        assert_eq!(parse_login_shell_path("command not found"), None);
        assert_eq!(
            parse_login_shell_path(&format!(
                "{LOGIN_SHELL_PATH_MARKER}  {LOGIN_SHELL_AGENT_DIR_MARKER}  {LOGIN_SHELL_END_MARKER}"
            )),
            None,
        );
    }

    #[test]
    fn capturing_stdout_gives_up_on_a_hanging_shell() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec 1>&-; sleep 30"]);
        assert_eq!(capture_stdout(command, Duration::from_millis(200)), None);
    }

    #[test]
    fn login_shell_output_is_bounded() {
        assert_eq!(read_bounded_stdout(&b"12345"[..], 4), None);
        assert_eq!(read_bounded_stdout(&b"1234"[..], 4), Some(b"1234".to_vec()),);
    }

    #[test]
    fn exit_message_includes_status_and_stderr() {
        assert_eq!(
            pi_exit_message(Some(127), "env: node: No such file or directory"),
            "Pi exited with status 127. env: node: No such file or directory",
        );
    }

    #[test]
    fn io_error_kinds_map_to_the_reviewed_category_set() {
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::BrokenPipe),
            "broken_pipe"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::Interrupted),
            "interrupted"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::UnexpectedEof),
            "unexpected_eof"
        );
        assert_eq!(
            io_error_kind_category(std::io::ErrorKind::PermissionDenied),
            "other"
        );
    }

    #[test]
    fn malformed_and_oversized_rpc_lines_map_to_reviewed_drop_reasons() {
        assert_eq!(line_drop_reason(br#"{"type":"response"}"#), None);
        assert_eq!(line_drop_reason(b"not json"), Some("malformed"));
        assert_eq!(
            line_drop_reason(br#"["not", "an", "object"]"#),
            Some("malformed")
        );
        assert_eq!(line_drop_reason(&[0xff]), Some("invalid_utf8"));
        assert_eq!(
            line_drop_reason(&vec![b'x'; MAX_RPC_LINE_BYTES + 1]),
            Some("oversized")
        );
    }

    fn eof_process(generation: u64) -> PiProcess {
        scripted_process(
            "while IFS= read -r line; do :; done; sleep 0.05; exit 0",
            generation,
        )
    }

    fn assert_clean_exit(process: &PiProcess) {
        assert!(process
            .child
            .lock()
            .expect("child lock")
            .try_wait()
            .expect("child status")
            .expect("child exited")
            .success());
        assert!(!process.usable.load(Ordering::Acquire));
        assert!(process.writer.lock().expect("writer lock").is_none());
    }

    #[test]
    fn stopping_closes_stdin_even_with_retained_process_handles() {
        let process = eof_process(1);
        let retained = process.clone();
        send_pi_line(&process, b"request\n".to_vec(), PI_WRITE_TIMEOUT).expect("write before stop");

        stop_process(&process).expect("stop on EOF");

        assert_clean_exit(&retained);
        assert!(send_pi_line(&retained, b"late\n".to_vec(), PI_WRITE_TIMEOUT).is_err());
        stop_process(&retained).expect("repeated stop");
    }

    #[test]
    fn frontend_takeover_delivers_eof_to_every_stale_runtime() {
        let state = PiState::default();
        claim_for_test(&state, "owner-a");
        let first = eof_process(1);
        let second = eof_process(2);
        {
            let mut manager = state.inner.lock().expect("Pi manager");
            manager.processes.insert("first".into(), first.clone());
            manager.processes.insert("second".into(), second.clone());
        }

        let outcome = claim_pi_frontend_inner(&state.inner, "owner-b".into(), 1)
            .expect("take over after EOF");

        assert_eq!(outcome.stopped.len(), 2);
        assert_clean_exit(&first);
        assert_clean_exit(&second);
        assert!(state.inner.lock().expect("Pi manager").processes.is_empty());
    }

    #[test]
    fn failed_takeover_retains_only_children_that_could_not_be_stopped() {
        let mut manager = PiManager::default();
        let responsive = eof_process(1);
        let locked = sleeping_process(2);
        manager
            .processes
            .insert("responsive".into(), responsive.clone());
        manager.processes.insert("locked".into(), locked.clone());
        let guard = locked.child.lock().expect("hold child lock");

        let (_, stopped) = stop_stale_processes(&mut manager).expect_err("locked child times out");

        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].runtime_id, "responsive");
        assert_clean_exit(&responsive);
        assert_eq!(manager.processes.len(), 1);
        assert!(manager.processes.contains_key("locked"));
        drop(guard);
        stop_stale_processes(&mut manager).expect("retry cleanup");
        assert!(manager.processes.is_empty());
    }

    #[test]
    fn app_shutdown_delivers_eof_to_all_managed_children() {
        let state = PiState::default();
        let processes = [eof_process(1), eof_process(2)];
        {
            let mut manager = state.inner.lock().expect("Pi manager");
            for (index, process) in processes.iter().enumerate() {
                manager.processes.insert(index.to_string(), process.clone());
            }
        }

        state.shutdown();

        for process in &processes {
            assert_clean_exit(process);
        }
        let manager = state.inner.lock().expect("Pi manager");
        assert!(matches!(manager.ownership, PiOwnership::ShuttingDown));
        assert!(manager.processes.is_empty());
    }

    #[test]
    fn app_shutdown_does_not_wait_for_an_in_flight_write_acknowledgement() {
        let state = Arc::new(PiState::default());
        claim_for_test(&state, "owner-a");
        let process = sleeping_process(1);
        let (sender, requests) = mpsc::sync_channel(1);
        *process.writer.lock().expect("writer lock") = Some(sender);
        state
            .inner
            .lock()
            .expect("Pi manager")
            .processes
            .insert("runtime-a".into(), process.clone());
        let sending_state = Arc::clone(&state);
        let sending = thread::spawn(move || {
            send_pi_to_state(
                &sending_state,
                "owner-a",
                "runtime-a".into(),
                json!({"type": "get_state"}),
            )
        });
        let pending = requests
            .recv_timeout(PI_WRITE_TIMEOUT)
            .expect("write awaiting acknowledgement");
        let started = Instant::now();

        state.shutdown();

        assert!(started.elapsed() < PI_STOP_TIMEOUT);
        assert!(process
            .child
            .lock()
            .expect("child lock")
            .try_wait()
            .expect("child status")
            .is_some());
        drop(pending);
        assert!(sending.join().expect("sending thread").is_err());
    }

    #[test]
    fn app_shutdown_uses_one_grace_period_for_unresponsive_children() {
        let state = PiState::default();
        let processes = (0..4).map(sleeping_process).collect::<Vec<_>>();
        {
            let mut manager = state.inner.lock().expect("Pi manager");
            for (index, process) in processes.iter().enumerate() {
                manager.processes.insert(index.to_string(), process.clone());
            }
        }
        let started = Instant::now();

        state.shutdown();

        assert!(started.elapsed() >= PI_STOP_GRACE_PERIOD);
        assert!(started.elapsed() < PI_STOP_TIMEOUT);
        for process in &processes {
            let status = process
                .child
                .lock()
                .expect("child lock")
                .try_wait()
                .expect("child status")
                .expect("child reaped");
            assert!(!status.success());
        }
    }

    #[test]
    fn stopping_one_runtime_keeps_other_processes() {
        let mut manager = PiManager::default();
        let first = eof_process(1);
        manager.processes.insert("first".into(), first.clone());
        manager
            .processes
            .insert("second".into(), sleeping_process(2));

        stop_runtime_process(&mut manager, "first").expect("stop first runtime");

        assert_clean_exit(&first);
        assert!(!manager.processes.contains_key("first"));
        let second = &manager.processes["second"];
        assert!(second.usable.load(Ordering::Acquire));
        assert!(second
            .child
            .lock()
            .expect("second child")
            .try_wait()
            .expect("second status")
            .is_none());
        let processes = manager
            .processes
            .drain()
            .map(|(_, process)| process)
            .collect();
        stop_all_processes(processes);
        assert!(manager.processes.is_empty());
    }

    fn sleeping_process(generation: u64) -> PiProcess {
        scripted_process("exec sleep 60", generation)
    }

    fn scripted_process(script: &str, generation: u64) -> PiProcess {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::piped())
            .spawn()
            .expect("scripted process");
        let stdin = child.stdin.take().expect("scripted stdin");
        PiProcess {
            owner_id: "owner-a".into(),
            generation,
            child: Arc::new(Mutex::new(child)),
            writer: spawn_stdin_writer(stdin).expect("stdin writer"),
            usable: Arc::new(AtomicBool::new(true)),
        }
    }

    #[test]
    fn a_blocked_pi_write_returns_within_its_timeout() {
        let process = sleeping_process(1);
        let started = Instant::now();

        let error = send_pi_line(
            &process,
            vec![b'x'; 1024 * 1024],
            Duration::from_millis(100),
        )
        .expect_err("an unread pipe should time out");

        assert_eq!(error, "Pi did not accept the request in time. Try again.");
        assert!(started.elapsed() < PI_STOP_TIMEOUT);
        assert!(process.writer.lock().expect("writer lock").is_none());
        assert_eq!(
            send_pi_line(&process, b"retry\n".to_vec(), Duration::from_millis(100))
                .expect_err("a timed-out runtime stays unavailable"),
            "Pi is no longer accepting requests. Restart Tau and try again.",
        );
        stop_process(&process).expect("timed-out process is stopped");
    }

    #[test]
    fn waiting_for_exit_does_not_block_process_termination() {
        let process = sleeping_process(1);
        let child = Arc::clone(&process.child);
        let waiter = thread::spawn(move || wait_for_child(&child));
        thread::sleep(Duration::from_millis(25));
        let started = Instant::now();

        stop_process(&process).expect("stop process while its exit is watched");

        assert!(started.elapsed() < PI_STOP_TIMEOUT);
        assert!(waiter.join().expect("exit watcher").is_some());
    }
}
