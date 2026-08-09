use serde::Serialize;
use serde_json::Value;
use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Emitter, State};

const MAX_RPC_LINE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub struct PiState {
    inner: Arc<Mutex<PiManager>>,
}

impl Drop for PiState {
    fn drop(&mut self) {
        if let Ok(mut manager) = self.inner.lock() {
            stop_process(&mut manager);
        }
    }
}

#[derive(Default)]
struct PiManager {
    generation: u64,
    process: Option<PiProcess>,
}

struct PiProcess {
    generation: u64,
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiEvent<'a> {
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
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
    validate_start_paths(&project_path, session_path.as_deref())?;

    let pi_path = resolve_pi_binary()
        .ok_or_else(|| "Could not find pi. Install it or set TAU_PI_PATH.".to_string())?;
    let mut command = Command::new(pi_path);
    command.args(["--mode", "rpc"]).current_dir(&project_path);
    if let Some(path) = session_path {
        command.args(["--session", &path]);
    }
    spawn_command(app, state, command)
}

#[tauri::command]
pub fn start_pi_sdk(
    app: AppHandle,
    state: State<'_, PiState>,
    project_path: String,
    session_path: Option<String>,
) -> Result<u64, String> {
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

    let mut command = Command::new(node_path);
    command
        .arg(sidecar_path)
        .args(["--sdk-entry", sdk_entry.to_string_lossy().as_ref()])
        .args(["--cwd", &project_path])
        .current_dir(&project_path);
    if let Some(path) = session_path {
        command.args(["--session", &path]);
    }
    spawn_command(app, state, command)
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
    mut command: Command,
) -> Result<u64, String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    stop_process(&mut manager);
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

    spawn_stdout_reader(
        app.clone(),
        Arc::clone(&state.inner),
        Arc::clone(&child),
        generation,
        stdout,
    );
    spawn_stderr_reader(app.clone(), generation, stderr);

    manager.process = Some(PiProcess {
        generation,
        child,
        stdin: Arc::new(Mutex::new(stdin)),
    });
    let _ = app.emit(
        "pi-event",
        PiEvent {
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
pub fn send_pi(state: State<'_, PiState>, request: Value) -> Result<(), String> {
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
            .process
            .as_ref()
            .map(|process| Arc::clone(&process.stdin))
            .ok_or_else(|| "Pi is not running.".to_string())?
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
pub fn stop_pi(state: State<'_, PiState>) -> Result<(), String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    stop_process(&mut manager);
    Ok(())
}

fn stop_process(manager: &mut PiManager) {
    let Some(process) = manager.process.take() else {
        return;
    };
    if let Ok(mut child) = process.child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    };
}

fn spawn_stdout_reader(
    app: AppHandle,
    manager: Arc<Mutex<PiManager>>,
    child: Arc<Mutex<Child>>,
    generation: u64,
    stdout: impl std::io::Read + Send + 'static,
) {
    thread::spawn(move || {
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

        let code = child
            .lock()
            .ok()
            .and_then(|mut child| child.wait().ok())
            .and_then(|status| status.code());
        if let Ok(mut current) = manager.lock() {
            if current
                .process
                .as_ref()
                .is_some_and(|process| process.generation == generation)
            {
                current.process = None;
            }
        }
        let _ = app.emit(
            "pi-event",
            PiEvent {
                generation,
                kind: "exited",
                line: None,
                message: None,
                code,
            },
        );
    });
}

fn spawn_stderr_reader(
    app: AppHandle,
    generation: u64,
    stderr: impl std::io::Read + Send + 'static,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if line.is_empty() {
                continue;
            }
            let _ = app.emit(
                "pi-event",
                PiEvent {
                    generation,
                    kind: "stderr",
                    line: None,
                    message: Some(&line),
                    code: None,
                },
            );
        }
    });
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
}
