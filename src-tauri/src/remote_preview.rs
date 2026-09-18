use crate::{
    pi::PiState,
    ssh::SshConnection,
    storage,
    telemetry::{trace_context::TraceContext, Telemetry},
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{State, WebviewWindow};
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREAMBLE_BYTES: usize = 64 * 1024;
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(30);
const TOKEN_LIFETIME: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct RemotePreviewState {
    inner: Arc<Mutex<PreviewManager>>,
    transfer_gate: Arc<Mutex<()>>,
}

impl Default for RemotePreviewState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PreviewManager::default())),
            transfer_gate: Arc::new(Mutex::new(())),
        }
    }
}

#[derive(Default)]
struct PreviewManager {
    generation: u64,
    owner_revision: u64,
    staged: Option<Snapshot>,
    queued: Option<QueuedSnapshot>,
    displayed: Option<Snapshot>,
    active: Option<ActiveTransfer>,
}

struct ActiveTransfer {
    generation: u64,
    owner: String,
    request_id: String,
    cancel: Arc<AtomicBool>,
    process_id: Arc<AtomicU32>,
}

struct QueuedSnapshot {
    snapshot: Snapshot,
    permit: Arc<AtomicBool>,
}

#[derive(Clone)]
struct Snapshot {
    token: String,
    path: PathBuf,
    _directory: Arc<SnapshotDirectory>,
    generation: u64,
    window: String,
    owner: String,
    owner_revision: u64,
    request_id: String,
    created: Instant,
}

struct SnapshotDirectory(PathBuf);

impl SnapshotDirectory {
    fn create() -> Result<Self, String> {
        #[cfg(unix)]
        use std::os::unix::fs::DirBuilderExt;

        for _ in 0..8 {
            let path =
                std::env::temp_dir().join(format!("tau-preview-{}", Uuid::new_v4().simple()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(unavailable()),
            }
        }
        Err(unavailable())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SnapshotDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreparedRemotePath {
    Directory,
    File { token: String },
}

#[tauri::command]
pub fn remote_preview_available() -> bool {
    cfg!(target_os = "macos")
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn prepare_remote_path(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    project_path: String,
    request_id: String,
    path: String,
) -> Result<PreparedRemotePath, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "prepare_remote_path"));
    validate_identifier(&request_id)?;
    validate_path(&path)?;
    let remote = storage::remote_project(&project_path)?;
    let window_label = window.label().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let (generation, owner_revision) = pi.with_owner(&owner_id, |owner_revision| {
        let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
        manager.generation = manager.generation.checked_add(1).ok_or_else(unavailable)?;
        manager.owner_revision = owner_revision;
        manager.staged.take();
        invalidate_queued(&mut manager);
        stop_active_transfer(&mut manager);
        let generation = manager.generation;
        manager.active = Some(ActiveTransfer {
            generation,
            owner: owner_id.clone(),
            request_id: request_id.clone(),
            cancel: cancel.clone(),
            process_id: Arc::new(AtomicU32::new(0)),
        });
        Ok((generation, owner_revision))
    })?;
    let transfer_state = previews.inner.clone();
    let transfer_gate = previews.transfer_gate.clone();
    let transfer_request = request_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = transfer_gate.lock().map_err(|_| unavailable())?;
        if cancel.load(Ordering::Acquire) {
            return Err("The remote file preview was superseded.".into());
        }
        let transfer_result = transfer_remote_path(
            &transfer_state,
            &remote.connection_string,
            &remote.working_directory,
            &path,
            generation,
            window_label,
            owner_id,
            owner_revision,
            transfer_request,
            cancel,
        );
        let mut manager = transfer_state.lock().map_err(|_| unavailable())?;
        let current = manager.active.as_ref().is_some_and(|active| {
            active.generation == generation && !active.cancel.load(Ordering::Acquire)
        }) && manager.generation == generation
            && manager.owner_revision == owner_revision;
        if manager
            .active
            .as_ref()
            .is_some_and(|active| active.generation == generation)
        {
            manager.active.take();
        }
        if !current {
            return Err("The remote file preview was superseded.".into());
        }
        match transfer_result? {
            TransferResult::Directory => Ok(PreparedRemotePath::Directory),
            TransferResult::File(snapshot) => {
                let token = snapshot.token.clone();
                manager.staged = Some(snapshot);
                drop(manager);
                schedule_token_expiry(transfer_state.clone(), generation, token.clone());
                Ok(PreparedRemotePath::File { token })
            }
        }
    })
    .await
    .map_err(|_| "The remote file preview could not be prepared.".to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn show_remote_preview(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    request_id: String,
    token: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "show_remote_preview"));
    let permit = Arc::new(AtomicBool::new(true));
    pi.with_owner(&owner_id, |owner_revision| {
        let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
        let valid = manager.staged.as_ref().is_some_and(|snapshot| {
            snapshot.token == token
                && snapshot.window == window.label()
                && snapshot.owner == owner_id
                && snapshot.owner_revision == owner_revision
                && snapshot.request_id == request_id
                && snapshot.created.elapsed() < TOKEN_LIFETIME
        });
        if !valid {
            return Err("The remote file preview is no longer available.".into());
        }
        let snapshot = manager.staged.take().expect("validated staged snapshot");
        manager.queued = Some(QueuedSnapshot {
            snapshot,
            permit: permit.clone(),
        });
        Ok(())
    })?;

    #[cfg(target_os = "macos")]
    {
        let state = previews.inner.clone();
        let queued_token = token.clone();
        let queued_request = request_id.clone();
        let closure_permit = permit.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        if window
            .run_on_main_thread(move || {
                let snapshot = (|| {
                    let manager = state.lock().map_err(|_| unavailable())?;
                    manager
                        .queued
                        .as_ref()
                        .filter(|queued| {
                            queued.snapshot.token == queued_token
                                && queued.snapshot.request_id == queued_request
                                && Arc::ptr_eq(&queued.permit, &closure_permit)
                                && queued.permit.load(Ordering::Acquire)
                                && queued.snapshot.generation == manager.generation
                                && queued.snapshot.owner_revision == manager.owner_revision
                        })
                        .map(|queued| queued.snapshot.clone())
                        .ok_or_else(|| {
                            "The remote file preview is no longer available.".to_string()
                        })
                })();
                let result = snapshot.and_then(|snapshot| {
                    if let Err(error) = macos::show(&snapshot.token, &snapshot.path) {
                        macos::close_if(&snapshot.token);
                        return Err(error);
                    }
                    if commit_presented_snapshot(&state, &snapshot.token, &closure_permit).is_err()
                    {
                        macos::close_if(&snapshot.token);
                        return Err("The remote file preview was superseded.".into());
                    }
                    Ok(())
                });
                let _ = sender.send(result);
            })
            .is_err()
        {
            permit.store(false, Ordering::Release);
            revoke_and_close_presentation(&window, &previews.inner, &token);
            return Err("Quick Look could not be opened.".into());
        }
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                monitor_displayed_snapshot(previews.inner.clone(), window.clone(), token.clone());
            }
            Ok(Err(error)) => {
                permit.store(false, Ordering::Release);
                revoke_and_close_presentation(&window, &previews.inner, &token);
                return Err(error);
            }
            Err(_) => {
                permit.store(false, Ordering::Release);
                revoke_and_close_presentation(&window, &previews.inner, &token);
                return Err("Quick Look did not respond.".into());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        permit.store(false, Ordering::Release);
        revoke_presentation(&previews.inner, &token);
        return Err("Quick Look is only available on macOS.".into());
    }

    Ok(())
}

#[tauri::command]
pub fn cancel_remote_path(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    owner_id: String,
    request_id: String,
) -> Result<(), String> {
    let mut manager = pi.with_owner(&owner_id, |_| {
        previews.inner.lock().map_err(|_| unavailable())
    })?;
    if manager
        .active
        .as_ref()
        .is_some_and(|active| active.request_id == request_id)
    {
        manager.generation = manager.generation.saturating_add(1);
        stop_active_transfer(&mut manager);
    }
    if manager
        .staged
        .as_ref()
        .is_some_and(|snapshot| snapshot.request_id == request_id)
    {
        manager.staged.take();
    }
    if manager
        .queued
        .as_ref()
        .is_some_and(|queued| queued.snapshot.request_id == request_id)
    {
        invalidate_queued(&mut manager);
    }
    let displayed = manager
        .displayed
        .as_ref()
        .is_some_and(|snapshot| snapshot.request_id == request_id)
        .then(|| manager.displayed.take())
        .flatten();
    drop(manager);
    if let Some(snapshot) = displayed {
        #[cfg(target_os = "macos")]
        {
            let displayed_token = snapshot.token.clone();
            let _ = window.run_on_main_thread(move || {
                macos::close_if(&displayed_token);
                drop(snapshot);
            });
        }
        #[cfg(not(target_os = "macos"))]
        drop(snapshot);
    }
    Ok(())
}

impl RemotePreviewState {
    pub fn replace_owner(&self, window: &WebviewWindow, owner_id: &str, owner_revision: u64) {
        let displayed = if let Ok(mut manager) = self.inner.lock() {
            let owns_work = manager.owner_revision != owner_revision
                || manager
                    .active
                    .as_ref()
                    .is_some_and(|active| active.owner != owner_id)
                || manager
                    .staged
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.owner != owner_id)
                || manager
                    .queued
                    .as_ref()
                    .is_some_and(|queued| queued.snapshot.owner != owner_id);
            if owns_work {
                manager.generation = manager.generation.saturating_add(1);
                stop_active_transfer(&mut manager);
                manager.staged.take();
                invalidate_queued(&mut manager);
            }
            manager.owner_revision = owner_revision;
            manager
                .displayed
                .as_ref()
                .filter(|snapshot| snapshot.owner != owner_id)
                .map(|snapshot| snapshot.token.clone())
                .and_then(|token| manager.displayed.take().map(|snapshot| (token, snapshot)))
        } else {
            None
        };
        if let Some((token, snapshot)) = displayed {
            #[cfg(target_os = "macos")]
            {
                let _ = window.run_on_main_thread(move || {
                    macos::close_if(&token);
                    drop(snapshot);
                });
            }
            #[cfg(not(target_os = "macos"))]
            drop((token, snapshot));
        }
    }

    pub fn shutdown(&self) {
        #[cfg(target_os = "macos")]
        macos::close();
        if let Ok(mut manager) = self.inner.lock() {
            manager.generation = manager.generation.saturating_add(1);
            stop_active_transfer(&mut manager);
            manager.staged.take();
            invalidate_queued(&mut manager);
            manager.displayed.take();
        }
        // A worker owns partial snapshots and its process group while holding
        // this gate. Waiting for it makes normal RunEvent::Exit cleanup real,
        // rather than relying on destructors after Tauri exits.
        let _retired = self.transfer_gate.lock();
    }
}

fn schedule_token_expiry(state: Arc<Mutex<PreviewManager>>, generation: u64, token: String) {
    std::thread::spawn(move || {
        std::thread::sleep(TOKEN_LIFETIME);
        if let Ok(mut manager) = state.lock() {
            if manager.generation == generation
                && manager
                    .staged
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.token == token)
            {
                manager.staged.take();
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn monitor_displayed_snapshot(
    state: Arc<Mutex<PreviewManager>>,
    window: WebviewWindow,
    token: String,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let checked_token = token.clone();
        if window
            .run_on_main_thread(move || {
                let visible = macos::is_visible(&checked_token);
                if !visible {
                    macos::close_if(&checked_token);
                }
                let _ = sender.send(visible);
            })
            .is_err()
        {
            break;
        }
        let visible = loop {
            match receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(visible) => break visible,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break false,
            }
        };
        if visible {
            continue;
        }
        if let Ok(mut manager) = state.lock() {
            if manager
                .displayed
                .as_ref()
                .is_some_and(|snapshot| snapshot.token == token)
            {
                manager.displayed.take();
            }
        }
        break;
    });
}

#[cfg(target_os = "macos")]
fn revoke_and_close_presentation(
    window: &WebviewWindow,
    state: &Arc<Mutex<PreviewManager>>,
    token: &str,
) {
    if let Some(snapshot) = revoke_presentation(state, token) {
        let displayed_token = snapshot.token.clone();
        let _ = window.run_on_main_thread(move || {
            macos::close_if(&displayed_token);
            drop(snapshot);
        });
    }
}

fn commit_presented_snapshot(
    state: &Arc<Mutex<PreviewManager>>,
    token: &str,
    permit: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut manager = state.lock().map_err(|_| unavailable())?;
    let current = manager.queued.as_ref().is_some_and(|queued| {
        queued.snapshot.token == token
            && Arc::ptr_eq(&queued.permit, permit)
            && queued.permit.load(Ordering::Acquire)
            && queued.snapshot.generation == manager.generation
            && queued.snapshot.owner_revision == manager.owner_revision
    });
    if !current {
        return Err("The remote file preview was superseded.".into());
    }
    let displayed = manager.queued.take().expect("validated queued snapshot");
    manager.displayed = Some(displayed.snapshot);
    Ok(())
}

fn revoke_presentation(state: &Arc<Mutex<PreviewManager>>, token: &str) -> Option<Snapshot> {
    let mut manager = state.lock().ok()?;
    if manager
        .queued
        .as_ref()
        .is_some_and(|queued| queued.snapshot.token == token)
    {
        invalidate_queued(&mut manager);
    }
    if manager
        .displayed
        .as_ref()
        .is_some_and(|snapshot| snapshot.token == token)
    {
        manager.displayed.take()
    } else {
        None
    }
}

fn invalidate_queued(manager: &mut PreviewManager) {
    if let Some(queued) = manager.queued.take() {
        queued.permit.store(false, Ordering::Release);
    }
}

fn stop_active_transfer(manager: &mut PreviewManager) {
    if let Some(active) = manager.active.take() {
        active.cancel.store(true, Ordering::Release);
        let process_id = active.process_id.swap(0, Ordering::AcqRel);
        if process_id != 0 {
            kill_process_group(process_id);
        }
    }
}

fn unavailable() -> String {
    "Remote preview is unavailable. Try again.".into()
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err("The remote preview request is invalid.".into());
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 16 * 1024 || path.chars().any(char::is_control) {
        return Err("The remote path is invalid.".into());
    }
    Ok(())
}

enum TransferResult {
    Directory,
    File(Snapshot),
}

#[allow(clippy::too_many_arguments)]
fn transfer_remote_path(
    coordinator: &Arc<Mutex<PreviewManager>>,
    connection_string: &str,
    working_directory: &str,
    authored_path: &str,
    generation: u64,
    window: String,
    owner: String,
    owner_revision: u64,
    request_id: String,
    cancel: Arc<AtomicBool>,
) -> Result<TransferResult, String> {
    let marker = format!("TAU_PREVIEW_{}", Uuid::new_v4().simple());
    let script = r#"p=$1
case "$p" in "~/"*) p=$HOME/${p#\~/} ;; /*) ;; *) p=$2/$p ;; esac
if [ -d "$p" ]; then printf '\000%s\000D\000' "$3"; exit 0; fi
if [ ! -f "$p" ]; then printf '\000%s\000E\000unreadable\000' "$3"; exit 44; fi
size=$(wc -c < "$p") || { printf '\000%s\000E\000unreadable\000' "$3"; exit 45; }
size=$(printf '%s' "$size" | tr -d '[:space:]')
base=${p##*/}
printf '\000%s\000F\000%s\000%s\000' "$3" "$size" "$base"
cat -- "$p" || exit 46
printf '\000%s\000END\000' "$3""#;
    let command = format!(
        "exec /bin/sh -c {} tau {} {} {}",
        shell_words::quote(script),
        shell_words::quote(authored_path),
        shell_words::quote(working_directory),
        shell_words::quote(&marker),
    );
    let connection = SshConnection::parse(connection_string)?;
    let mut command_builder = connection.transfer_command(&command)?;
    configure_process_group(&mut command_builder);
    let mut child = command_builder
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The SSH transfer could not be started.".to_string())?;
    let process_id = child.id();
    let stdout = child.stdout.take().ok_or_else(unavailable)?;
    if let Err(error) = set_nonblocking(&stdout) {
        kill_process_group(process_id);
        let _ = child.wait();
        return Err(error);
    }
    let process_registration = {
        let mut manager = coordinator.lock().map_err(|_| unavailable())?;
        let owner_is_current = manager.owner_revision == owner_revision;
        let active = manager.active.as_mut().filter(|active| {
            owner_is_current
                && active.generation == generation
                && active.owner == owner
                && active.request_id == request_id
                && !active.cancel.load(Ordering::Acquire)
        });
        let Some(active) = active else {
            kill_process_group(process_id);
            let _ = child.wait();
            return Err("The remote file preview was superseded.".into());
        };
        active.process_id.store(process_id, Ordering::Release);
        active.process_id.clone()
    };
    let started = Instant::now();
    let finished = Arc::new(AtomicBool::new(false));
    start_watchdog(process_id, cancel.clone(), finished.clone(), started);
    let mut process = TransferProcess {
        child,
        stdout,
        process_id,
        process_registration,
        finished,
        retired: false,
    };

    let prefix = format!("\0{marker}\0").into_bytes();
    find_prefix(&mut process.stdout, &prefix, started, &cancel)?;
    check_transfer(&cancel, started)?;
    let kind = read_field(&mut process.stdout, 16, started, &cancel)?;
    if kind == b"D" {
        let status = process.wait(&cancel, started)?;
        return if status.success() {
            Ok(TransferResult::Directory)
        } else {
            Err("The remote directory could not be inspected.".into())
        };
    }
    if kind == b"E" {
        let _category = read_field(&mut process.stdout, 32, started, &cancel)?;
        let _ = process.wait(&cancel, started)?;
        return Err("The remote file could not be read.".into());
    }
    if kind != b"F" {
        return Err("The remote file response was invalid.".into());
    }
    let size_text = read_field(&mut process.stdout, 32, started, &cancel)?;
    let size = std::str::from_utf8(&size_text)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "The remote file size was invalid.".to_string())?;
    if size > MAX_FILE_BYTES {
        return Err("The remote file is larger than 64 MiB.".into());
    }
    let basename = String::from_utf8(read_field(&mut process.stdout, 1024, started, &cancel)?)
        .map_err(|_| "The remote filename is unsupported.".to_string())?;
    if basename.is_empty()
        || basename == "."
        || basename == ".."
        || basename.contains(['/', '\\'])
        || basename.chars().any(char::is_control)
    {
        return Err("The remote filename is unsupported.".into());
    }
    let directory = Arc::new(SnapshotDirectory::create()?);
    let output_path = directory.path().join(basename);
    write_payload(&mut process.stdout, &output_path, size, started, &cancel)?;
    let expected_trailer = format!("\0{marker}\0END\0").into_bytes();
    let mut trailer = vec![0; expected_trailer.len()];
    read_exact_deadline(&mut process.stdout, &mut trailer, started, &cancel)?;
    if trailer != expected_trailer {
        return Err("The remote file transfer was incomplete.".into());
    }
    let status = process.wait(&cancel, started)?;
    if !status.success() {
        return Err("The remote file could not be read.".into());
    }
    make_read_only(&output_path)?;
    Ok(TransferResult::File(Snapshot {
        token: Uuid::new_v4().to_string(),
        path: output_path,
        _directory: directory,
        generation,
        window,
        owner,
        owner_revision,
        request_id,
        created: Instant::now(),
    }))
}

struct TransferProcess {
    child: Child,
    stdout: std::process::ChildStdout,
    process_id: u32,
    process_registration: Arc<AtomicU32>,
    finished: Arc<AtomicBool>,
    retired: bool,
}

impl TransferProcess {
    fn wait(
        &mut self,
        cancel: &AtomicBool,
        started: Instant,
    ) -> Result<std::process::ExitStatus, String> {
        loop {
            if let Some(status) = self.child.try_wait().map_err(|_| unavailable())? {
                // The group leader may exit while proxy/wrapper descendants
                // still own pipes or credentials. Retire the whole group on
                // successful and failed terminal paths alike.
                self.process_registration.store(0, Ordering::Release);
                kill_process_group(self.process_id);
                self.retired = true;
                self.finished.store(true, Ordering::Release);
                return Ok(status);
            }
            check_transfer(cancel, started)?;
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for TransferProcess {
    fn drop(&mut self) {
        if !self.retired {
            self.process_registration.store(0, Ordering::Release);
            kill_process_group(self.process_id);
            let _ = self.child.wait();
            self.retired = true;
        }
        self.finished.store(true, Ordering::Release);
    }
}

fn start_watchdog(
    process_id: u32,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    started: Instant,
) {
    std::thread::spawn(move || {
        while !finished.load(Ordering::Acquire) {
            if cancel.load(Ordering::Acquire) || started.elapsed() >= PREPARATION_TIMEOUT {
                kill_process_group(process_id);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_group(process_id: u32) {
    if let Ok(process_id) = i32::try_from(process_id) {
        // SAFETY: a negative pid addresses the process group created for this transfer.
        unsafe {
            libc::kill(-process_id, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(_process_id: u32) {}

#[cfg(unix)]
fn set_nonblocking(stdout: &std::process::ChildStdout) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    let descriptor = stdout.as_raw_fd();
    // SAFETY: fcntl only reads or updates flags for this live pipe descriptor.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_nonblocking(_stdout: &std::process::ChildStdout) -> Result<(), String> {
    Ok(())
}

fn check_transfer(cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("The remote file preview was superseded.".into());
    }
    if started.elapsed() >= PREPARATION_TIMEOUT {
        return Err("The remote file preview timed out.".into());
    }
    Ok(())
}

fn find_prefix(
    reader: &mut impl Read,
    prefix: &[u8],
    started: Instant,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut matched = 0;
    for _ in 0..MAX_PREAMBLE_BYTES {
        let mut byte = [0];
        read_exact_deadline(reader, &mut byte, started, cancel)?;
        matched = if byte[0] == prefix[matched] {
            matched + 1
        } else if byte[0] == prefix[0] {
            1
        } else {
            0
        };
        if matched == prefix.len() {
            return Ok(());
        }
    }
    Err("The SSH response did not contain a preview.".into())
}

fn read_field(
    reader: &mut impl Read,
    max: usize,
    started: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut value = Vec::new();
    while value.len() <= max {
        let mut byte = [0];
        read_exact_deadline(reader, &mut byte, started, cancel)?;
        if byte[0] == 0 {
            return Ok(value);
        }
        value.push(byte[0]);
    }
    Err("The remote file response was invalid.".into())
}

fn read_exact_deadline(
    reader: &mut impl Read,
    output: &mut [u8],
    started: Instant,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut offset = 0;
    while offset < output.len() {
        check_transfer(cancel, started)?;
        match reader.read(&mut output[offset..]) {
            Ok(0) => return Err("The remote file transfer was incomplete.".into()),
            Ok(count) => offset += count,
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return Err("The remote file transfer was incomplete.".into()),
        }
    }
    Ok(())
}

fn write_payload(
    reader: &mut impl Read,
    path: &Path,
    size: u64,
    started: Instant,
    cancel: &AtomicBool,
) -> Result<(), String> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).map_err(|_| unavailable())?;
    let mut remaining = size;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let count = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        read_exact_deadline(reader, &mut buffer[..count], started, cancel)?;
        file.write_all(&buffer[..count])
            .map_err(|_| unavailable())?;
        remaining -= count as u64;
    }
    file.sync_all().map_err(|_| unavailable())
}

fn make_read_only(path: &Path) -> Result<(), String> {
    let mut permissions = fs::metadata(path).map_err(|_| unavailable())?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|_| unavailable())
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::{NSBackingStoreType, NSPanel, NSWindowStyleMask};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString, NSURL};
    use objc2_quick_look_ui::{QLPreviewItem, QLPreviewView, QLPreviewViewStyle};
    use std::{cell::RefCell, path::Path};

    struct PreviewPanel {
        request_id: String,
        panel: objc2::rc::Retained<NSPanel>,
        _view: objc2::rc::Retained<QLPreviewView>,
        _url: objc2::rc::Retained<NSURL>,
    }
    thread_local! { static PANEL: RefCell<Option<PreviewPanel>> = const { RefCell::new(None) }; }

    pub fn show(request_id: &str, path: &Path) -> Result<(), String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "Quick Look must open on the main thread.".to_string())?;
        let path = path
            .to_str()
            .ok_or_else(|| "The preview filename is unsupported.".to_string())?;
        let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(800., 600.));
        unsafe {
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            );
            panel.setTitle(&NSString::from_str("Remote Preview"));
            let view =
                QLPreviewView::initWithFrame_style(mtm.alloc(), frame, QLPreviewViewStyle::Normal)
                    .ok_or_else(|| "Quick Look could not create a preview view.".to_string())?;
            view.setAutostarts(false);
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            let item = objc2::runtime::ProtocolObject::<dyn QLPreviewItem>::from_ref(&*url);
            view.setPreviewItem(Some(item));
            panel.setContentView(Some(&view));
            panel.center();
            panel.makeKeyAndOrderFront(None);
            PANEL.with(|slot| {
                if let Some(previous) = slot.borrow_mut().take() {
                    previous.panel.close();
                }
                *slot.borrow_mut() = Some(PreviewPanel {
                    request_id: request_id.into(),
                    panel,
                    _view: view,
                    _url: url,
                })
            });
        }
        Ok(())
    }

    pub fn is_visible(request_id: &str) -> bool {
        MainThreadMarker::new().is_some()
            && PANEL.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .is_some_and(|panel| panel.request_id == request_id && panel.panel.isVisible())
            })
    }

    pub fn close_if(request_id: &str) {
        if MainThreadMarker::new().is_some() {
            PANEL.with(|slot| {
                let matches = slot
                    .borrow()
                    .as_ref()
                    .is_some_and(|panel| panel.request_id == request_id);
                if matches {
                    if let Some(panel) = slot.borrow_mut().take() {
                        panel.panel.close();
                    }
                }
            });
        }
    }

    pub fn close() {
        if MainThreadMarker::new().is_some() {
            PANEL.with(|slot| {
                if let Some(panel) = slot.borrow_mut().take() {
                    panel.panel.close();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(token: &str, generation: u64) -> Snapshot {
        let directory = Arc::new(SnapshotDirectory::create().unwrap());
        let path = directory.path().join("preview.txt");
        fs::write(&path, b"preview").unwrap();
        Snapshot {
            token: token.into(),
            path,
            _directory: directory,
            generation,
            window: "main".into(),
            owner: "owner".into(),
            owner_revision: 1,
            request_id: format!("request-{token}"),
            created: Instant::now(),
        }
    }

    #[test]
    fn presentation_revocation_cleans_queued_and_just_displayed_tokens() {
        let queued_permit = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            owner_revision: 1,
            queued: Some(QueuedSnapshot {
                snapshot: snapshot("queued", 1),
                permit: queued_permit.clone(),
            }),
            ..Default::default()
        }));
        queued_permit.store(false, Ordering::Release);
        assert!(revoke_presentation(&state, "queued").is_none());
        assert!(state.lock().unwrap().queued.is_none());

        let displayed_permit = Arc::new(AtomicBool::new(true));
        state.lock().unwrap().queued = Some(QueuedSnapshot {
            snapshot: snapshot("displayed", 1),
            permit: displayed_permit.clone(),
        });
        commit_presented_snapshot(&state, "displayed", &displayed_permit).unwrap();
        displayed_permit.store(false, Ordering::Release);
        let retired = revoke_presentation(&state, "displayed").expect("displayed snapshot");
        assert_eq!(retired.token, "displayed");
        assert!(state.lock().unwrap().displayed.is_none());
    }

    #[test]
    fn presentation_finishing_after_revocation_is_rejected() {
        use std::sync::Barrier;

        let permit = Arc::new(AtomicBool::new(true));
        let state = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            owner_revision: 1,
            queued: Some(QueuedSnapshot {
                snapshot: snapshot("late", 1),
                permit: permit.clone(),
            }),
            ..Default::default()
        }));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_state = state.clone();
        let worker_permit = permit.clone();
        let worker_entered = entered.clone();
        let worker_release = release.clone();
        let completion = std::thread::spawn(move || {
            worker_entered.wait();
            worker_release.wait();
            commit_presented_snapshot(&worker_state, "late", &worker_permit)
        });

        entered.wait();
        permit.store(false, Ordering::Release);
        assert!(revoke_presentation(&state, "late").is_none());
        release.wait();

        assert!(completion.join().unwrap().is_err());
        let manager = state.lock().unwrap();
        assert!(manager.queued.is_none());
        assert!(manager.displayed.is_none());
    }

    #[test]
    fn rejects_control_characters_and_empty_paths() {
        assert!(validate_path("").is_err());
        assert!(validate_path("safe/file.txt").is_ok());
        assert!(validate_path("bad\nfile").is_err());
    }

    #[test]
    fn finds_marker_after_a_banner_without_consuming_payload() {
        let mut input = &b"banner\r\n\0TOKEN\0F\0"[..];
        let cancel = AtomicBool::new(false);
        find_prefix(&mut input, b"\0TOKEN\0", Instant::now(), &cancel).expect("marker");
        assert_eq!(
            read_field(&mut input, 16, Instant::now(), &cancel).unwrap(),
            b"F"
        );
    }

    #[test]
    fn writes_binary_payload_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("binary");
        let bytes = b"\0TOKEN\0END\0\xff";
        write_payload(
            &mut &bytes[..],
            &path,
            bytes.len() as u64,
            Instant::now(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[cfg(unix)]
    fn fake_ssh(script: &str) -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ssh");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let connection = format!("{} fixture", executable.display());
        (directory, connection)
    }

    #[cfg(unix)]
    fn transfer(
        coordinator: &Arc<Mutex<PreviewManager>>,
        connection: &str,
        cwd: &str,
        path: &str,
        generation: u64,
    ) -> Result<TransferResult, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        coordinator.lock().unwrap().active = Some(ActiveTransfer {
            generation,
            owner: "owner".into(),
            request_id: format!("request-{generation}"),
            cancel: cancel.clone(),
            process_id: Arc::new(AtomicU32::new(0)),
        });
        transfer_remote_path(
            coordinator,
            connection,
            cwd,
            path,
            generation,
            "main".into(),
            "owner".into(),
            0,
            format!("request-{generation}"),
            cancel,
        )
    }

    #[cfg(unix)]
    #[test]
    fn transfers_binary_home_and_absolute_files_through_fake_ssh() {
        use std::os::unix::fs::PermissionsExt;
        let remote = tempfile::tempdir().unwrap();
        let bytes = b"binary\0payload\xff\0TAU_PREVIEW_marker\0END\0";
        fs::write(remote.path().join("quoted name.bin"), bytes).unwrap();
        let (_ssh_dir, connection) = fake_ssh(
            "#!/bin/sh\nfor last do :; done\nHOME=$TAU_TEST_HOME exec /bin/sh -c \"$last\"\n",
        );
        std::env::set_var("TAU_TEST_HOME", remote.path());

        for path in [
            "~/quoted name.bin".to_string(),
            remote.path().join("quoted name.bin").display().to_string(),
        ] {
            let coordinator = Arc::new(Mutex::new(PreviewManager {
                generation: 1,
                ..Default::default()
            }));
            let result = transfer(&coordinator, &connection, "/", &path, 1).expect("file");
            let TransferResult::File(snapshot) = result else {
                panic!("expected a file");
            };
            assert_eq!(fs::read(&snapshot.path).unwrap(), bytes);
            assert!(fs::metadata(&snapshot.path)
                .unwrap()
                .permissions()
                .readonly());
            assert_eq!(
                fs::metadata(snapshot._directory.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn classifies_directories_through_fake_ssh() {
        let remote = tempfile::tempdir().unwrap();
        fs::create_dir(remote.path().join("folder")).unwrap();
        let (_ssh_dir, connection) =
            fake_ssh("#!/bin/sh\nfor last do :; done\nexec /bin/sh -c \"$last\"\n");
        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));
        assert!(matches!(
            transfer(
                &coordinator,
                &connection,
                remote.path().to_str().unwrap(),
                "folder",
                1
            )
            .expect("directory"),
            TransferResult::Directory
        ));
    }

    #[cfg(unix)]
    #[test]
    fn failed_transfer_clears_the_retired_process_registration() {
        let (_ssh_dir, connection) =
            fake_ssh("#!/bin/sh\nfor last do :; done\nexec /bin/sh -c \"$last\"\n");
        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));

        let result = transfer(&coordinator, &connection, "/tmp", "tau-missing-preview", 1);

        assert_eq!(
            result.err().as_deref(),
            Some("The remote file could not be read.")
        );
        let manager = coordinator.lock().unwrap();
        assert_eq!(
            manager
                .active
                .as_ref()
                .unwrap()
                .process_id
                .load(Ordering::Acquire),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn deadline_interrupts_a_child_pipe_held_by_a_helper() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 60 & printf x"]);
        configure_process_group(&mut command);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let process_id = child.id();
        let mut reader = child.stdout.take().unwrap();
        set_nonblocking(&reader).unwrap();
        read_exact_deadline(
            &mut reader,
            &mut [0],
            Instant::now(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let started = Instant::now()
            .checked_sub(PREPARATION_TIMEOUT - Duration::from_millis(50))
            .unwrap();
        let began = Instant::now();

        let error = read_exact_deadline(&mut reader, &mut [0], started, &AtomicBool::new(false))
            .unwrap_err();

        kill_process_group(process_id);
        let _ = child.wait();
        assert_eq!(error, "The remote file preview timed out.");
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn successful_leader_exit_still_retires_process_group_helpers() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "sleep 60 >/dev/null 2>&1 & helper=$!; printf '%s\\n' \"$helper\"; exit 0",
        ]);
        configure_process_group(&mut command);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let mut pid = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(&mut stdout), &mut pid).unwrap();
        let helper = pid.trim().parse::<i32>().unwrap();
        let process_id = child.id();
        let finished = Arc::new(AtomicBool::new(false));
        let mut process = TransferProcess {
            child,
            stdout,
            process_id,
            process_registration: Arc::new(AtomicU32::new(process_id)),
            finished,
            retired: false,
        };

        process
            .wait(&AtomicBool::new(false), Instant::now())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { libc::kill(helper, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(unsafe { libc::kill(helper, 0) }, 0, "helper survived");
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_a_process_tree_without_waiting_on_child_lock() {
        let (_ssh_dir, connection) = fake_ssh("#!/bin/sh\nsleep 60 &\nwait\n");
        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));
        let transfer_coordinator = coordinator.clone();
        let transfer = std::thread::spawn(move || {
            transfer(&transfer_coordinator, &connection, "/tmp", "missing", 1)
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if coordinator.lock().unwrap().active.is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "SSH child was not registered");
            std::thread::sleep(Duration::from_millis(10));
        }
        {
            let mut manager = coordinator.lock().unwrap();
            manager.generation = 2;
            stop_active_transfer(&mut manager);
        }
        assert!(transfer.join().unwrap().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn completed_protocol_that_never_exits_can_be_cancelled() {
        let (_ssh_dir, connection) =
            fake_ssh("#!/bin/sh\nfor last do :; done\n/bin/sh -c \"$last\"\nsleep 60\n");
        let remote = tempfile::tempdir().unwrap();
        fs::create_dir(remote.path().join("folder")).unwrap();
        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));
        let transfer_coordinator = coordinator.clone();
        let cwd = remote.path().display().to_string();
        let transfer = std::thread::spawn(move || {
            transfer(&transfer_coordinator, &connection, &cwd, "folder", 1)
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while coordinator.lock().unwrap().active.is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        {
            let mut manager = coordinator.lock().unwrap();
            manager.generation = 2;
            stop_active_transfer(&mut manager);
        }
        assert!(transfer.join().unwrap().is_err());
    }
}
