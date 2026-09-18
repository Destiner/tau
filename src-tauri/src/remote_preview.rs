use crate::{
    pi::PiState,
    ssh::SshConnection,
    storage,
    telemetry::{trace_context::TraceContext, Telemetry},
};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
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
    process_id: u32,
}

struct QueuedSnapshot {
    snapshot: Snapshot,
    permit: Arc<AtomicBool>,
}

struct Snapshot {
    token: String,
    path: PathBuf,
    _directory: SnapshotDirectory,
    generation: u64,
    window: String,
    owner: String,
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
    pi.require_owner(&owner_id)?;
    validate_identifier(&request_id)?;
    validate_path(&path)?;
    let remote = storage::remote_project(&project_path)?;
    let window_label = window.label().to_string();
    let generation = {
        let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
        manager.generation = manager.generation.checked_add(1).ok_or_else(unavailable)?;
        manager.staged.take();
        invalidate_queued(&mut manager);
        stop_active_transfer(&mut manager);
        manager.generation
    };
    let state = previews.inner.clone();
    let transfer_state = state.clone();
    let transfer_gate = previews.transfer_gate.clone();
    let transfer_request = request_id.clone();
    let transfer_result = tauri::async_runtime::spawn_blocking(move || {
        let _gate = transfer_gate.lock().map_err(|_| unavailable())?;
        transfer_remote_path(
            &transfer_state,
            &remote.connection_string,
            &remote.working_directory,
            &path,
            generation,
            window_label,
            owner_id,
            transfer_request,
        )
    })
    .await
    .map_err(|_| "The remote file preview could not be prepared.".to_string())?;

    let mut manager = state.lock().map_err(|_| unavailable())?;
    if manager
        .active
        .as_ref()
        .is_some_and(|active| active.generation == generation)
    {
        manager.active.take();
    }
    if manager.generation != generation {
        return Err("The remote file preview was superseded.".into());
    }
    match transfer_result? {
        TransferResult::Directory => Ok(PreparedRemotePath::Directory),
        TransferResult::File(snapshot) => {
            let token = snapshot.token.clone();
            manager.staged = Some(snapshot);
            schedule_token_expiry(state.clone(), generation, token.clone());
            Ok(PreparedRemotePath::File { token })
        }
    }
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
    pi.require_owner(&owner_id)?;
    let permit = Arc::new(AtomicBool::new(true));
    {
        let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
        let valid = manager.staged.as_ref().is_some_and(|snapshot| {
            snapshot.token == token
                && snapshot.window == window.label()
                && snapshot.owner == owner_id
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
    }

    #[cfg(target_os = "macos")]
    {
        let state = previews.inner.clone();
        let queued_token = token.clone();
        let queued_request = request_id.clone();
        let closure_permit = permit.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        window
            .run_on_main_thread(move || {
                if !closure_permit.load(Ordering::Acquire) {
                    let _ = sender.send(Err("The remote file preview was superseded.".into()));
                    return;
                }
                let result = (|| {
                    let mut manager = state.lock().map_err(|_| unavailable())?;
                    let queued = manager.queued.as_ref().filter(|queued| {
                        queued.snapshot.token == queued_token
                            && queued.snapshot.request_id == queued_request
                            && queued.permit.load(Ordering::Acquire)
                            && queued.snapshot.generation == manager.generation
                    });
                    let queued = queued.ok_or_else(|| {
                        "The remote file preview is no longer available.".to_string()
                    })?;
                    macos::show(&queued.snapshot.request_id, &queued.snapshot.path)?;
                    let displayed = manager.queued.take().expect("validated queued snapshot");
                    manager.displayed = Some(displayed.snapshot);
                    Ok(())
                })();
                let _ = sender.send(result);
            })
            .map_err(|_| "Quick Look could not be opened.".to_string())?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => {
                result?;
                monitor_displayed_snapshot(
                    previews.inner.clone(),
                    window.clone(),
                    request_id.clone(),
                );
            }
            Err(_) => {
                permit.store(false, Ordering::Release);
                remove_queued(&previews.inner, &request_id);
                return Err("Quick Look did not respond.".into());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        permit.store(false, Ordering::Release);
        remove_queued(&previews.inner, &request_id);
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
    pi.require_owner(&owner_id)?;
    let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
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
            let _ = window.run_on_main_thread(move || {
                macos::close_if(&request_id);
                drop(snapshot);
            });
        }
        #[cfg(not(target_os = "macos"))]
        drop(snapshot);
    }
    Ok(())
}

impl RemotePreviewState {
    pub fn replace_owner(&self, window: &WebviewWindow, owner_id: &str) {
        let displayed = if let Ok(mut manager) = self.inner.lock() {
            let owns_work = manager
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
            manager
                .displayed
                .as_ref()
                .filter(|snapshot| snapshot.owner != owner_id)
                .map(|snapshot| snapshot.request_id.clone())
                .and_then(|request_id| {
                    manager
                        .displayed
                        .take()
                        .map(|snapshot| (request_id, snapshot))
                })
        } else {
            None
        };
        if let Some((request_id, snapshot)) = displayed {
            #[cfg(target_os = "macos")]
            {
                let _ = window.run_on_main_thread(move || {
                    macos::close_if(&request_id);
                    drop(snapshot);
                });
            }
            #[cfg(not(target_os = "macos"))]
            drop((request_id, snapshot));
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
    request_id: String,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let checked_request = request_id.clone();
        if window
            .run_on_main_thread(move || {
                let visible = macos::is_visible(&checked_request);
                if !visible {
                    macos::close_if(&checked_request);
                }
                let _ = sender.send(visible);
            })
            .is_err()
        {
            break;
        }
        if receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(mut manager) = state.lock() {
            if manager
                .displayed
                .as_ref()
                .is_some_and(|snapshot| snapshot.request_id == request_id)
            {
                manager.displayed.take();
            }
        }
        break;
    });
}

fn remove_queued(state: &Arc<Mutex<PreviewManager>>, request_id: &str) {
    if let Ok(mut manager) = state.lock() {
        if manager
            .queued
            .as_ref()
            .is_some_and(|queued| queued.snapshot.request_id == request_id)
        {
            invalidate_queued(&mut manager);
        }
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
        kill_process_group(active.process_id);
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
    request_id: String,
) -> Result<TransferResult, String> {
    let marker = format!("TAU_PREVIEW_{}", Uuid::new_v4().simple());
    let script = r#"p=$1
case "$p" in "~/"*) p=$HOME/${p#\~/} ;; /*) ;; *) p=$2/$p ;; esac
if [ -d "$p" ]; then printf '\000%s\000D\000' "$3"; exit 0; fi
if [ ! -f "$p" ]; then exit 44; fi
size=$(wc -c < "$p") || exit 45
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
    let stdout = child.stdout.take().ok_or_else(unavailable)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let process_id = child.id();
    {
        let mut manager = coordinator.lock().map_err(|_| unavailable())?;
        if manager.generation != generation {
            kill_process_group(process_id);
            let _ = child.wait();
            return Err("The remote file preview was superseded.".into());
        }
        manager.active = Some(ActiveTransfer {
            generation,
            owner: owner.clone(),
            request_id: request_id.clone(),
            cancel: cancel.clone(),
            process_id,
        });
    }
    let started = Instant::now();
    let finished = Arc::new(AtomicBool::new(false));
    start_watchdog(process_id, cancel.clone(), finished.clone(), started);
    let mut process = TransferProcess {
        child,
        stdout,
        process_id,
        finished,
    };

    let prefix = format!("\0{marker}\0").into_bytes();
    find_prefix(&mut process.stdout, &prefix, started)?;
    check_transfer(&cancel, started)?;
    let kind = read_field(&mut process.stdout, 16, started)?;
    if kind == b"D" {
        let status = process.wait(&cancel, started)?;
        return if status.success() {
            Ok(TransferResult::Directory)
        } else {
            Err("The remote directory could not be inspected.".into())
        };
    }
    if kind != b"F" {
        return Err("The remote file response was invalid.".into());
    }
    let size_text = read_field(&mut process.stdout, 32, started)?;
    let size = std::str::from_utf8(&size_text)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "The remote file size was invalid.".to_string())?;
    if size > MAX_FILE_BYTES {
        return Err("The remote file is larger than 64 MiB.".into());
    }
    let basename = String::from_utf8(read_field(&mut process.stdout, 1024, started)?)
        .map_err(|_| "The remote filename is unsupported.".to_string())?;
    if basename.is_empty()
        || basename == "."
        || basename == ".."
        || basename.contains(['/', '\\'])
        || basename.chars().any(char::is_control)
    {
        return Err("The remote filename is unsupported.".into());
    }
    let directory = SnapshotDirectory::create()?;
    let output_path = directory.path().join(basename);
    write_payload(&mut process.stdout, &output_path, size, started)?;
    let expected_trailer = format!("\0{marker}\0END\0").into_bytes();
    let mut trailer = vec![0; expected_trailer.len()];
    read_exact_deadline(&mut process.stdout, &mut trailer, started)?;
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
        request_id,
        created: Instant::now(),
    }))
}

struct TransferProcess {
    child: Child,
    stdout: std::process::ChildStdout,
    process_id: u32,
    finished: Arc<AtomicBool>,
}

impl TransferProcess {
    fn wait(
        &mut self,
        cancel: &AtomicBool,
        started: Instant,
    ) -> Result<std::process::ExitStatus, String> {
        loop {
            if let Some(status) = self.child.try_wait().map_err(|_| unavailable())? {
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
        self.finished.store(true, Ordering::Release);
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                kill_process_group(self.process_id);
                let _ = self.child.wait();
            }
        }
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

fn check_transfer(cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("The remote file preview was superseded.".into());
    }
    if started.elapsed() >= PREPARATION_TIMEOUT {
        return Err("The remote file preview timed out.".into());
    }
    Ok(())
}

fn find_prefix(reader: &mut impl Read, prefix: &[u8], started: Instant) -> Result<(), String> {
    let mut matched = 0;
    for _ in 0..MAX_PREAMBLE_BYTES {
        let mut byte = [0];
        read_exact_deadline(reader, &mut byte, started)?;
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

fn read_field(reader: &mut impl Read, max: usize, started: Instant) -> Result<Vec<u8>, String> {
    let mut value = Vec::new();
    while value.len() <= max {
        let mut byte = [0];
        read_exact_deadline(reader, &mut byte, started)?;
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
) -> Result<(), String> {
    if started.elapsed() >= PREPARATION_TIMEOUT {
        return Err("The remote file preview timed out.".into());
    }
    reader
        .read_exact(output)
        .map_err(|_| "The remote file transfer was incomplete.".into())
}

fn write_payload(
    reader: &mut impl Read,
    path: &Path,
    size: u64,
    started: Instant,
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
        read_exact_deadline(reader, &mut buffer[..count], started)?;
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

    #[test]
    fn rejects_control_characters_and_empty_paths() {
        assert!(validate_path("").is_err());
        assert!(validate_path("safe/file.txt").is_ok());
        assert!(validate_path("bad\nfile").is_err());
    }

    #[test]
    fn finds_marker_after_a_banner_without_consuming_payload() {
        let mut input = &b"banner\r\n\0TOKEN\0F\0"[..];
        find_prefix(&mut input, b"\0TOKEN\0", Instant::now()).expect("marker");
        assert_eq!(read_field(&mut input, 16, Instant::now()).unwrap(), b"F");
    }

    #[test]
    fn writes_binary_payload_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("binary");
        let bytes = b"\0TOKEN\0END\0\xff";
        write_payload(&mut &bytes[..], &path, bytes.len() as u64, Instant::now()).unwrap();
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
        transfer_remote_path(
            coordinator,
            connection,
            cwd,
            path,
            generation,
            "main".into(),
            "owner".into(),
            format!("request-{generation}"),
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
