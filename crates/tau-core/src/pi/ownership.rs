use super::process::PI_WRITE_TIMEOUT;
use super::process::{
    send_pi_line, spawn_child, stop_child, stop_process, stop_processes, wait_for_child_until,
    PiProcess,
};
use super::transport::{
    emit_current_event, emit_pi_event, io_error_kind_category, line_drop_reason, pi_exit_message,
    read_bounded_line, spawn_stderr_reader, InterruptiblePipe, PiEvent, StderrTail,
    MAX_RPC_LINE_BYTES, STDERR_TAIL_LINES,
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
        if let Ok(mut manager) = self.inner.lock() {
            // This best-effort API cannot acknowledge failed remote cleanup.
            // Keep failed entries until the state itself is destroyed.
            let Ok(_cleanup) = CleanupGuard::reserve(&self.inner, &mut manager) else {
                return;
            };
            manager.ownership = PiOwnership::ShuttingDown;
            manager.fence_stop(None);
            drop(manager);
            let _ = stop_reserved_processes(&self.inner, None);
        }
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
    cleanup_in_progress: bool,
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
    pub(super) fn fence_stop(&self, runtime: Option<&str>) {
        for (id, process) in &self.processes {
            if runtime.is_none_or(|runtime| id == runtime) {
                process
                    .stopping
                    .store(true, std::sync::atomic::Ordering::Release);
                process.close_stdin();
            }
        }
    }

    pub(super) fn require_owner(&self, owner_id: &str) -> Result<(), String> {
        if self.cleanup_in_progress {
            return Err("Pi runtime cleanup is in progress. Try again.".into());
        }
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
pub(super) struct CleanupGuard {
    inner: Arc<Mutex<PiManager>>,
}
impl CleanupGuard {
    pub(super) fn reserve(
        inner: &Arc<Mutex<PiManager>>,
        manager: &mut PiManager,
    ) -> Result<Self, String> {
        if manager.cleanup_in_progress {
            return Err("Pi runtime cleanup is in progress. Try again.".into());
        }
        manager.cleanup_in_progress = true;
        Ok(Self {
            inner: Arc::clone(inner),
        })
    }
}
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Ok(mut manager) = self.inner.lock() {
            manager.cleanup_in_progress = false;
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
    pub(super) process: PiProcess,
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
    if manager.cleanup_in_progress {
        return Err(ClaimFailure {
            kind: "retryable",
            message: "Pi runtime cleanup is in progress. Try again.".into(),
            stopped: Vec::new(),
        });
    }
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

    let _cleanup = CleanupGuard::reserve(inner, &mut manager).map_err(|message| ClaimFailure {
        kind: "retryable",
        message,
        stopped: Vec::new(),
    })?;
    manager.fence_stop(None);
    drop(manager);
    let stopped =
        stop_reserved_processes(inner, None).map_err(|(message, stopped)| ClaimFailure {
            kind: "retryable",
            message,
            stopped,
        })?;
    let mut manager = inner.lock().map_err(|_| ClaimFailure {
        kind: "retryable",
        message: "Pi process state is unavailable.".into(),
        stopped: Vec::new(),
    })?;
    manager.ownership = PiOwnership::Active { owner_id };
    drop(manager);
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
    remote: bool,
) -> Result<u64, String> {
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    manager.require_owner(&owner_id)?;
    let _cleanup = CleanupGuard::reserve(&state.inner, &mut manager)?;
    let replaced_generation = manager.processes.get(&runtime_id).map(|p| p.generation);
    manager.fence_stop(Some(&runtime_id));
    drop(manager);
    stop_reserved_processes(&state.inner, Some(&runtime_id)).map_err(|(error, _)| error)?;
    let mut manager = state
        .inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    manager.generation = manager.generation.wrapping_add(1).max(1);
    let generation = manager.generation;

    let (child, writer, stdout, stderr) = match spawn_child(&mut command) {
        Ok(child) => child,
        Err((error, spawned_child)) => {
            let error = if let Some(child) = spawned_child {
                if remote {
                    // A post-spawn setup failure still owns a potentially live remote.
                    let process = PiProcess {
                        remote: true,
                        remote_uncertain: Arc::new(AtomicBool::new(true)),
                        kill_attempted: Arc::new(AtomicBool::new(false)),
                        owner_id,
                        generation,
                        sink: sink.clone(),
                        child,
                        writer: Arc::new(Mutex::new(None)),
                        usable: Arc::new(AtomicBool::new(false)),
                        output_current: Arc::new(AtomicBool::new(false)),
                        stopping: Arc::new(AtomicBool::new(true)),
                        drain: {
                            let drain = Arc::new(super::process::ReaderDrain::default());
                            drain.complete(Err("Pi output stream setup failed.".into()));
                            drain
                        },
                    };
                    let _ = stop_process(&process);
                    manager.processes.insert(runtime_id.clone(), process);
                    super::process::REMOTE_EXIT_UNCONFIRMED.to_string()
                } else {
                    let _ = stop_child(&child);
                    error
                }
            } else {
                error
            };
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
            remote,
            remote_uncertain: Arc::new(AtomicBool::new(false)),
            kill_attempted: Arc::new(AtomicBool::new(false)),
            owner_id,
            generation,
            sink: sink.clone(),
            child: Arc::clone(&child),
            writer,
            usable: Arc::clone(&usable),
            output_current: Arc::clone(&output_current),
            stopping: Arc::new(AtomicBool::new(false)),
            drain: Arc::new(super::process::ReaderDrain::default()),
        },
    );

    let process = manager.processes[&runtime_id].clone();
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
    let stdout = InterruptiblePipe {
        pipe: stdout,
        drain: Arc::clone(&process.drain),
        current: Arc::clone(&process.output_current),
    };
    let stderr = InterruptiblePipe {
        pipe: stderr,
        drain: Arc::clone(&process.drain),
        current: Arc::clone(&process.output_current),
    };
    let reader_context = PiReaderContext {
        sink: sink.clone(),
        manager: Arc::clone(&state.inner),
        child,
        usable,
        output_current: Arc::clone(&output_current),
        runtime_id: runtime_id.clone(),
        generation,
        process,
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

pub(super) fn stop_reserved_processes(
    inner: &Arc<Mutex<PiManager>>,
    runtime: Option<&str>,
) -> Result<Vec<StoppedRuntime>, (String, Vec<StoppedRuntime>)> {
    let selected = runtime.map(|id| std::collections::HashSet::from([id.to_owned()]));
    let outcomes =
        stop_selected_processes(inner, selected.as_ref()).map_err(|error| (error, Vec::new()))?;
    let mut stopped = Vec::new();
    let mut first_error = None;
    for (runtime_id, outcome) in outcomes {
        match outcome {
            Ok(Some(generation)) => stopped.push(StoppedRuntime {
                runtime_id,
                generation,
            }),
            Ok(None) => {}
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

pub(super) fn stop_selected_processes(
    inner: &Arc<Mutex<PiManager>>,
    selected: Option<&std::collections::HashSet<String>>,
) -> Result<Vec<super::StopOutcome>, String> {
    let runtimes = {
        let manager = inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        manager
            .processes
            .iter()
            .filter(|(id, _)| selected.is_none_or(|ids| ids.contains(*id)))
            .map(|(id, process)| {
                // Keep the original entry visible until both physical cleanup
                // and its producer receipt succeed.
                process
                    .stopping
                    .store(true, std::sync::atomic::Ordering::Release);
                process.close_stdin();
                (id.clone(), process.clone())
            })
            .collect::<Vec<_>>()
    };
    let processes = runtimes
        .iter()
        .map(|(_, process)| process)
        .collect::<Vec<_>>();
    let results = stop_processes(&processes);
    let mut manager = inner
        .lock()
        .map_err(|_| "Pi process state is unavailable.".to_string())?;
    let mut outcomes = Vec::new();
    for ((runtime_id, process), result) in runtimes.into_iter().zip(results) {
        match result.result(&process) {
            Ok(()) => {
                if manager
                    .processes
                    .get(&runtime_id)
                    .is_some_and(|p| p.generation == process.generation)
                {
                    manager.processes.remove(&runtime_id);
                }
                outcomes.push((runtime_id, Ok(Some(process.generation))));
            }
            Err(error) => {
                outcomes.push((runtime_id, Err(error)));
            }
        }
    }
    Ok(outcomes)
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
            process,
        } = context;
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        let mut drain_result = Ok(());
        loop {
            bytes.clear();
            match read_bounded_line(&mut reader, &mut bytes, MAX_RPC_LINE_BYTES + 2) {
                Ok((false, _)) => break,
                Ok((true, overflow)) => {
                    if overflow {
                        drain_result = Err("Pi output contained an oversized record.".into());
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
                        drain_result = Err("Pi output contained an invalid record.".into());
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
                    drain_result = Err("Pi output reader failed.".into());
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

        let status = wait_for_child_until(&child, Some(&process.drain));
        usable.store(false, std::sync::atomic::Ordering::Release);
        output_current.store(false, std::sync::atomic::Ordering::Release);
        // Descendants may inherit stderr after Pi exits. The tail is
        // best-effort, so they must not delay process cleanup or failure UI.
        drop(stderr_reader);
        let code = status.as_ref().and_then(|status| status.code());
        let succeeded = status.as_ref().is_some_and(|status| status.success());
        let confirmed = process.confirms_exit(status);
        process.close_stdin();
        let stderr = stderr_tail
            .lock()
            .map(|tail| tail.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        let remote_uncertain = process
            .remote_uncertain
            .load(std::sync::atomic::Ordering::Acquire);
        let message = if remote_uncertain {
            Some(super::process::REMOTE_EXIT_UNCONFIRMED.into())
        } else {
            (!succeeded).then(|| pi_exit_message(code, &stderr))
        };
        // Natural cleanup publishes its completed stdout receipt before making
        // the entry absent. A reserved Stop must keep the entry pinned and waits
        // for the Exit enqueue as well.
        if confirmed && drain_result.is_ok() {
            if let Ok(mut current) = manager.lock() {
                if !process.stopping.load(std::sync::atomic::Ordering::Acquire)
                    && current
                        .processes
                        .get(&runtime_id)
                        .is_some_and(|p| p.generation == generation)
                {
                    process.drain.complete(drain_result.clone());
                    current.processes.remove(&runtime_id);
                }
            }
        }
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
        process.drain.complete(drain_result);
    });
}

#[cfg(test)]
pub(super) fn send_pi_to_state(
    state: &PiState,
    owner_id: &str,
    runtime_id: String,
    request: Value,
) -> Result<(), String> {
    send_pi_request(state, owner_id, runtime_id, request, false)
}

pub(super) fn send_pi_with_input_event(
    state: &PiState,
    owner_id: &str,
    runtime_id: String,
    request: Value,
) -> Result<(), String> {
    send_pi_request(state, owner_id, runtime_id, request, true)
}

fn send_pi_request(
    state: &PiState,
    owner_id: &str,
    runtime_id: String,
    request: Value,
    notify_input: bool,
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
    let result = send_pi_line(&process, line, PI_WRITE_TIMEOUT);
    if notify_input
        && result.is_err()
        && !process.usable.load(std::sync::atomic::Ordering::Acquire)
        && !process.stopping.load(std::sync::atomic::Ordering::Acquire)
        && process
            .output_current
            .load(std::sync::atomic::Ordering::Acquire)
    {
        // Report the captured writer's generation, never a later manager entry.
        process.sink.emit(PiEvent {
            runtime_id,
            generation: process.generation,
            kind: "input_closed",
            line: None,
            message: None,
            code: None,
        });
    }
    result
}
