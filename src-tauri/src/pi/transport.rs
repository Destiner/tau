use crate::telemetry::Telemetry;
use opentelemetry::Value as TelemetryValue;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::ChildStdin,
    sync::{mpsc, Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub(super) const MAX_RPC_LINE_BYTES: usize = 64 * 1024 * 1024;
pub(super) const STDERR_TAIL_LINES: usize = 8;
const STDERR_LINE_CHARS: usize = 2048;
const PI_WRITER_QUEUE_CAPACITY: usize = 32;

pub(super) type StderrTail = Arc<Mutex<VecDeque<String>>>;
pub(super) type PiWriter = Arc<Mutex<Option<mpsc::SyncSender<PiWriteRequest>>>>;
pub(super) struct PiWriteRequest {
    pub(super) line: Vec<u8>,
    pub(super) result: mpsc::SyncSender<std::io::Result<()>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PiEvent<'a> {
    pub(super) runtime_id: &'a str,
    pub(super) generation: u64,
    pub(super) kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) line: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) code: Option<i32>,
}
/// Emits a `pi-event`, recording (and force-flushing) a `pi.reader`
/// telemetry log if the emission itself fails — e.g. the webview is gone.
/// `app.emit`'s own error is never recorded: only the bounded, reviewed
/// event kind that failed to reach the frontend.
pub(super) fn emit_pi_event<R: Runtime>(app: &AppHandle<R>, event: PiEvent<'_>) {
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
pub(super) fn io_error_kind_category(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::BrokenPipe => "broken_pipe",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        _ => "other",
    }
}

pub(super) fn line_drop_reason(bytes: &[u8]) -> Option<&'static str> {
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
pub(super) fn spawn_stdin_writer(mut stdin: ChildStdin) -> Result<PiWriter, String> {
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
pub(super) fn spawn_stderr_reader<R: Runtime>(
    app: AppHandle<R>,
    runtime_id: String,
    generation: u64,
    stderr: impl std::io::Read + Send + 'static,
    stderr_tail: StderrTail,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
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

pub(super) fn pi_exit_message(code: Option<i32>, stderr: &str) -> String {
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
