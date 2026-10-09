use super::process::PI_WRITE_TIMEOUT;
use super::process::{
    send_pi_line, spawn_child, stop_all_processes, stop_process, stop_processes, wait_for_child,
    PiProcess,
};
use super::transport::{
    emit_current_event, emit_pi_event, io_error_kind_category, line_drop_reason, pi_exit_message,
    read_bounded_line, spawn_stderr_reader, PiEvent, StderrTail, MAX_RPC_LINE_BYTES,
    STDERR_TAIL_LINES,
};
use super::{PiDiagnostic, PiSink};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    io::BufReader,
    process::{Child, Command},
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread,
};

#[derive(Default)]
pub struct PiState {
    pub(super) inner: Arc<Mutex<PiManager>>,
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
pub(super) struct PiManager {
    pub(super) ownership_revision: u64,
    pub(super) ownership: PiOwnership,
    pub(super) generation: u64,
    pub(super) processes: HashMap<String, PiProcess>,
}

#[derive(Default)]
pub(super) enum PiOwnership {
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
    pub(super) fn require_owner(&self, owner_id: &str) -> Result<(), String> {
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
pub(super) struct PiReaderContext {
    pub(super) sink: PiSink,
    pub(super) manager: Arc<Mutex<PiManager>>,
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) usable: Arc<AtomicBool>,
    pub(super) output_current: Arc<AtomicBool>,
    pub(super) runtime_id: String,
    pub(super) generation: u64,
}

#[derive(Debug)]
pub struct StoppedRuntime {
    pub runtime_id: String,
    pub generation: u64,
}

#[derive(Debug)]
pub struct ClaimOutcome {
    pub stopped: Vec<StoppedRuntime>,
    pub replacement: bool,
}

#[derive(Debug)]
pub struct ClaimFailure {
    pub kind: &'static str,
    pub message: String,
    pub stopped: Vec<StoppedRuntime>,
}
pub(super) fn claim_pi_frontend_inner(
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

pub(super) fn validate_owner_id(owner_id: &str) -> Result<(), String> {
    if owner_id.is_empty() || owner_id.len() > 128 {
        return Err("Tau supplied an invalid frontend owner id.".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_command(
    state: &PiState,
    sink: PiSink,
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
    manager.generation = manager.generation.wrapping_add(1).max(1);
    let generation = manager.generation;

    let (child, writer, stdout, stderr) = match spawn_child(&mut command) {
        Ok(child) => child,
        Err(error) => {
            drop(manager);
            if let Some(generation) = replaced_generation {
                sink.record(
                    &runtime_id,
                    Some(generation),
                    PiDiagnostic::Stopped { reason: "replaced" },
                );
            }
            return Err(error);
        }
    };
    let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));

    let usable = Arc::new(AtomicBool::new(true));
    // A failed stdin write must not discard buffered final stdout from this generation.
    let output_current = Arc::new(AtomicBool::new(true));
    manager.processes.insert(
        runtime_id.clone(),
        PiProcess {
            owner_id,
            generation,
            sink: sink.clone(),
            child: Arc::clone(&child),
            writer,
            usable: Arc::clone(&usable),
            output_current: Arc::clone(&output_current),
        },
    );

    drop(manager);
    if let Some(generation) = replaced_generation {
        sink.record(
            &runtime_id,
            Some(generation),
            PiDiagnostic::Stopped { reason: "replaced" },
        );
    }
    sink.record(&runtime_id, Some(generation), PiDiagnostic::Started);
    emit_current_event(
        &output_current,
        &sink,
        PiEvent {
            runtime_id: runtime_id.clone(),
            generation,
            kind: "started",
            line: None,
            message: None,
            code: None,
        },
    );
    let reader_context = PiReaderContext {
        sink: sink.clone(),
        manager: Arc::clone(&state.inner),
        child,
        usable,
        output_current: Arc::clone(&output_current),
        runtime_id: runtime_id.clone(),
        generation,
    };
    let stderr_reader = spawn_stderr_reader(
        sink.clone(),
        output_current,
        runtime_id.clone(),
        generation,
        stderr,
        Arc::clone(&stderr_tail),
    );
    spawn_stdout_reader(reader_context, stdout, stderr_tail, stderr_reader);
    Ok(generation)
}

pub(super) fn stop_runtime_process(
    manager: &mut PiManager,
    runtime_id: &str,
) -> Result<(), String> {
    let Some(process) = manager.processes.remove(runtime_id) else {
        return Ok(());
    };
    if let Err(error) = stop_process(&process) {
        manager.processes.insert(runtime_id.to_string(), process);
        return Err(error);
    }
    Ok(())
}

pub(super) fn stop_stale_processes(
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
pub(super) fn spawn_stdout_reader(
    context: PiReaderContext,
    stdout: impl std::io::Read + Send + 'static,
    stderr_tail: StderrTail,
    stderr_reader: thread::JoinHandle<()>,
) {
    thread::spawn(move || {
        let PiReaderContext {
            sink,
            manager,
            child,
            usable,
            output_current,
            runtime_id,
            generation,
        } = context;
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match read_bounded_line(&mut reader, &mut bytes, MAX_RPC_LINE_BYTES + 2) {
                Ok((false, _)) => break,
                Ok((true, overflow)) => {
                    if overflow {
                        sink.record(
                            &runtime_id,
                            Some(generation),
                            PiDiagnostic::LineDropped {
                                reason: "oversized",
                            },
                        );
                        continue;
                    }
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
                        sink.record(
                            &runtime_id,
                            Some(generation),
                            PiDiagnostic::LineDropped { reason },
                        );
                        continue;
                    }
                    let line = String::from_utf8_lossy(&bytes);
                    emit_current_event(
                        &output_current,
                        &sink,
                        PiEvent {
                            runtime_id: runtime_id.clone(),
                            generation,
                            kind: "rpc",
                            line: Some(line.into_owned()),
                            message: None,
                            code: None,
                        },
                    );
                }
                Err(error) => {
                    let message = error.to_string();
                    sink.record(
                        &runtime_id,
                        Some(generation),
                        PiDiagnostic::ReaderFailed {
                            category: io_error_kind_category(error.kind()),
                        },
                    );
                    emit_current_event(
                        &output_current,
                        &sink,
                        PiEvent {
                            runtime_id: runtime_id.clone(),
                            generation,
                            kind: "error",
                            line: None,
                            message: Some(message),
                            code: None,
                        },
                    );
                    break;
                }
            }
        }

        let status = wait_for_child(&child);
        usable.store(false, std::sync::atomic::Ordering::Release);
        output_current.store(false, std::sync::atomic::Ordering::Release);
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
        sink.record(&runtime_id, Some(generation), PiDiagnostic::Exited { code });
        emit_pi_event(
            &sink,
            PiEvent {
                runtime_id: runtime_id.clone(),
                generation,
                kind: "exited",
                line: None,
                message,
                code,
            },
        );
    });
}

pub(super) fn send_pi_to_state(
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
