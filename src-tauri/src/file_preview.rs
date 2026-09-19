use crate::{
    ssh::SshConnection,
    storage,
    telemetry::{trace_context::TraceContext, Telemetry},
};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, State};
use tempfile::TempDir;
use uuid::Uuid;

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct FilePreviewState {
    preparation: Arc<Mutex<()>>,
    active: Arc<Mutex<Option<Snapshot>>>,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FilePreviewResult {
    Ready {
        id: String,
        asset_path: String,
        filename: String,
        source_path: String,
        byte_length: u64,
    },
    Directory,
    Busy,
}

struct Snapshot {
    id: String,
    path: PathBuf,
    filename: String,
    source_path: String,
    byte_length: u64,
    _directory: TempDir,
}

enum PreparedPath {
    Snapshot(Snapshot),
    Directory,
}

#[tauri::command]
pub async fn prepare_file_preview(
    app: AppHandle,
    previews: State<'_, FilePreviewState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    project_path: Option<String>,
    base_path: Option<String>,
    path: String,
) -> Result<FilePreviewResult, String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "prepare_file_preview"));
    let preparation = previews.preparation.clone();
    let active = previews.active.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Preparations are intentionally not queued: a second click can retry
        // after the current bounded copy or SSH transfer completes.
        let Ok(_guard) = preparation.try_lock() else {
            return Ok(FilePreviewResult::Busy);
        };
        let prepared = if let Some(project_path) = project_path.filter(|path| !path.is_empty()) {
            let remote = storage::remote_project(&project_path).map_err(|_| unavailable())?;
            prepare_remote(&remote.connection_string, &remote.working_directory, &path)?
        } else {
            prepare_local(base_path.as_deref().unwrap_or_default(), &path)?
        };
        let PreparedPath::Snapshot(snapshot) = prepared else {
            return Ok(FilePreviewResult::Directory);
        };

        activate(&app, &active, snapshot)
    })
    .await
    .map_err(|_| unavailable())?
}

#[tauri::command]
pub fn release_file_preview(
    app: AppHandle,
    previews: State<'_, FilePreviewState>,
    telemetry: State<'_, Telemetry>,
    telemetry_context: Option<TraceContext>,
    id: String,
) -> Result<(), String> {
    let _span = telemetry_context
        .as_ref()
        .and_then(|context| telemetry.start_command_span(context, "release_file_preview"));
    let snapshot = take_matching(&previews.active, &id)?;
    if let Some(snapshot) = snapshot {
        app.asset_protocol_scope()
            .forbid_file(&snapshot.path)
            .map_err(|_| unavailable())?;
    }
    Ok(())
}

pub fn cleanup(app: &AppHandle) {
    let Some(previews) = app.try_state::<FilePreviewState>() else {
        return;
    };
    let snapshot = previews
        .active
        .lock()
        .ok()
        .and_then(|mut active| active.take());
    if let Some(snapshot) = snapshot {
        let _ = app.asset_protocol_scope().forbid_file(&snapshot.path);
    }
}

fn activate(
    app: &AppHandle,
    active: &Mutex<Option<Snapshot>>,
    snapshot: Snapshot,
) -> Result<FilePreviewResult, String> {
    let mut current = active.lock().map_err(|_| unavailable())?;
    if let Some(previous) = current.take() {
        app.asset_protocol_scope()
            .forbid_file(&previous.path)
            .map_err(|_| unavailable())?;
    }
    app.asset_protocol_scope()
        .allow_file(&snapshot.path)
        .map_err(|_| unavailable())?;
    let result = FilePreviewResult::Ready {
        id: snapshot.id.clone(),
        asset_path: snapshot.path.to_string_lossy().into_owned(),
        filename: snapshot.filename.clone(),
        source_path: snapshot.source_path.clone(),
        byte_length: snapshot.byte_length,
    };
    *current = Some(snapshot);
    Ok(result)
}

fn take_matching(active: &Mutex<Option<Snapshot>>, id: &str) -> Result<Option<Snapshot>, String> {
    let mut active = active.lock().map_err(|_| unavailable())?;
    if active.as_ref().is_some_and(|snapshot| snapshot.id == id) {
        Ok(active.take())
    } else {
        Ok(None)
    }
}

fn unavailable() -> String {
    "Could not preview file. Check the connection and path, then try again.".into()
}

fn prepare_local(base_path: &str, path: &str) -> Result<PreparedPath, String> {
    validate_path(path)?;
    let source_path = resolve_local_path(base_path, path)?;
    let metadata = fs::metadata(&source_path).map_err(|_| unavailable())?;
    if metadata.is_dir() {
        return Ok(PreparedPath::Directory);
    }
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    let mut source = File::open(&source_path).map_err(|_| unavailable())?;
    let filename = filename(&source_path);
    snapshot_from_reader(
        &filename,
        source_path.to_string_lossy().into_owned(),
        &mut source,
    )
}

fn resolve_local_path(base_path: &str, path: &str) -> Result<PathBuf, String> {
    if let Some(relative) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(relative))
            .ok_or_else(unavailable);
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if base_path.is_empty() || base_path.chars().any(char::is_control) {
        return Err(unavailable());
    }
    Ok(Path::new(base_path).join(path))
}

fn prepare_remote(connection: &str, cwd: &str, path: &str) -> Result<PreparedPath, String> {
    validate_path(path)?;
    let started = Instant::now();
    let resolve = r#"p=$1
case "$p" in "~/"*) p=$HOME/${p#\~/} ;; /*) ;; *) p=$2/$p ;; esac"#;
    let probe_script =
        format!("{resolve}\n[ -d \"$p\" ] && exit 3\n[ -f \"$p\" ] || exit 1\nprintf '%s' \"$p\"");
    let mut probe = remote_command(connection, &probe_script, cwd, path, None)?;
    probe
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (status, resolved_path) = run_transfer_with_output(&mut probe, TRANSFER_TIMEOUT)?;
    if status.code() == Some(3) {
        return Ok(PreparedPath::Directory);
    }
    if !status.success() {
        return Err(unavailable());
    }
    let resolved_path = String::from_utf8(resolved_path).map_err(|_| unavailable())?;
    validate_path(&resolved_path)?;

    let filename = filename(Path::new(&resolved_path));
    let (directory, output_path) = create_snapshot_file(&filename)?;
    let output = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&output_path)
        .map_err(|_| unavailable())?;
    let transfer_script =
        format!("{resolve}\n[ -f \"$p\" ] || exit 1\nexec head -c \"$3\" -- \"$p\"");
    let mut transfer = remote_command(
        connection,
        &transfer_script,
        cwd,
        path,
        Some(MAX_FILE_BYTES + 1),
    )?;
    transfer
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::null());
    let remaining = TRANSFER_TIMEOUT
        .checked_sub(started.elapsed())
        .ok_or_else(unavailable)?;
    if !run_transfer(&mut transfer, remaining)?.success() {
        return Err(unavailable());
    }
    finish_snapshot(directory, output_path, filename, resolved_path)
}

fn remote_command(
    connection: &str,
    script: &str,
    cwd: &str,
    path: &str,
    byte_limit: Option<u64>,
) -> Result<Command, String> {
    let mut command = format!(
        "exec /bin/sh -c {} tau {} {}",
        shell_words::quote(script),
        shell_words::quote(path),
        shell_words::quote(cwd),
    );
    if let Some(byte_limit) = byte_limit {
        command.push(' ');
        command.push_str(&byte_limit.to_string());
    }
    Ok(SshConnection::parse(connection)
        .map_err(|_| unavailable())?
        .command(&command))
}

fn snapshot_from_reader(
    filename: &str,
    source_path: String,
    reader: &mut impl Read,
) -> Result<PreparedPath, String> {
    let (directory, output_path) = create_snapshot_file(filename)?;
    let mut output = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&output_path)
        .map_err(|_| unavailable())?;
    let byte_length =
        io::copy(&mut reader.take(MAX_FILE_BYTES + 1), &mut output).map_err(|_| unavailable())?;
    drop(output);
    if byte_length > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    finish_snapshot(directory, output_path, filename.to_string(), source_path)
}

fn create_snapshot_file(filename: &str) -> Result<(TempDir, PathBuf), String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("tau-preview-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir().map_err(|_| unavailable())?;
    let output_path = directory.path().join(filename);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&output_path).map_err(|_| unavailable())?;
    Ok((directory, output_path))
}

fn finish_snapshot(
    directory: TempDir,
    output_path: PathBuf,
    filename: String,
    source_path: String,
) -> Result<PreparedPath, String> {
    let metadata = fs::metadata(&output_path).map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output_path, fs::Permissions::from_mode(0o400))
            .map_err(|_| unavailable())?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&output_path, permissions).map_err(|_| unavailable())?;
    }
    Ok(PreparedPath::Snapshot(Snapshot {
        id: Uuid::new_v4().to_string(),
        path: output_path,
        filename,
        source_path,
        byte_length: metadata.len(),
        _directory: directory,
    }))
}

fn filename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("preview")
        .to_string()
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.chars().any(char::is_control) {
        Err(unavailable())
    } else {
        Ok(())
    }
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

fn run_transfer_with_output(
    command: &mut Command,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>), String> {
    let mut child = command.spawn().map_err(|_| unavailable())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut output = Vec::new();
                child
                    .stdout
                    .take()
                    .ok_or_else(unavailable)?
                    .read_to_end(&mut output)
                    .map_err(|_| unavailable())?;
                return Ok((status, output));
            }
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

    fn snapshot(prepared: PreparedPath) -> Snapshot {
        match prepared {
            PreparedPath::Snapshot(snapshot) => snapshot,
            PreparedPath::Directory => panic!("expected snapshot"),
        }
    }

    #[test]
    fn prepares_local_relative_and_absolute_files_as_private_read_only_snapshots() {
        let source = tempfile::tempdir().unwrap();
        let bytes = b"binary\0payload\xff";
        let name = "quoted file.bin";
        let source_path = source.path().join(name);
        fs::write(&source_path, bytes).unwrap();

        for path in [name.to_string(), source_path.display().to_string()] {
            let snapshot = snapshot(
                prepare_local(source.path().to_str().unwrap(), &path).expect("local preview"),
            );
            assert_eq!(fs::read(&snapshot.path).unwrap(), bytes);
            assert_eq!(snapshot.filename, name);
            assert_eq!(snapshot.byte_length, bytes.len() as u64);
            assert_eq!(
                fs::metadata(snapshot._directory.path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&snapshot.path).unwrap().permissions().mode() & 0o777,
                0o400
            );
        }
    }

    #[test]
    fn local_directories_and_invalid_or_oversized_files_do_not_snapshot() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("folder")).unwrap();
        assert!(matches!(
            prepare_local(source.path().to_str().unwrap(), "folder").unwrap(),
            PreparedPath::Directory
        ));
        assert!(prepare_local(source.path().to_str().unwrap(), "missing").is_err());
        assert!(prepare_local(source.path().to_str().unwrap(), "bad\npath").is_err());

        File::create(source.path().join("large.bin"))
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(prepare_local(source.path().to_str().unwrap(), "large.bin").is_err());
    }

    #[test]
    fn prepares_fake_ssh_files_and_recognizes_directories() {
        let remote = tempfile::tempdir().unwrap();
        let bytes = b"remote\0payload\xff";
        let name = "quoted ' file.bin";
        fs::write(remote.path().join(name), bytes).unwrap();
        fs::create_dir(remote.path().join("folder")).unwrap();
        let (_ssh, connection) = fake_ssh();
        let cwd = remote.path().to_str().unwrap();

        let snapshot = snapshot(prepare_remote(&connection, cwd, name).expect("remote preview"));
        assert_eq!(fs::read(&snapshot.path).unwrap(), bytes);
        assert_eq!(snapshot.filename, name);
        assert_eq!(
            snapshot.source_path,
            remote.path().join(name).to_string_lossy()
        );
        assert_eq!(snapshot.byte_length, bytes.len() as u64);
        assert_eq!(
            fs::metadata(&snapshot.path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        assert!(matches!(
            prepare_remote(&connection, cwd, "folder").unwrap(),
            PreparedPath::Directory
        ));
    }

    #[test]
    fn fake_ssh_rejects_oversized_and_unreadable_files() {
        let remote = tempfile::tempdir().unwrap();
        File::create(remote.path().join("large.bin"))
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let unreadable = remote.path().join("unreadable.bin");
        fs::write(&unreadable, b"secret").unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        let (_ssh, connection) = fake_ssh();
        let cwd = remote.path().to_str().unwrap();

        assert!(prepare_remote(&connection, cwd, "large.bin").is_err());
        // Root can still read mode-000 fixtures, so only assert this where the
        // process is subject to ordinary Unix file permissions.
        if fs::read(&unreadable).is_err() {
            assert!(prepare_remote(&connection, cwd, "unreadable.bin").is_err());
        }
    }

    #[test]
    fn stale_release_ids_leave_the_active_snapshot_owned() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("file.txt"), b"preview").unwrap();
        let snapshot =
            snapshot(prepare_local(source.path().to_str().unwrap(), "file.txt").expect("snapshot"));
        let id = snapshot.id.clone();
        let active = Mutex::new(Some(snapshot));

        assert!(take_matching(&active, "stale-id").unwrap().is_none());
        assert_eq!(active.lock().unwrap().as_ref().unwrap().id, id);
        assert!(take_matching(&active, &id).unwrap().is_some());
        assert!(active.lock().unwrap().is_none());
    }

    #[test]
    fn serializes_the_tagged_camel_case_contract() {
        let value = serde_json::to_value(FilePreviewResult::Ready {
            id: "opaque".into(),
            asset_path: "/tmp/preview".into(),
            filename: "preview.png".into(),
            source_path: "/work/tau/preview.png".into(),
            byte_length: 42,
        })
        .unwrap();
        assert_eq!(value["kind"], "ready");
        assert_eq!(value["assetPath"], "/tmp/preview");
        assert_eq!(value["sourcePath"], "/work/tau/preview.png");
        assert_eq!(value["byteLength"], 42);
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
