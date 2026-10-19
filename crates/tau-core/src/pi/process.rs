use super::transport::{spawn_stdin_writer, PiWriteRequest, PiWriter};

pub(super) const PI_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const PI_STOP_TIMEOUT: Duration = Duration::from_secs(3);
pub(super) const PI_STOP_GRACE_PERIOD: Duration = Duration::from_secs(1);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
use std::{
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct PiProcess {
    pub(super) remote: bool,
    pub(super) remote_uncertain: Arc<AtomicBool>,
    pub(super) kill_attempted: Arc<AtomicBool>,
    pub(super) owner_id: String,
    pub(super) generation: u64,
    pub(super) sink: super::PiSink,
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) writer: PiWriter,
    pub(super) usable: Arc<AtomicBool>,
    pub(super) output_current: Arc<AtomicBool>,
    pub(super) stopping: Arc<AtomicBool>,
    pub(super) drain: Arc<ReaderDrain>,
}

#[derive(Default)]
pub(super) struct ReaderDrain {
    result: Mutex<Option<(Instant, Result<(), String>)>>,
    ready: Condvar,
    deadline: Mutex<Option<Instant>>,
}
impl ReaderDrain {
    pub(super) fn complete(&self, result: Result<(), String>) {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(|| (Instant::now(), result));
        self.ready.notify_all();
    }

    pub(super) fn expired(&self) -> bool {
        self.deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    #[cfg(windows)]
    pub(super) fn poll_pause(&self) {
        let result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        drop(
            self.ready
                .wait_timeout(result, PROCESS_POLL_INTERVAL)
                .unwrap_or_else(|e| e.into_inner()),
        );
    }

    pub(super) fn wait(&self, deadline: Instant) -> Result<(), String> {
        let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some((completed, result)) = &*result {
                return if *completed <= deadline {
                    result.clone()
                } else {
                    Err(
                        "Pi output did not drain in time. Tau has retained cleanup ownership."
                            .into(),
                    )
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(
                    "Pi output did not drain in time. Tau has retained cleanup ownership.".into(),
                );
            }
            result = self
                .ready
                .wait_timeout(result, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

pub(super) const REMOTE_EXIT_UNCONFIRMED: &str = "Remote Pi exit is unconfirmed. Tau has retained cleanup ownership and blocked replacement. Retrying cannot prove remote exit after a transport failure; external cleanup reconciliation is not yet supported.";

impl PiProcess {
    pub(super) fn confirms_exit(&self, status: Option<ExitStatus>) -> bool {
        if !self.remote {
            return status.is_some();
        }
        // OpenSSH reports the remote command status, or 255 for an SSH error.
        // Never reinterpret a cached status after a local kill as remote proof.
        let confirmed = !self.remote_uncertain.load(Ordering::Acquire)
            && !self.kill_attempted.load(Ordering::Acquire)
            && status
                .and_then(|status| status.code())
                .is_some_and(|code| (0..255).contains(&code));
        if !confirmed {
            self.remote_uncertain.store(true, Ordering::Release);
        }
        confirmed
    }

    fn unavailable_message(&self) -> String {
        if self.remote_uncertain.load(Ordering::Acquire) {
            REMOTE_EXIT_UNCONFIRMED.into()
        } else if self.remote {
            "The SSH transport is no longer accepting requests. This does not confirm remote Pi exit.".into()
        } else {
            "Pi is no longer accepting requests. Restart Tau and try again.".into()
        }
    }

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
type SpawnFailure = (String, Option<Arc<Mutex<Child>>>);

pub(super) fn spawn_child(command: &mut Command) -> Result<SpawnedChild, SpawnFailure> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| (format!("Could not start pi: {error}"), None))?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err((
            "Pi did not expose its process streams.".into(),
            Some(Arc::new(Mutex::new(child))),
        ));
    };
    let child = Arc::new(Mutex::new(child));
    #[cfg(unix)]
    if super::transport::nonblocking_pipe(&stdout).is_err()
        || super::transport::nonblocking_pipe(&stderr).is_err()
    {
        return Err((
            "Tau could not configure Pi output streams.".into(),
            Some(child),
        ));
    }
    let writer = match spawn_stdin_writer(stdin) {
        Ok(writer) => writer,
        Err(error) => return Err((error, Some(child))),
    };
    Ok((child, writer, stdout, stderr))
}

pub(super) fn send_pi_line(
    process: &PiProcess,
    line: Vec<u8>,
    timeout: Duration,
) -> Result<(), String> {
    if !process.usable.load(Ordering::Acquire) {
        return Err(process.unavailable_message());
    }
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    process
        .writer
        .lock()
        .map_err(|_| {
            "The Pi input stream is unavailable. Reopen the session and try again.".to_string()
        })?
        .as_ref()
        .ok_or_else(|| process.unavailable_message())?
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
                return Err(if process.remote {
                    REMOTE_EXIT_UNCONFIRMED.into()
                } else {
                    "Pi stopped responding and Tau could not stop it. Try again.".into()
                });
            }
            Err("Pi did not accept the request in time. Try again.".into())
        }
    }
}

#[cfg(test)]
pub(super) fn stop_all_processes(processes: Vec<PiProcess>) {
    let _ = stop_processes(&processes.iter().collect::<Vec<_>>());
}

pub(super) fn stop_process(process: &PiProcess) -> Result<(), String> {
    stop_processes(&[process]).remove(0).result(process)
}

#[derive(Debug)]
pub(super) struct ChildStopOutcome {
    pub(super) natural_exit_status: Option<ExitStatus>,
    pub(super) reaped_status: Option<ExitStatus>,
    pub(super) kill_sent: bool,
    pub(super) error: Option<String>,
}

impl ChildStopOutcome {
    pub(super) fn result(&self, process: &PiProcess) -> Result<(), String> {
        let status = if process.remote {
            if self.kill_sent {
                process.remote_uncertain.store(true, Ordering::Release);
            }
            self.natural_exit_status
        } else {
            self.reaped_status
        };
        if process.confirms_exit(status) {
            self.error.clone().map_or(Ok(()), Err)
        } else if process.remote {
            Err(REMOTE_EXIT_UNCONFIRMED.into())
        } else {
            Err(self
                .error
                .clone()
                .unwrap_or_else(|| "Pi did not stop in time. Try again.".into()))
        }
    }
}

pub(super) fn stop_processes(processes: &[&PiProcess]) -> Vec<ChildStopOutcome> {
    let started = Instant::now();
    let deadline = started + PI_STOP_TIMEOUT;
    for process in processes {
        process.stopping.store(true, Ordering::Release);
        process
            .drain
            .deadline
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(deadline);
        process.close_stdin();
    }
    let children = processes
        .iter()
        .map(|process| &process.child)
        .collect::<Vec<_>>();
    let kill_attempted = processes
        .iter()
        .map(|process| &process.kill_attempted)
        .collect::<Vec<_>>();
    let mut outcomes = stop_children(&children, &kill_attempted, started, PI_STOP_GRACE_PERIOD);
    for (process, outcome) in processes.iter().zip(&mut outcomes) {
        if let Err(error) = process.drain.wait(deadline) {
            outcome.error = Some(error);
        }
        // Record uncertainty even if the caller only performs best-effort cleanup.
        let _ = outcome.result(process);
    }
    outcomes
}

pub(super) fn stop_child(child: &Arc<Mutex<Child>>) -> Result<(), String> {
    let killed = Arc::new(AtomicBool::new(false));
    let outcome = stop_children(&[child], &[&killed], Instant::now(), Duration::ZERO).remove(0);
    if outcome.reaped_status.is_some() {
        Ok(())
    } else {
        Err(outcome
            .error
            .unwrap_or_else(|| "Pi did not stop in time. Try again.".into()))
    }
}

fn stop_children(
    children: &[&Arc<Mutex<Child>>],
    kill_attempted: &[&Arc<AtomicBool>],
    started: Instant,
    grace_period: Duration,
) -> Vec<ChildStopOutcome> {
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
                    Ok(Some(status)) => results[index] = Some(Ok(status)),
                    Ok(None) if !kill_sent[index] && Instant::now() >= kill_after => {
                        kill_attempted[index].store(true, Ordering::Release);
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
                .enumerate()
                .map(|(index, (result, error))| {
                    let (status, error) = match result {
                        Some(Ok(status)) => (Some(status), None),
                        Some(Err(error)) => (None, Some(error)),
                        None => (None, error),
                    };
                    ChildStopOutcome {
                        natural_exit_status: status
                            .filter(|_| !kill_attempted[index].load(Ordering::Acquire)),
                        reaped_status: status,
                        kill_sent: kill_sent[index],
                        error,
                    }
                })
                .collect();
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

#[cfg(test)]
pub(super) fn wait_for_child(child: &Arc<Mutex<Child>>) -> Option<ExitStatus> {
    wait_for_child_until(child, None)
}

pub(super) fn wait_for_child_until(
    child: &Arc<Mutex<Child>>,
    drain: Option<&ReaderDrain>,
) -> Option<ExitStatus> {
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
        if drain.is_some_and(ReaderDrain::expired) {
            return None;
        }
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}
