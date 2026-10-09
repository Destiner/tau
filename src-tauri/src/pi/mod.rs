use crate::telemetry::{trace_context::TraceContext, Telemetry};
use opentelemetry::{trace::SpanContext, Value as TelemetryValue};
use serde_json::Value;
use std::sync::Arc;
use tau_core::pi::{PiDiagnostic, PiEvent, PiSink, StoppedRuntime};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub use tau_core::pi::PiState;

#[cfg(test)]
mod tests;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipCommandError {
    kind: &'static str,
    message: String,
}

fn event_sink<R: Runtime>(app: AppHandle<R>, span: Option<SpanContext>) -> PiSink {
    let diagnostic_app = app.clone();
    PiSink::new(Arc::new(move |event: PiEvent| {
        let runtime = event.runtime_id.clone();
        let generation = event.generation;
        let kind = event.kind;
        if app.emit("pi-event", event).is_err() {
            let telemetry = app.state::<Telemetry>();
            telemetry.record_reader_event(
                "pi.event.emit_failed",
                &runtime,
                Some(generation),
                &[("tau.event.kind", TelemetryValue::String(kind.into()))],
            );
            telemetry.force_flush_logs();
        }
    }))
    .with_diagnostics(Arc::new(
        move |runtime: &str, generation: Option<u64>, diagnostic: PiDiagnostic| {
            let telemetry = diagnostic_app.state::<Telemetry>();
            match diagnostic {
                PiDiagnostic::Resolved { found } => telemetry.record_process_lifecycle(
                    "pi.process.resolved",
                    runtime,
                    generation,
                    span.as_ref(),
                    &[(
                        "tau.process.resolution",
                        TelemetryValue::String(if found { "found" } else { "not_found" }.into()),
                    )],
                ),
                PiDiagnostic::Started => {
                    telemetry.record_process_lifecycle(
                        "pi.process.started",
                        runtime,
                        generation,
                        span.as_ref(),
                        &[],
                    );
                    telemetry.record_pi_process_start();
                }
                PiDiagnostic::Stopped {
                    reason: "explicit_stop",
                } => {}
                PiDiagnostic::Stopped { reason } => {
                    telemetry.record_process_lifecycle(
                        "pi.process.stopped",
                        runtime,
                        generation,
                        span.as_ref(),
                        &[(
                            "tau.process.stop_reason",
                            TelemetryValue::String(reason.into()),
                        )],
                    );
                    telemetry.record_pi_process_exit(true);
                }
                PiDiagnostic::Exited { code } => {
                    let attributes = code
                        .map(|code| {
                            vec![("tau.process.exit_code", TelemetryValue::I64(code as i64))]
                        })
                        .unwrap_or_default();
                    telemetry.record_process_lifecycle(
                        "pi.process.exited",
                        runtime,
                        generation,
                        None,
                        &attributes,
                    );
                    telemetry.record_pi_process_exit(false);
                    telemetry.force_flush_logs();
                }
                PiDiagnostic::LineDropped { reason } => telemetry.record_reader_event(
                    "pi.reader.line_dropped",
                    runtime,
                    generation,
                    &[(
                        "tau.reader.drop_reason",
                        TelemetryValue::String(reason.into()),
                    )],
                ),
                PiDiagnostic::ReaderFailed { category } => {
                    telemetry.record_reader_event(
                        "pi.reader.failed",
                        runtime,
                        generation,
                        &[(
                            "tau.reader.error_kind",
                            TelemetryValue::String(category.into()),
                        )],
                    );
                    telemetry.force_flush_logs();
                }
            }
        },
    ))
}

#[tauri::command]
pub fn read_pi_frontend_revision(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
) -> Result<u64, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "read_pi_frontend_revision"));
    state.read_revision()
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
    // State is application-owned; run bounded child cleanup away from the UI thread.
    let state = state.inner().clone_handle();
    let result =
        tauri::async_runtime::spawn_blocking(move || state.claim(owner_id, expected_revision))
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
    span: Option<&crate::telemetry::CommandSpan>,
    stopped: &[StoppedRuntime],
) {
    let context = span.map(|span| span.span_context());
    for runtime in stopped {
        telemetry.record_process_lifecycle(
            "pi.process.stopped",
            &runtime.runtime_id,
            Some(runtime.generation),
            context.as_ref(),
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
    let span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "start_pi"));
    state.start_local(
        owner_id,
        runtime_id,
        project_path,
        session_path,
        event_sink(app, span.as_ref().map(|span| span.span_context())),
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
    let span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "start_pi_remote"));
    state.start_remote(
        owner_id,
        runtime_id,
        connection_string,
        working_directory,
        session_path,
        event_sink(app, span.as_ref().map(|span| span.span_context())),
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
    let _ = app;
    state.send(&owner_id, runtime_id, request)
}
#[tauri::command]
pub fn stop_pi(
    state: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    runtime_id: String,
) -> Result<(), String> {
    let span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "stop_pi"));
    if let Some(generation) = state.stop(&owner_id, &runtime_id)? {
        telemetry.record_process_lifecycle(
            "pi.process.stopped",
            &runtime_id,
            Some(generation),
            span.as_ref().map(|span| span.span_context()).as_ref(),
            &[(
                "tau.process.stop_reason",
                TelemetryValue::String("explicit_stop".into()),
            )],
        );
        telemetry.record_pi_process_exit(true);
    }
    Ok(())
}
