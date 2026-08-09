use serde::Serialize;
use serde_json::Value;
use std::{
    env,
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
    let project = PathBuf::from(&project_path);
    if !project.is_dir() {
        return Err("The project folder no longer exists.".into());
    }
    if let Some(path) = session_path.as_deref() {
        if !Path::new(path).is_file() {
            return Err("The selected session file no longer exists.".into());
        }
    }

    let pi_path = resolve_pi_binary()
        .ok_or_else(|| "Could not find pi. Install it or set TAU_PI_PATH.".to_string())?;
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    stop_process(&mut manager);
    manager.generation = manager.generation.wrapping_add(1).max(1);
    let generation = manager.generation;

    let mut command = Command::new(pi_path);
    command
        .args(["--mode", "rpc"])
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = session_path {
        command.args(["--session", &path]);
    }

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
}
