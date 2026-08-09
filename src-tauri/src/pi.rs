use crate::ssh::{remote_pi_command, SshConnection};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Emitter, State};

const MAX_RPC_LINE_BYTES: usize = 64 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 8;
const STDERR_LINE_CHARS: usize = 2048;

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

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = Command::new(shell)
        .args(["-lc", "command -v pi"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    is_executable_file(&path).then_some(path)
}

pub fn sdk_available() -> bool {
    let Some(pi_path) = resolve_pi_binary() else {
        return false;
    };
    resolve_node_binary().is_some() && resolve_pi_sdk_entry(&pi_path).is_some()
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

fn resolve_pi_sdk_entry(pi_path: &Path) -> Option<PathBuf> {
    let resolved = pi_path.canonicalize().ok()?;
    let entry = resolved.parent()?.join("index.js");
    entry.is_file().then_some(entry)
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
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = Command::new(shell)
        .args(["-lc", &format!("command -v {executable}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    is_executable_file(&path).then_some(path)
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[tauri::command]
pub fn start_pi(
    app: AppHandle,
    state: State<'_, PiState>,
    runtime_id: String,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    validate_start_paths(&project_path, session_path.as_deref())?;

    let pi_path = resolve_pi_binary()
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
    spawn_command(app, state, runtime_id, command)
}

#[tauri::command]
pub fn start_pi_remote(
    app: AppHandle,
    state: State<'_, PiState>,
    runtime_id: String,
    connection_string: String,
    working_directory: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    let connection = SshConnection::parse(&connection_string)?;
    let remote_command = remote_pi_command(&working_directory, session_path.as_deref());
    spawn_command(app, state, runtime_id, connection.command(&remote_command))
}

#[tauri::command]
pub fn start_pi_sdk(
    app: AppHandle,
    state: State<'_, PiState>,
    runtime_id: String,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_runtime_id(&runtime_id)?;
    validate_start_paths(&project_path, session_path.as_deref())?;
    let pi_path = resolve_pi_binary()
        .ok_or_else(|| "Could not find pi. Install it or set TAU_PI_PATH.".to_string())?;
    let sdk_entry = resolve_pi_sdk_entry(&pi_path).ok_or_else(|| {
        "The installed Pi executable does not expose its Node SDK. Use RPC or install Pi with npm."
            .to_string()
    })?;
    let node_path = resolve_node_binary()
        .ok_or_else(|| "Could not find Node.js. Install it or set TAU_NODE_PATH.".to_string())?;
    let sidecar_path = materialize_sdk_sidecar()?;

    let mut command = Command::new(&node_path);
    configure_child_path(&mut command, &[pi_path.as_path(), node_path.as_path()])?;
    command
        .arg(sidecar_path)
        .args(["--sdk-entry", sdk_entry.to_string_lossy().as_ref()])
        .args(["--cwd", &project_path])
        .current_dir(&project_path);
    if let Some(path) = session_path {
        command.args(["--session", &path]);
    }
    spawn_command(app, state, runtime_id, command)
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
    let path = build_child_path(executables, current_path.as_deref())?;
    command.env("PATH", path);
    Ok(())
}

fn build_child_path(
    executables: &[&Path],
    current_path: Option<&OsStr>,
) -> Result<OsString, String> {
    let mut directories = Vec::new();
    for executable in executables {
        if let Some(parent) = executable
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            let parent = parent.to_path_buf();
            if !directories.contains(&parent) {
                directories.push(parent);
            }
        }
    }
    if let Some(path) = current_path {
        for directory in env::split_paths(path) {
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
    }
    env::join_paths(directories)
        .map_err(|error| format!("Could not prepare the Pi process PATH: {error}"))
}

fn materialize_sdk_sidecar() -> Result<PathBuf, String> {
    const SOURCE: &[u8] = include_bytes!("../../sidecar/pi-sdk.mjs");
    let directory = dirs::cache_dir()
        .ok_or_else(|| "Could not locate the cache folder.".to_string())?
        .join("tau");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create the Tau sidecar cache: {error}"))?;
    let path = directory.join("pi-sdk-sidecar.mjs");
    if fs::read(&path).ok().as_deref() != Some(SOURCE) {
        fs::write(&path, SOURCE)
            .map_err(|error| format!("Could not prepare the Pi SDK sidecar: {error}"))?;
    }
    Ok(path)
}

fn spawn_command(
    app: AppHandle,
    state: State<'_, PiState>,
    runtime_id: String,
    mut command: Command,
) -> Result<u64, String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    stop_runtime_process(&mut manager, &runtime_id);
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
    let _ = app.emit(
        "pi-event",
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
pub fn stop_pi(state: State<'_, PiState>, runtime_id: String) -> Result<(), String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    stop_runtime_process(&mut manager, &runtime_id);
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
                    if bytes.is_empty() || bytes.len() > MAX_RPC_LINE_BYTES {
                        continue;
                    }
                    let line = String::from_utf8_lossy(&bytes);
                    let _ = app.emit(
                        "pi-event",
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
                    let _ = app.emit(
                        "pi-event",
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
        let _ = app.emit(
            "pi-event",
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
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if line.is_empty() {
                continue;
            }
            let line = truncate_stderr_line(&line);
            if let Ok(mut tail) = stderr_tail.lock() {
                if tail.len() == STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line.clone());
            }
            let _ = app.emit(
                "pi-event",
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
    fn resolves_sdk_entry_next_to_the_pi_cli() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let dist = directory.path().join("dist");
        std::fs::create_dir(&dist).expect("dist directory");
        let cli = dist.join("cli.js");
        let index = dist.join("index.js");
        std::fs::write(&cli, "").expect("cli entry");
        std::fs::write(&index, "").expect("sdk entry");
        assert_eq!(
            resolve_pi_sdk_entry(&cli),
            Some(index.canonicalize().expect("canonical SDK entry")),
        );
    }

    #[test]
    fn child_path_prepends_and_deduplicates_executable_directories() {
        let path = build_child_path(
            &[
                Path::new("/opt/homebrew/bin/pi"),
                Path::new("/opt/homebrew/bin/node"),
            ],
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
    fn exit_message_includes_status_and_stderr() {
        assert_eq!(
            pi_exit_message(Some(127), "env: node: No such file or directory"),
            "Pi exited with status 127. env: node: No such file or directory",
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
