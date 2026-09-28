use super::transport::{spawn_stdin_writer, PiWriteRequest, PiWriter};

pub(super) const PI_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const PI_STOP_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const PI_STOP_GRACE_PERIOD: Duration = Duration::from_secs(1);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
use std::{
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct PiProcess {
    pub(super) owner_id: String,
    pub(super) generation: u64,
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) writer: PiWriter,
    pub(super) usable: Arc<AtomicBool>,
}

impl PiProcess {
    pub(super) fn close_stdin(&self) {
        self.usable.store(false, Ordering::Release);
        // Closing the shared sender also fences retained process handles. The
        // writer owns stdin and releases it without holding a process lock.
        self.writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}
type SpawnedChild = (Arc<Mutex<Child>>, PiWriter, ChildStdout, ChildStderr);

pub(super) fn spawn_child(command: &mut Command) -> Result<SpawnedChild, String> {
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
    let writer = match spawn_stdin_writer(stdin) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = stop_child(&child);
            return Err(error);
        }
    };
    Ok((child, writer, stdout, stderr))
}

pub(super) fn send_pi_line(
    process: &PiProcess,
    line: Vec<u8>,
    timeout: Duration,
) -> Result<(), String> {
    if !process.usable.load(Ordering::Acquire) {
        return Err("Pi is no longer accepting requests. Restart Tau and try again.".into());
    }
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    process
        .writer
        .lock()
        .map_err(|_| {
            "The Pi input stream is unavailable. Reopen the session and try again.".to_string()
        })?
        .as_ref()
        .ok_or_else(|| {
            "Pi is no longer accepting requests. Restart Tau and try again.".to_string()
        })?
        .try_send(PiWriteRequest {
            line,
            result: result_sender,
        })
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => {
                "Pi is still accepting another request. Try again.".to_string()
            }
            mpsc::TrySendError::Disconnected(_) => {
                "The Pi input stream is unavailable. Reopen the session and try again.".to_string()
            }
        })?;

    match result_receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => {
            process.usable.store(false, Ordering::Release);
            Err("Pi could not accept the request. Reopen the session and try again.".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            process.usable.store(false, Ordering::Release);
            Err("The Pi input stream closed before accepting the request. Try again.".into())
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            process.usable.store(false, Ordering::Release);
            if stop_process(process).is_err() {
                return Err(
                    "Pi stopped responding and Tau could not stop it. Restart Tau and try again."
                        .into(),
                );
            }
            Err("Pi did not accept the request in time. Try again.".into())
        }
    }
}

pub(super) fn stop_all_processes(processes: Vec<PiProcess>) {
    let _ = stop_processes(&processes.iter().collect::<Vec<_>>());
}

pub(super) fn stop_process(process: &PiProcess) -> Result<(), String> {
    stop_processes(&[process]).remove(0)
}

pub(super) fn stop_processes(processes: &[&PiProcess]) -> Vec<Result<(), String>> {
    for process in processes {
        process.close_stdin();
    }
    let children = processes
        .iter()
        .map(|process| &process.child)
        .collect::<Vec<_>>();
    stop_children(&children, PI_STOP_GRACE_PERIOD)
}

pub(super) fn stop_child(child: &Arc<Mutex<Child>>) -> Result<(), String> {
    stop_children(&[child], Duration::ZERO).remove(0)
}

fn stop_children(
    children: &[&Arc<Mutex<Child>>],
    grace_period: Duration,
) -> Vec<Result<(), String>> {
    let started = Instant::now();
    let kill_after = started + grace_period;
    let deadline = started + PI_STOP_TIMEOUT;
    let mut results = vec![None; children.len()];
    let mut errors = vec![None; children.len()];
    let mut kill_sent = vec![false; children.len()];

    // All children share a deadline: quitting with many warm runtimes must not
    // wait through one grace period per SSH connection. A blocked stdin writer
    // cannot deliver EOF, so killing the direct child remains the fallback.
    loop {
        for (index, child) in children.iter().enumerate() {
            if results[index].is_some() {
                continue;
            }
            match child.try_lock() {
                Ok(mut child) => match child.try_wait() {
                    Ok(Some(_)) => results[index] = Some(Ok(())),
                    Ok(None) if !kill_sent[index] && Instant::now() >= kill_after => {
                        match child.kill() {
                            Ok(()) => kill_sent[index] = true,
                            Err(_) => {
                                errors[index] =
                                    Some("Tau could not stop Pi. Try again.".to_string());
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        errors[index] =
                            Some("Tau could not check whether Pi stopped. Try again.".to_string());
                    }
                },
                Err(std::sync::TryLockError::WouldBlock) => {}
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    results[index] =
                        Some(Err("Pi process state is unavailable. Try again.".into()));
                }
            }
        }
        if results.iter().all(Option::is_some) || Instant::now() >= deadline {
            return results
                .into_iter()
                .zip(errors)
                .map(|(result, error)| {
                    result.unwrap_or_else(|| {
                        Err(error.unwrap_or_else(|| "Pi did not stop in time. Try again.".into()))
                    })
                })
                .collect();
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

pub(super) fn wait_for_child(child: &Arc<Mutex<Child>>) -> Option<ExitStatus> {
    loop {
        match child.try_lock() {
            Ok(mut child) => match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            },
            Err(std::sync::TryLockError::WouldBlock) => {}
            Err(std::sync::TryLockError::Poisoned(_)) => return None,
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}
