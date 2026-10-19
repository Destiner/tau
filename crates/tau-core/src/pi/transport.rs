use super::{PiDiagnostic, PiSink};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    process::ChildStdin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

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
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiEvent {
    pub runtime_id: String,
    pub generation: u64,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}
pub(super) fn emit_pi_event(sink: &PiSink, event: PiEvent) {
    sink.emit(event);
}
pub(super) fn emit_current_event(usable: &AtomicBool, sink: &PiSink, event: PiEvent) {
    // Input closure does not revoke output. The stdout producer receipt, not
    // this admission check, establishes completion of ordered sink enqueues.
    if usable.load(Ordering::Acquire) {
        sink.emit(event);
    }
}
#[cfg(unix)]
pub(super) fn nonblocking_pipe(pipe: &impl std::os::fd::AsRawFd) -> std::io::Result<()> {
    let fd = pipe.as_raw_fd();
    // The owned child pipe remains alive throughout both fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub(super) struct InterruptiblePipe<R> {
    pub(super) pipe: R,
    pub(super) drain: Arc<super::process::ReaderDrain>,
    pub(super) current: Arc<AtomicBool>,
}
#[cfg(unix)]
impl<R: std::io::Read + std::os::fd::AsRawFd> std::io::Read for InterruptiblePipe<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.drain.expired() || !self.current.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Pi output drain ended.",
                ));
            }
            match self.pipe.read(buffer) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let mut fd = libc::pollfd {
                        fd: self.pipe.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // Poll only within the existing process-check interval;
                    // inherited pipes cannot keep the reader alive past Stop.
                    if unsafe { libc::poll(&mut fd, 1, 10) } < 0 {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
                }
                result => return result,
            }
        }
    }
}
#[cfg(windows)]
impl<R: std::io::Read + std::os::windows::io::AsRawHandle> std::io::Read for InterruptiblePipe<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        #[link(name = "kernel32")]
        extern "system" {
            fn PeekNamedPipe(
                handle: *mut std::ffi::c_void,
                buffer: *mut std::ffi::c_void,
                size: u32,
                read: *mut u32,
                available: *mut u32,
                remaining: *mut u32,
            ) -> i32;
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            if self.drain.expired() || !self.current.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Pi output drain ended.",
                ));
            }
            let mut available = 0;
            // The owned anonymous child pipe is valid for this non-consuming
            // probe; ReadFile is called only for bytes already available.
            if unsafe {
                PeekNamedPipe(
                    self.pipe.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            } == 0
            {
                let error = std::io::Error::last_os_error();
                return if error.kind() == std::io::ErrorKind::BrokenPipe {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
            if available > 0 {
                let count = buffer.len().min(available as usize);
                return self.pipe.read(&mut buffer[..count]);
            }
            self.drain.poll_pause();
        }
    }
}
#[cfg(not(any(unix, windows)))]
impl<R: std::io::Read> std::io::Read for InterruptiblePipe<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.pipe.read(buffer)
    }
}

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
/// Drain the entire line but retain only a bounded prefix, even without a newline.
pub(super) fn read_bounded_line(
    reader: &mut impl BufRead,
    bytes: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<(bool, bool)> {
    bytes.clear();
    let mut any = false;
    let mut overflow = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok((any, overflow));
        }
        any = true;
        let end = buffer.iter().position(|b| *b == b'\n').map(|i| i + 1);
        let count = end.unwrap_or(buffer.len());
        let remaining = limit.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        overflow |= count > remaining;
        reader.consume(count);
        if end.is_some() {
            return Ok((true, overflow));
        }
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
pub(super) fn spawn_stderr_reader(
    sink: PiSink,
    usable: Arc<AtomicBool>,
    runtime_id: String,
    generation: u64,
    stderr: impl std::io::Read + Send + 'static,
    stderr_tail: StderrTail,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut bytes = Vec::new();
        loop {
            match read_bounded_line(&mut reader, &mut bytes, STDERR_LINE_CHARS * 4) {
                Ok((false, _)) => break,
                Ok((true, overflow)) => {
                    let line = String::from_utf8_lossy(&bytes);
                    let line = line.trim_end_matches(['\n', '\r']);
                    if line.is_empty() {
                        continue;
                    }
                    let mut line = truncate_stderr_line(line);
                    if overflow && !line.ends_with('…') {
                        line.push('…');
                    }
                    if let Ok(mut tail) = stderr_tail.lock() {
                        if tail.len() == STDERR_TAIL_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line.clone());
                    }
                    emit_current_event(
                        &usable,
                        &sink,
                        PiEvent {
                            runtime_id: runtime_id.clone(),
                            generation,
                            kind: "stderr",
                            line: None,
                            message: Some(line),
                            code: None,
                        },
                    );
                }
                Err(error) => {
                    sink.record(
                        &runtime_id,
                        Some(generation),
                        PiDiagnostic::ReaderFailed {
                            category: io_error_kind_category(error.kind()),
                        },
                    );
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
