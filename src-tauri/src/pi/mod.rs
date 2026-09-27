mod environment;
mod ownership;
mod process;
mod transport;

use crate::ssh::{remote_pi_command, SshConnection};
use crate::telemetry::{trace_context::TraceContext, Telemetry};
use environment::{
    configure_child_path, configure_session_arguments, resolve_node_binary, validate_runtime_id,
    validate_start_paths,
};
use opentelemetry::Value as TelemetryValue;
use ownership::send_pi_to_state;
use ownership::{
    claim_pi_frontend_inner, spawn_command, stop_runtime_process, validate_owner_id, PiOwnership,
    StoppedRuntime,
};
use serde_json::Value;
#[cfg(not(dev))]
use std::path::PathBuf;
use std::{
    process::Command,
    sync::{atomic::Ordering, Arc},
};
use tauri::{AppHandle, Runtime, State};
use transport::{emit_pi_event, PiEvent};

pub(crate) use environment::login_shell_pi_agent_dir;
pub use environment::resolve_pi_binary;
pub use ownership::PiState;

#[cfg(test)]
use {environment::*, ownership::*, process::*, transport::*};
#[cfg(test)]
mod tests;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipCommandError {
    kind: &'static str,
    message: String,
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
