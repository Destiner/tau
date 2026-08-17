use crate::ssh::{remote_pi_command, SshConnection};
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use opentelemetry::Value as TelemetryValue;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    env,
    ffi::{OsStr, OsString},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

const MAX_RPC_LINE_BYTES: usize = 64 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 8;
const STDERR_LINE_CHARS: usize = 2048;
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_SHELL_PATH_MARKER: &str = "__TAU_PATH__";
/// Shells disagree about flags: tcsh rejects `-l` unless it stands alone, and
/// shells that are not POSIX-like may reject bundling altogether. The
/// combinations are ordered from most to least of the user's configuration.
const LOGIN_SHELL_ARGUMENTS: [&[&str]; 4] = [&["-lic"], &["-i", "-c"], &["-lc"], &["-c"]];

type StderrTail = Arc<Mutex<VecDeque<String>>>;

#[derive(Default)]
pub struct PiState {
    inner: Arc<Mutex<PiManager>>,
}

impl Drop for PiState {
    fn drop(&mut self) {
        if let Ok(mut manager) = self.inner.lock() {
            stop_all_processes(&mut manager);
        }
    }
}

#[derive(Default)]
struct PiManager {
    generation: u64,
    processes: HashMap<String, PiProcess>,
}

struct PiProcess {
    generation: u64,
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
}

#[derive(Clone)]
struct PiReaderContext {
    app: AppHandle,
    manager: Arc<Mutex<PiManager>>,
    child: Arc<Mutex<Child>>,
    runtime_id: String,
    generation: u64,
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

    if let Ok(output) = Command::new("which").arg("pi").output() {
        if output.status.success() {
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if is_executable_file(&path) {
                return Some(path);
            }
        }
    }

    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/pi"),
        PathBuf::from("/usr/local/bin/pi"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/pi"));
        candidates.push(home.join(".bun/bin/pi"));
    }
    if let Some(path) = candidates.into_iter().find(|path| is_executable_file(path)) {
        return Some(path);
    }

    find_on_login_shell_path("pi")
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
    if let Ok(output) = Command::new("which").arg(executable).output() {
        if output.status.success() {
            let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            if is_executable_file(&path) {
                return Some(path);
            }
        }
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
    find_on_login_shell_path(executable)
}

/// Searching the login shell's PATH ourselves avoids depending on a lookup
/// command whose spelling differs between shells.
fn find_on_login_shell_path(executable: &str) -> Option<PathBuf> {
    let path = login_shell_path()?;
    env::split_paths(path)
        .map(|directory| directory.join(executable))
        .find(|path| is_executable_file(path))
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_pi(
    app: AppHandle,
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    runtime_id: String,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    validate_start_paths(&project_path, session_path.as_deref())?;
    let command_span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "start_pi"));
    let span_context = command_span.as_ref().map(|span| span.span_context());

    let resolved_pi_path = resolve_pi_binary();
    telemetry.record_process_lifecycle(
        "pi.process.resolved",
        &runtime_id,
        None,
        span_context.as_ref(),
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
    if let Some(path) = session_path {
        command.args(["--session", &path]);
    }
    spawn_command(
        app,
        state,
        &telemetry,
        span_context.as_ref(),
        runtime_id,
        command,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_pi_remote(
    app: AppHandle,
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
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
        state,
        &telemetry,
        span_context.as_ref(),
        runtime_id,
        connection.command(&remote_command),
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
    Ok(())
}

/// Launchd gives GUI apps a minimal PATH, so toolchains installed by version
/// managers (bun, volta, cargo) are invisible to Pi and to the shell commands
/// it runs. Ask the login shell for the PATH a terminal would have. The shell
/// must be interactive as well as login: zsh only reads `.zshrc`, where such
/// toolchains usually register themselves, when it is interactive.
fn login_shell_path() -> Option<&'static OsString> {
    static PATH: OnceLock<Option<OsString>> = OnceLock::new();
    PATH.get_or_init(query_login_shell_path).as_ref()
}

fn query_login_shell_path() -> Option<OsString> {
    // Windows processes already inherit a usable PATH, and none of these flags
    // mean anything to its shells.
    if cfg!(windows) {
        return None;
    }
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    // `printenv` reports the exported variable, which is colon separated for
    // every shell because that is what the process environment holds.
    // Interpolating `$PATH` instead would yield a space separated list in fish
    // and csh, whose PATH is a shell array.
    let script =
        format!("echo {LOGIN_SHELL_PATH_MARKER}; printenv PATH; echo {LOGIN_SHELL_PATH_MARKER}");
    LOGIN_SHELL_ARGUMENTS.into_iter().find_map(|arguments| {
        let mut command = Command::new(&shell);
        command.args(arguments).arg(&script);
        let output = capture_stdout(command, LOGIN_SHELL_TIMEOUT)?;
        parse_login_shell_path(&String::from_utf8_lossy(&output))
    })
}

/// Startup files are free to print banners, so the PATH is read from between
/// markers rather than from the surrounding output.
fn parse_login_shell_path(output: &str) -> Option<OsString> {
    let path = output.split(LOGIN_SHELL_PATH_MARKER).nth(1)?.trim();
    (!path.is_empty()).then(|| OsString::from(path))
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
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        let _ = sender.send(buffer);
    });
    let output = receiver.recv_timeout(timeout).ok();
    if output.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    output
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
fn emit_pi_event(app: &AppHandle, event: PiEvent<'_>) {
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

fn spawn_command(
    app: AppHandle,
    state: State<'_, PiState>,
    telemetry: &Telemetry,
    span_context: Option<&opentelemetry::trace::SpanContext>,
    runtime_id: String,
    mut command: Command,
) -> Result<u64, String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    let replaced_generation = manager.processes.get(&runtime_id).map(|p| p.generation);
    stop_runtime_process(&mut manager, &runtime_id);
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
    let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));

    manager.processes.insert(
        runtime_id.clone(),
        PiProcess {
            generation,
            child: Arc::clone(&child),
            stdin: Arc::new(Mutex::new(stdin)),
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
pub fn send_pi(
    state: State<'_, PiState>,
    runtime_id: String,
    request: Value,
) -> Result<(), String> {
    let line = serde_json::to_vec(&request)
        .map_err(|error| format!("Could not encode the Pi request: {error}"))?;
    if line.len() > MAX_RPC_LINE_BYTES {
        return Err("The Pi request is too large.".into());
    }

    let stdin = {
        let manager = state
            .inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        manager
            .processes
            .get(&runtime_id)
            .map(|process| Arc::clone(&process.stdin))
            .ok_or_else(|| "The selected Pi runtime is not running.".to_string())?
    };
    let mut stdin = stdin
        .lock()
        .map_err(|_| "Pi input stream is unavailable.".to_string())?;
    stdin
        .write_all(&line)
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Could not send a request to pi: {error}"))
}

#[tauri::command]
pub fn stop_pi(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
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
    let stopped_generation = manager.processes.get(&runtime_id).map(|p| p.generation);
    stop_runtime_process(&mut manager, &runtime_id);
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

fn stop_runtime_process(manager: &mut PiManager, runtime_id: &str) {
    if let Some(process) = manager.processes.remove(runtime_id) {
        stop_process(process);
    }
}

fn stop_all_processes(manager: &mut PiManager) {
    for (_, process) in manager.processes.drain() {
        stop_process(process);
    }
}

fn stop_process(process: PiProcess) {
    if let Ok(mut child) = process.child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn_stdout_reader(
    context: PiReaderContext,
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

        let status = child.lock().ok().and_then(|mut child| child.wait().ok());
        let _ = stderr_reader.join();
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

fn spawn_stderr_reader(
    context: PiReaderContext,
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
    fn configured_pi_path_takes_priority() {
        let current = std::env::current_exe().expect("current executable");
        std::env::set_var("TAU_PI_PATH", &current);
        assert_eq!(resolve_pi_binary(), Some(current));
        std::env::remove_var("TAU_PI_PATH");
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
            "startup banner\n{LOGIN_SHELL_PATH_MARKER}\n/Users/tau/.bun/bin:/usr/bin\n{LOGIN_SHELL_PATH_MARKER}\n"
        );
        assert_eq!(
            parse_login_shell_path(&output),
            Some(OsString::from("/Users/tau/.bun/bin:/usr/bin")),
        );
    }

    #[test]
    fn every_login_shell_argument_set_ends_with_a_command_flag() {
        for arguments in LOGIN_SHELL_ARGUMENTS {
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
        std::fs::write(directory.path().join("pi"), "").expect("executable");
        let path =
            env::join_paths([Path::new("/nonexistent"), directory.path()]).expect("search PATH");

        let found = env::split_paths(&path)
            .map(|candidate| candidate.join("pi"))
            .find(|candidate| is_executable_file(candidate));

        assert_eq!(found, Some(directory.path().join("pi")));
    }

    #[test]
    fn login_shell_path_ignores_output_without_markers() {
        assert_eq!(parse_login_shell_path("command not found"), None);
        assert_eq!(
            parse_login_shell_path(&format!(
                "{LOGIN_SHELL_PATH_MARKER}  {LOGIN_SHELL_PATH_MARKER}"
            )),
            None,
        );
    }

    #[test]
    fn capturing_stdout_gives_up_on_a_hanging_shell() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        assert_eq!(capture_stdout(command, Duration::from_millis(200)), None);
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

    #[test]
    fn stopping_one_runtime_keeps_other_processes() {
        let mut manager = PiManager::default();
        manager
            .processes
            .insert("first".into(), sleeping_process(1));
        manager
            .processes
            .insert("second".into(), sleeping_process(2));

        stop_runtime_process(&mut manager, "first");

        assert!(!manager.processes.contains_key("first"));
        assert!(manager.processes.contains_key("second"));
        stop_all_processes(&mut manager);
        assert!(manager.processes.is_empty());
    }

    fn sleeping_process(generation: u64) -> PiProcess {
        let mut child = Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("sleep process");
        let stdin = child.stdin.take().expect("sleep stdin");
        PiProcess {
            generation,
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
        }
    }
}
