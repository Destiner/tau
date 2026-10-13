mod environment;
mod ownership;
mod process;
mod transport;

pub use environment::{login_shell_pi_agent_dir, resolve_pi_binary};
pub use ownership::{ClaimFailure, ClaimOutcome, PiState, StoppedRuntime};
use serde_json::Value;
use std::{process::Command, sync::Arc};
pub use transport::PiEvent;

pub type StopOutcome = (String, Result<Option<u64>, String>);

/// `emit` returns only after accepting the event into the consumer's ordered
/// queue (or processing it). Deferred enqueueing cannot establish a Stop drain.
/// Bounded consumers must keep draining while a worker awaits Stop.
pub trait PiEventSink: Send + Sync + 'static {
    fn emit(&self, event: PiEvent);
}
impl<F: Fn(PiEvent) + Send + Sync + 'static> PiEventSink for F {
    fn emit(&self, event: PiEvent) {
        self(event);
    }
}

/// Content-free diagnostic categories. No paths, payloads, stderr or raw errors.
#[derive(Clone, Debug)]
pub enum PiDiagnostic {
    Resolved { found: bool },
    Started,
    Stopped { reason: &'static str },
    Exited { code: Option<i32> },
    LineDropped { reason: &'static str },
    ReaderFailed { category: &'static str },
}
pub trait PiDiagnosticSink: Send + Sync + 'static {
    fn record(&self, runtime_id: &str, generation: Option<u64>, diagnostic: PiDiagnostic);
}
impl<F: Fn(&str, Option<u64>, PiDiagnostic) + Send + Sync + 'static> PiDiagnosticSink for F {
    fn record(&self, runtime: &str, generation: Option<u64>, diagnostic: PiDiagnostic) {
        self(runtime, generation, diagnostic);
    }
}
#[derive(Clone)]
pub struct PiSink {
    events: Arc<dyn PiEventSink>,
    diagnostics: Option<Arc<dyn PiDiagnosticSink>>,
}
impl PiSink {
    pub fn new(events: Arc<dyn PiEventSink>) -> Self {
        Self {
            events,
            diagnostics: None,
        }
    }
    pub fn with_diagnostics(mut self, diagnostics: Arc<dyn PiDiagnosticSink>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
    fn emit(&self, event: PiEvent) {
        self.events.emit(event);
    }
    fn record(&self, runtime: &str, generation: Option<u64>, diagnostic: PiDiagnostic) {
        if let Some(sink) = &self.diagnostics {
            sink.record(runtime, generation, diagnostic);
        }
    }
}

/// A claim-only handle for running ownership cleanup on a worker without transferring lifecycle ownership.
pub struct PiClaimHandle {
    inner: Arc<std::sync::Mutex<ownership::PiManager>>,
}
impl PiClaimHandle {
    pub fn claim(&self, owner: String, revision: u64) -> Result<ClaimOutcome, ClaimFailure> {
        ownership::validate_owner_id(&owner).map_err(|message| ClaimFailure {
            kind: "retryable",
            message,
            stopped: Vec::new(),
        })?;
        ownership::claim_pi_frontend_inner(&self.inner, owner, revision)
    }
}
impl PiState {
    pub fn clone_handle(&self) -> PiClaimHandle {
        PiClaimHandle {
            inner: Arc::clone(&self.inner),
        }
    }
    pub fn read_revision(&self) -> Result<u64, String> {
        let manager = self
            .inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        if matches!(manager.ownership, ownership::PiOwnership::ShuttingDown) {
            return Err("Tau is shutting down.".into());
        }
        Ok(manager.ownership_revision)
    }
    pub fn claim(&self, owner: String, revision: u64) -> Result<ClaimOutcome, ClaimFailure> {
        ownership::validate_owner_id(&owner).map_err(|message| ClaimFailure {
            kind: "retryable",
            message,
            stopped: Vec::new(),
        })?;
        ownership::claim_pi_frontend_inner(&self.inner, owner, revision)
    }
    pub fn start_local(
        &self,
        owner: String,
        runtime: String,
        project: String,
        session: Option<String>,
        sink: PiSink,
    ) -> Result<u64, String> {
        environment::validate_runtime_id(&runtime)?;
        environment::validate_start_paths(&project, session.as_deref())?;
        let pi = resolve_pi_binary();
        sink.record(
            &runtime,
            None,
            PiDiagnostic::Resolved {
                found: pi.is_some(),
            },
        );
        let pi =
            pi.ok_or_else(|| "Could not find pi. Install it or set TAU_PI_PATH.".to_string())?;
        let node = environment::resolve_node_binary();
        let mut paths = vec![pi.as_path()];
        if let Some(node) = node.as_deref() {
            paths.push(node);
        }
        let mut command = Command::new(&pi);
        environment::configure_child_path(&mut command, &paths)?;
        command.args(["--mode", "rpc"]).current_dir(&project);
        let profile = crate::profile::current()?;
        let session_dir = (profile.session_registry_filename()
            == crate::profile::ISOLATED_SESSION_REGISTRY_FILENAME)
            .then(|| crate::storage::default_session_dir(&project))
            .transpose()?;
        environment::configure_session_arguments(
            &mut command,
            session_dir.as_deref(),
            session.as_deref(),
        );
        self.start_command(owner, runtime, command, sink)
    }
    pub fn start_remote(
        &self,
        owner: String,
        runtime: String,
        connection: String,
        working_directory: String,
        session: Option<String>,
        sink: PiSink,
    ) -> Result<u64, String> {
        environment::validate_runtime_id(&runtime)?;
        let connection = crate::ssh::SshConnection::parse(&connection)?;
        let remote = crate::ssh::remote_pi_command(&working_directory, session.as_deref());
        ownership::spawn_command(
            self,
            sink,
            owner,
            runtime,
            connection.pi_command(&remote),
            true,
        )
    }
    /// Explicit command injection for deterministic subprocess tests and embedders.
    pub fn start_command(
        &self,
        owner: String,
        runtime: String,
        command: Command,
        sink: PiSink,
    ) -> Result<u64, String> {
        environment::validate_runtime_id(&runtime)?;
        ownership::spawn_command(self, sink, owner, runtime, command, false)
    }
    pub fn send(&self, owner: &str, runtime: String, request: Value) -> Result<(), String> {
        ownership::send_pi_with_input_event(self, owner, runtime, request)
    }
    /// One cleanup budget for all selected runtimes; failures retain ownership.
    /// The outer error means no batch was admitted, not a successful stop receipt.
    pub fn stop_many(&self, owner: &str, runtimes: &[String]) -> Result<Vec<StopOutcome>, String> {
        let mut manager = self
            .inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        manager.require_owner(owner)?;
        let _cleanup = ownership::CleanupGuard::reserve(&self.inner, &mut manager)?;
        let selected = runtimes
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        for runtime in &selected {
            manager.fence_stop(Some(runtime));
        }
        drop(manager);
        let outcomes = ownership::stop_selected_processes(&self.inner, Some(&selected))?;
        let mut outcomes = outcomes
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>();
        Ok(selected
            .into_iter()
            .map(|runtime| {
                let result = outcomes.remove(&runtime).unwrap_or(Ok(None));
                (runtime, result)
            })
            .collect())
    }
    pub fn stop(&self, owner: &str, runtime: &str) -> Result<Option<u64>, String> {
        let mut manager = self
            .inner
            .lock()
            .map_err(|_| "Pi process state is unavailable.".to_string())?;
        manager.require_owner(owner)?;
        let _cleanup = ownership::CleanupGuard::reserve(&self.inner, &mut manager)?;
        let process = manager.processes.get(runtime).cloned();
        manager.fence_stop(Some(runtime));
        drop(manager);
        ownership::stop_reserved_processes(&self.inner, Some(runtime))
            .map_err(|(error, _)| error)?;
        if let Some(process) = process {
            process.sink.record(
                runtime,
                Some(process.generation),
                PiDiagnostic::Stopped {
                    reason: "explicit_stop",
                },
            );
            Ok(Some(process.generation))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
use {environment::*, ownership::*, process::*, transport::*};
#[cfg(test)]
mod tests;
