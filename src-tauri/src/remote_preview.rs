use crate::{
    ssh::SshConnection,
    storage,
    telemetry::{trace_context::TraceContext, Telemetry},
};
use serde::Serialize;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{State, WebviewWindow};
use tempfile::TempDir;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct RemotePreviewState(Arc<Mutex<()>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewResult {
    Opened,
    Directory,
    Unsupported,
    Busy,
}

struct Snapshot {
    path: PathBuf,
    _directory: TempDir,
}

#[tauri::command]
pub async fn preview_remote_path(
    window: WebviewWindow,
    previews: State<'_, RemotePreviewState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: String,
    path: String,
) -> Result<PreviewResult, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "preview_remote_path"));
    if !cfg!(target_os = "macos") {
        return Ok(PreviewResult::Unsupported);
    }
    let remote = storage::remote_project(&project_path)?;
    let gate = previews.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // No queue or replacement: another click can retry when this request finishes.
        let Ok(_guard) = gate.try_lock() else {
            return Ok(PreviewResult::Busy);
        };
        let Some(snapshot) = download(&remote.connection_string, &remote.working_directory, &path)?
        else {
            return Ok(PreviewResult::Directory);
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = sender.send(show(snapshot));
            })
            .map_err(|_| unavailable())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| unavailable())??;
        Ok(PreviewResult::Opened)
    })
    .await
    .map_err(|_| unavailable())?
}

fn unavailable() -> String {
    "Could not preview file. Check the connection and path, then try again.".into()
}

fn download(connection: &str, cwd: &str, path: &str) -> Result<Option<Snapshot>, String> {
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(unavailable());
    }
    let mut directory = tempfile::Builder::new();
    directory.prefix("tau-preview-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.permissions(fs::Permissions::from_mode(0o700));
    }
    let directory = directory.tempdir().map_err(|_| unavailable())?;
    let output_path = directory.path().join(
        Path::new(path)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("preview")),
    );
    let output = File::create(&output_path).map_err(|_| unavailable())?;
    let script = r#"p=$1
case "$p" in "~/"*) p=$HOME/${p#\~/} ;; /*) ;; *) p=$2/$p ;; esac
[ -d "$p" ] && exit 3
[ -f "$p" ] || exit 1
exec head -c "$3" -- "$p""#;
    let remote_command = format!(
        "exec /bin/sh -c {} tau {} {} {}",
        shell_words::quote(script),
        shell_words::quote(path),
        shell_words::quote(cwd),
        MAX_FILE_BYTES + 1,
    );
    let mut command = SshConnection::parse(connection)?.command(&remote_command);
    command
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::null());
    let status = run_transfer(&mut command, TRANSFER_TIMEOUT)?;
    if status.code() == Some(3) {
        return Ok(None);
    }
    if !status.success() {
        return Err(unavailable());
    }
    let metadata = fs::metadata(&output_path).map_err(|_| unavailable())?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    let mut permissions = metadata.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&output_path, permissions).map_err(|_| unavailable())?;
    Ok(Some(Snapshot {
        path: output_path,
        _directory: directory,
    }))
}

fn run_transfer(command: &mut Command, timeout: Duration) -> Result<ExitStatus, String> {
    let mut child = command.spawn().map_err(|_| unavailable())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unavailable());
            }
        }
    }
}

#[cfg(target_os = "macos")]
use macos::show;

#[cfg(not(target_os = "macos"))]
fn show(_snapshot: Snapshot) -> Result<(), String> {
    Err(unavailable())
}

pub fn close() {
    #[cfg(target_os = "macos")]
    macos::close();
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{unavailable, Snapshot};
    use objc2::rc::Retained;
    use objc2_app_kit::{NSBackingStoreType, NSPanel, NSWindowStyleMask};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString, NSURL};
    use objc2_quick_look_ui::{QLPreviewItem, QLPreviewView, QLPreviewViewStyle};
    use std::cell::RefCell;

    struct PreviewPanel {
        panel: Retained<NSPanel>,
        _view: Retained<QLPreviewView>,
        _url: Retained<NSURL>,
        // Keep the snapshot until the next preview or app exit, even after closing the panel.
        _snapshot: Snapshot,
    }
    thread_local! { static PANEL: RefCell<Option<PreviewPanel>> = const { RefCell::new(None) }; }

    pub fn show(snapshot: Snapshot) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or_else(unavailable)?;
        let path = snapshot.path.to_str().ok_or_else(unavailable)?;
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
                    .ok_or_else(unavailable)?;
            view.setAutostarts(false);
            let url = NSURL::fileURLWithPath(&NSString::from_str(path));
            let item = objc2::runtime::ProtocolObject::<dyn QLPreviewItem>::from_ref(&*url);
            view.setPreviewItem(Some(item));
            panel.setContentView(Some(&view));
            panel.center();
            close();
            panel.makeKeyAndOrderFront(None);
            PANEL.with(|slot| {
                *slot.borrow_mut() = Some(PreviewPanel {
                    panel,
                    _view: view,
                    _url: url,
                    _snapshot: snapshot,
                });
            });
        }
        Ok(())
    }

    pub fn close() {
        PANEL.with(|slot| {
            if let Some(panel) = slot.borrow_mut().take() {
                panel.panel.close();
            }
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_ssh() -> (TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ssh");
        fs::write(
            &executable,
            "#!/bin/sh\nfor last do :; done\nexec /bin/sh -c \"$last\"\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let connection = format!("{} fixture", executable.display());
        (directory, connection)
    }

    #[test]
    fn downloads_private_read_only_snapshots_and_removes_them_on_drop() {
        let remote = tempfile::tempdir().unwrap();
        let bytes = b"binary\0payload\xff";
        let name = "quoted ' file.bin";
        fs::write(remote.path().join(name), bytes).unwrap();
        let (_ssh, connection) = fake_ssh();
        for path in [
            name.to_string(),
            remote.path().join(name).display().to_string(),
        ] {
            let snapshot = download(&connection, remote.path().to_str().unwrap(), &path)
                .unwrap()
                .unwrap();
            assert_eq!(fs::read(&snapshot.path).unwrap(), bytes);
            assert!(fs::metadata(&snapshot.path)
                .unwrap()
                .permissions()
                .readonly());
            let directory = snapshot._directory.path().to_path_buf();
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            drop(snapshot);
            assert!(!directory.exists());
        }
    }

    #[test]
    fn directories_copy_and_unreadable_files_fail() {
        let remote = tempfile::tempdir().unwrap();
        fs::create_dir(remote.path().join("folder")).unwrap();
        let (_ssh, connection) = fake_ssh();
        let cwd = remote.path().to_str().unwrap();
        for path in ["folder", ".", "/"] {
            assert!(download(&connection, cwd, path).unwrap().is_none());
        }
        assert!(download(&connection, cwd, "missing").is_err());
        assert!(download(&connection, cwd, "bad\npath").is_err());
    }

    #[test]
    fn rejects_oversized_files() {
        let remote = tempfile::tempdir().unwrap();
        File::create(remote.path().join("large.bin"))
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let (_ssh, connection) = fake_ssh();
        assert!(download(&connection, remote.path().to_str().unwrap(), "large.bin").is_err());
    }

    #[test]
    fn stops_a_stalled_transfer() {
        let mut command = Command::new("sleep");
        command.arg("60");
        let started = Instant::now();
        assert!(run_transfer(&mut command, Duration::from_millis(50)).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
