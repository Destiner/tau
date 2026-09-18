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
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{State, WebviewWindow};
use tempfile::TempDir;
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PREAMBLE_BYTES: usize = 64 * 1024;
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct RemotePreviewState {
    inner: Arc<Mutex<PreviewManager>>,
}

#[derive(Default)]
struct PreviewManager {
    generation: u64,
    staged: Option<Snapshot>,
    displayed: Option<Snapshot>,
    active: Option<(u64, Arc<Mutex<Child>>)>,
}

struct Snapshot {
    token: String,
    path: PathBuf,
    _directory: TempDir,
    generation: u64,
    window: String,
    owner: String,
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
#[allow(clippy::too_many_arguments)] // Tauri injects the window and managed services at this boundary.
pub async fn prepare_remote_path(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    project_path: String,
    path: String,
) -> Result<PreparedRemotePath, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "prepare_remote_path"));
    pi.require_owner(&owner_id)?;
    validate_path(&path)?;
    let remote = storage::remote_project(&project_path)?;
    let window_label = window.label().to_string();
    let generation = {
        let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
        manager.generation = manager.generation.checked_add(1).ok_or_else(unavailable)?;
        manager.staged.take();
        stop_active_transfer(&mut manager);
        manager.generation
    };
    let state = previews.inner.clone();
    let transfer_state = state.clone();
    let transfer_result = tauri::async_runtime::spawn_blocking(move || {
        transfer_remote_path(
            &transfer_state,
            &remote.connection_string,
            &remote.working_directory,
            &path,
            generation,
            window_label,
            owner_id,
        )
    })
    .await
    .map_err(|_| "The remote file preview could not be prepared.".to_string())?;

    let mut manager = state.lock().map_err(|_| unavailable())?;
    if manager
        .active
        .as_ref()
        .is_some_and(|(active_generation, _)| *active_generation == generation)
    {
        manager.active.take();
    }
    if manager.generation != generation {
        return Err("The remote file preview was superseded.".into());
    }
    let result = transfer_result?;
    match result {
        TransferResult::Directory => Ok(PreparedRemotePath::Directory),
        TransferResult::File(snapshot) => {
            let token = snapshot.token.clone();
            manager.staged = Some(snapshot);
            Ok(PreparedRemotePath::File { token })
        }
    }
}

#[tauri::command]
pub async fn show_remote_preview(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    owner_id: String,
    token: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "show_remote_preview"));
    pi.require_owner(&owner_id)?;
    let (path, generation) = {
        let manager = previews.inner.lock().map_err(|_| unavailable())?;
        let snapshot = manager
            .staged
            .as_ref()
            .filter(|snapshot| {
                snapshot.token == token
                    && snapshot.window == window.label()
                    && snapshot.owner == owner_id
            })
            .ok_or_else(|| "The remote file preview is no longer available.".to_string())?;
        (snapshot.path.clone(), snapshot.generation)
    };

    #[cfg(target_os = "macos")]
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = sender.send(macos::show(&path));
            })
            .map_err(|_| "Quick Look could not be opened.".to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Quick Look did not respond.".to_string())??;
    }
    #[cfg(not(target_os = "macos"))]
    return Err("Quick Look is only available on macOS.".into());

    let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
    if manager.generation != generation {
        #[cfg(target_os = "macos")]
        macos::close_on_main(&window);
        return Err("The remote file preview was superseded.".into());
    }
    manager.displayed.take();
    manager.displayed = manager.staged.take();
    Ok(())
}

#[tauri::command]
pub fn cancel_remote_path(
    previews: State<'_, RemotePreviewState>,
    pi: State<'_, PiState>,
    owner_id: String,
) -> Result<(), String> {
    pi.require_owner(&owner_id)?;
    let mut manager = previews.inner.lock().map_err(|_| unavailable())?;
    manager.generation = manager.generation.saturating_add(1);
    stop_active_transfer(&mut manager);
    manager.staged.take();
    Ok(())
}

impl RemotePreviewState {
    pub fn shutdown(&self) {
        #[cfg(target_os = "macos")]
        macos::close();
        if let Ok(mut manager) = self.inner.lock() {
            manager.generation = manager.generation.saturating_add(1);
            stop_active_transfer(&mut manager);
            manager.staged.take();
            manager.displayed.take();
        }
    }
}

fn stop_active_transfer(manager: &mut PreviewManager) {
    if let Some((_, child)) = manager.active.take() {
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
        }
    }
}

fn unavailable() -> String {
    "Remote preview is unavailable. Try again.".into()
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

fn transfer_remote_path(
    coordinator: &Arc<Mutex<PreviewManager>>,
    connection_string: &str,
    working_directory: &str,
    authored_path: &str,
    generation: u64,
    window: String,
    owner: String,
) -> Result<TransferResult, String> {
    let marker = format!("TAU_PREVIEW_{}", Uuid::new_v4().simple());
    let script = r#"p=$1
case "$p" in ~/*) p=$HOME/${p#\~/} ;; /*) ;; *) p=$2/$p ;; esac
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
    let mut child = connection
        .transfer_command(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "The SSH transfer could not be started.".to_string())?;
    let mut stdout = child.stdout.take().ok_or_else(unavailable)?;
    let mut stderr = child.stderr.take().ok_or_else(unavailable)?;
    let child = Arc::new(Mutex::new(child));
    {
        let mut manager = coordinator.lock().map_err(|_| unavailable())?;
        if manager.generation != generation {
            kill_child(&child);
            return Err("The remote file preview was superseded.".into());
        }
        manager.active = Some((generation, child.clone()));
    }
    let finished = Arc::new(AtomicBool::new(false));
    let timeout_child = child.clone();
    let timeout_finished = finished.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PREPARATION_TIMEOUT);
        if !timeout_finished.load(Ordering::Acquire) {
            kill_child(&timeout_child);
        }
    });
    std::thread::spawn(move || {
        let mut sink = [0_u8; 8192];
        let mut total = 0;
        while total < MAX_PREAMBLE_BYTES {
            match stderr.read(&mut sink) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
    });
    let started = Instant::now();
    let prefix = format!("\0{marker}\0").into_bytes();
    find_prefix(&mut stdout, &prefix, started)?;
    let kind = read_field(&mut stdout, 16, started)?;
    if kind == b"D" {
        let status = wait_child(&child)?;
        finished.store(true, Ordering::Release);
        return if status.success() {
            Ok(TransferResult::Directory)
        } else {
            Err("The remote directory could not be inspected.".into())
        };
    }
    if kind != b"F" {
        kill_child(&child);
        return Err("The remote file response was invalid.".into());
    }
    let size_text = read_field(&mut stdout, 32, started)?;
    let size = std::str::from_utf8(&size_text)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "The remote file size was invalid.".to_string())?;
    if size > MAX_FILE_BYTES {
        kill_child(&child);
        return Err("The remote file is larger than 64 MiB.".into());
    }
    let basename = String::from_utf8(read_field(&mut stdout, 1024, started)?)
        .map_err(|_| "The remote filename is unsupported.".to_string())?;
    if basename.is_empty()
        || basename == "."
        || basename == ".."
        || basename.contains(['/', '\\'])
        || basename.chars().any(char::is_control)
    {
        kill_child(&child);
        return Err("The remote filename is unsupported.".into());
    }
    let directory = tempfile::Builder::new()
        .prefix("tau-preview-")
        .tempdir()
        .map_err(|_| unavailable())?;
    let output_path = directory.path().join(basename);
    write_payload(&mut stdout, &output_path, size, started)?;
    let expected_trailer = format!("\0{marker}\0END\0").into_bytes();
    let mut trailer = vec![0; expected_trailer.len()];
    read_exact_deadline(&mut stdout, &mut trailer, started)?;
    if trailer != expected_trailer {
        kill_child(&child);
        return Err("The remote file transfer was incomplete.".into());
    }
    let status = wait_child(&child)?;
    finished.store(true, Ordering::Release);
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
    }))
}

fn kill_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
    }
}

fn wait_child(child: &Arc<Mutex<Child>>) -> Result<std::process::ExitStatus, String> {
    child
        .lock()
        .map_err(|_| unavailable())?
        .wait()
        .map_err(|_| unavailable())
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
    use tauri::WebviewWindow;

    struct PreviewPanel {
        panel: objc2::rc::Retained<NSPanel>,
        _view: objc2::rc::Retained<QLPreviewView>,
        _url: objc2::rc::Retained<NSURL>,
    }
    thread_local! { static PANEL: RefCell<Option<PreviewPanel>> = const { RefCell::new(None) }; }

    pub fn show(path: &Path) -> Result<(), String> {
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
                    panel,
                    _view: view,
                    _url: url,
                })
            });
        }
        Ok(())
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

    pub fn close_on_main(window: &WebviewWindow) {
        let _ = window.run_on_main_thread(close);
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
    fn fake_ssh(script: &str) -> (TempDir, String) {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ssh");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let connection = format!("{} fixture", executable.display());
        (directory, connection)
    }

    #[cfg(unix)]
    #[test]
    fn transfers_binary_files_and_classifies_directories_through_fake_ssh() {
        let remote = tempfile::tempdir().unwrap();
        let bytes = b"binary\0payload\xff\0TAU_PREVIEW_marker\0END\0";
        fs::write(remote.path().join("quoted name.bin"), bytes).unwrap();
        fs::create_dir(remote.path().join("folder")).unwrap();

        let (_ssh_dir, connection) =
            fake_ssh("#!/bin/sh\nfor last do :; done\nexec /bin/sh -c \"$last\"\n");

        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));
        let result = transfer_remote_path(
            &coordinator,
            &connection,
            remote.path().to_str().unwrap(),
            "quoted name.bin",
            1,
            "main".into(),
            "owner".into(),
        )
        .expect("file transfer");
        let TransferResult::File(snapshot) = result else {
            panic!("expected a file");
        };
        assert_eq!(fs::read(&snapshot.path).unwrap(), bytes);
        assert!(fs::metadata(&snapshot.path)
            .unwrap()
            .permissions()
            .readonly());

        coordinator.lock().unwrap().generation = 2;
        assert!(matches!(
            transfer_remote_path(
                &coordinator,
                &connection,
                remote.path().to_str().unwrap(),
                "folder",
                2,
                "main".into(),
                "owner".into(),
            )
            .expect("directory classification"),
            TransferResult::Directory
        ));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_a_blocked_ssh_transfer() {
        let (_ssh_dir, connection) = fake_ssh("#!/bin/sh\nsleep 60\n");
        let coordinator = Arc::new(Mutex::new(PreviewManager {
            generation: 1,
            ..Default::default()
        }));
        let transfer_coordinator = coordinator.clone();
        let transfer = std::thread::spawn(move || {
            transfer_remote_path(
                &transfer_coordinator,
                &connection,
                "/tmp",
                "missing",
                1,
                "main".into(),
                "owner".into(),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let active = coordinator.lock().unwrap().active.is_some();
            if active {
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
}
