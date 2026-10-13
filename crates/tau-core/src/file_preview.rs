//! Blocking, private snapshots for native preview owners. Never run on the UI thread.
use crate::{ssh::SshConnection, storage};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tempfile::TempDir;

pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const TEXT_PREVIEW_LIMIT: u64 = 512 * 1024;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

/// Dropping the final owner releases the snapshot; stale results need no global release ID.
pub struct Snapshot {
    pub path: PathBuf,
    pub filename: String,
    pub source_path: String,
    pub byte_length: u64,
    _directory: TempDir,
}
pub enum PreparedPreview {
    Ready(Arc<Snapshot>),
    Directory,
    Busy,
}
#[derive(Clone, Default)]
pub struct PreviewService {
    preparation: Arc<Mutex<()>>,
}
fn unavailable() -> String {
    "Could not preview file. Check the connection and path, then try again.".into()
}
fn validate(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 32 * 1024 || path.chars().any(char::is_control) {
        Err(unavailable())
    } else {
        Ok(())
    }
}
impl PreviewService {
    /// Remote identity is the registered project path, never an unchecked SSH command.
    pub fn prepare(
        &self,
        project: Option<&str>,
        base: &str,
        path: &str,
    ) -> Result<PreparedPreview, String> {
        let Ok(_guard) = self.preparation.try_lock() else {
            return Ok(PreparedPreview::Busy);
        };
        validate(path)?;
        if let Some(project) = project.filter(|s| !s.is_empty()) {
            let remote = storage::remote_project(project).map_err(|_| unavailable())?;
            prepare_remote(&remote.connection_string, &remote.working_directory, path)
        } else {
            prepare_local(base, path)
        }
    }
}
pub fn resolve_local_path(base: &str, path: &str) -> Result<PathBuf, String> {
    validate(path)?;
    if let Some(relative) = path.strip_prefix("~/") {
        Ok(dirs::home_dir().ok_or_else(unavailable)?.join(relative))
    } else if Path::new(path).is_absolute() {
        Ok(PathBuf::from(path))
    } else {
        validate(base)?;
        Ok(Path::new(base).join(path))
    }
}
fn prepare_local(base: &str, path: &str) -> Result<PreparedPreview, String> {
    let source = resolve_local_path(base, path)?;
    let metadata = fs::metadata(&source).map_err(|_| unavailable())?;
    if metadata.is_dir() {
        return Ok(PreparedPreview::Directory);
    }
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    let name = filename(&source);
    let (directory, output_path, mut output) = create_snapshot(&name)?;
    let mut input = File::open(&source).map_err(|_| unavailable())?;
    // Recheck the opened handle rather than trusting the earlier pathname probe.
    let opened = input.metadata().map_err(|_| unavailable())?;
    if !opened.is_file() || opened.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    io::copy(&mut (&mut input).take(MAX_FILE_BYTES + 1), &mut output).map_err(|_| unavailable())?;
    drop(output);
    finish(
        directory,
        output_path,
        name,
        source.to_string_lossy().into_owned(),
    )
}
fn filename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .unwrap_or("preview")
        .into()
}
fn create_snapshot(name: &str) -> Result<(TempDir, PathBuf, File), String> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("tau-preview-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir().map_err(|_| unavailable())?;
    let path = directory.path().join(name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let output = options.open(&path).map_err(|_| unavailable())?;
    Ok((directory, path, output))
}
fn finish(
    directory: TempDir,
    path: PathBuf,
    filename: String,
    source_path: String,
) -> Result<PreparedPreview, String> {
    let metadata = fs::metadata(&path).map_err(|_| unavailable())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(unavailable());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).map_err(|_| unavailable())?;
    }
    Ok(PreparedPreview::Ready(Arc::new(Snapshot {
        path,
        filename,
        source_path,
        byte_length: metadata.len(),
        _directory: directory,
    })))
}
fn remote_command(
    connection: &str,
    script: &str,
    cwd: &str,
    path: &str,
    limit: Option<u64>,
) -> Result<Command, String> {
    let mut command = format!(
        "exec /bin/sh -c {} tau {} {}",
        shell_words::quote(script),
        shell_words::quote(path),
        shell_words::quote(cwd)
    );
    if let Some(limit) = limit {
        command.push_str(&format!(" {limit}"));
    }
    Ok(SshConnection::parse(connection)
        .map_err(|_| unavailable())?
        .command(&command))
}
fn run(command: &mut Command, timeout: Duration) -> Result<(ExitStatus, Vec<u8>), String> {
    let mut child = command.spawn().map_err(|_| unavailable())?;
    let start = Instant::now();
    // Drain the probe concurrently: waiting first can deadlock on a full pipe.
    let reader = child.stdout.take().map(|pipe| {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe
                .take(32 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = sender.send(result);
        });
        receiver
    });
    let mut output = None;
    loop {
        if let Some(Ok(result)) = reader
            .as_ref()
            .filter(|_| output.is_none())
            .map(|reader| reader.try_recv())
        {
            output = result.ok().filter(|bytes| bytes.len() <= 32 * 1024);
            if output.is_none() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unavailable());
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(reader) = reader.as_ref().filter(|_| output.is_none()) {
                    let remaining = timeout
                        .checked_sub(start.elapsed())
                        .ok_or_else(unavailable)?;
                    output = reader
                        .recv_timeout(remaining)
                        .ok()
                        .and_then(Result::ok)
                        .filter(|bytes| bytes.len() <= 32 * 1024);
                    if output.is_none() {
                        return Err(unavailable());
                    }
                }
                return Ok((status, output.unwrap_or_default()));
            }
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unavailable());
            }
        }
    }
}
fn prepare_remote(connection: &str, cwd: &str, path: &str) -> Result<PreparedPreview, String> {
    let started = Instant::now();
    let resolve = "p=$1\ncase \"$p\" in \"~/\"*) p=$HOME/${p#\\~/} ;; /*) ;; *) p=$2/$p ;; esac";
    let probe =
        format!("{resolve}\n[ -d \"$p\" ] && exit 3\n[ -f \"$p\" ] || exit 1\nprintf '%s' \"$p\"");
    let mut command = remote_command(connection, &probe, cwd, path, None)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (status, resolved) = run(&mut command, TRANSFER_TIMEOUT)?;
    if status.code() == Some(3) {
        return Ok(PreparedPreview::Directory);
    }
    if !status.success() {
        return Err(unavailable());
    }
    let source = String::from_utf8(resolved).map_err(|_| unavailable())?;
    validate(&source)?;
    let name = filename(Path::new(&source));
    let (directory, output_path, output) = create_snapshot(&name)?;
    let transfer = format!("{resolve}\n[ -f \"$p\" ] || exit 1\nexec head -c \"$3\" -- \"$p\"");
    let mut command = remote_command(connection, &transfer, cwd, path, Some(MAX_FILE_BYTES + 1))?;
    command
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::null());
    let remaining = TRANSFER_TIMEOUT
        .checked_sub(started.elapsed())
        .ok_or_else(unavailable)?;
    if !run(&mut command, remaining)?.0.success() {
        return Err(unavailable());
    }
    finish(directory, output_path, name, source)
}

pub fn read_text(snapshot: &Snapshot) -> Result<(String, bool), String> {
    if snapshot.byte_length == 0 {
        return Ok((String::new(), false));
    }
    let mut bytes = Vec::new();
    File::open(&snapshot.path)
        .map_err(|_| unavailable())?
        .take(TEXT_PREVIEW_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    let truncated = snapshot.byte_length > bytes.len() as u64;
    // TextDecoder's default UTF-8 mode consumes a leading BOM.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    Ok((String::from_utf8_lossy(bytes).into_owned(), truncated))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn probe_drain_is_bounded_and_cannot_wait_on_a_full_pipe() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "head -c 32768 /dev/zero"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (status, output) = run(&mut command, Duration::from_secs(2)).unwrap();
        assert!(status.success());
        assert_eq!(output.len(), 32768);
        let mut oversized = Command::new("/bin/sh");
        oversized
            .args(["-c", "head -c 32769 /dev/zero"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        assert!(run(&mut oversized, Duration::from_secs(2)).is_err());
    }
    #[test]
    fn shared_inflight_and_sparse_oversize_admission() {
        let source = tempfile::tempdir().unwrap();
        let large = source.path().join("large.txt");
        File::create(&large)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        let service = PreviewService::default();
        let clone = service.clone();
        let guard = service.preparation.lock().unwrap();
        assert!(matches!(
            clone.prepare(None, source.path().to_str().unwrap(), "large.txt"),
            Ok(PreparedPreview::Busy)
        ));
        drop(guard);
        assert!(clone
            .prepare(None, source.path().to_str().unwrap(), "large.txt")
            .is_err());
        assert!(clone
            .prepare(None, "/synthetic", &"x".repeat(32 * 1024 + 1))
            .is_err());
    }
    #[test]
    fn empty_snapshot_private_permissions_and_failed_read_do_not_touch_source() {
        let source = tempfile::tempdir().unwrap();
        let original = source.path().join("empty.txt");
        fs::write(&original, b"").unwrap();
        let PreparedPreview::Ready(snapshot) = PreviewService::default()
            .prepare(None, source.path().to_str().unwrap(), "empty.txt")
            .unwrap()
        else {
            panic!("snapshot")
        };
        assert_eq!(read_text(&snapshot).unwrap(), (String::new(), false));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(snapshot.path.parent().unwrap())
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
        fs::remove_file(&snapshot.path).unwrap();
        assert_eq!(read_text(&snapshot).unwrap(), (String::new(), false));
        drop(snapshot);
        fs::write(source.path().join("read-failure.txt"), b"synthetic").unwrap();
        let PreparedPreview::Ready(failed) = PreviewService::default()
            .prepare(None, source.path().to_str().unwrap(), "read-failure.txt")
            .unwrap()
        else {
            panic!("snapshot")
        };
        fs::remove_file(&failed.path).unwrap();
        assert!(read_text(&failed).is_err());
        drop(failed);
        assert!(original.exists());
        assert_eq!(fs::read(&original).unwrap(), b"");
    }
    #[test]
    fn snapshot_ownership_and_bounded_text() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a.txt"), b"exact\n\n").unwrap();
        let service = PreviewService::default();
        let PreparedPreview::Ready(snapshot) = service
            .prepare(None, source.path().to_str().unwrap(), "a.txt")
            .unwrap()
        else {
            panic!("ready");
        };
        let path = snapshot.path.clone();
        fs::write(source.path().join("a.txt"), b"changed").unwrap();
        assert_eq!(read_text(&snapshot).unwrap(), ("exact\n\n".into(), false));
        let retained = snapshot.clone();
        drop(snapshot);
        assert!(path.exists());
        drop(retained);
        assert!(!path.exists());
        assert!(matches!(
            service
                .prepare(None, "", source.path().to_str().unwrap())
                .unwrap(),
            PreparedPreview::Directory
        ));
        assert!(service.prepare(None, "", "relative").is_err());
        assert!(service.prepare(None, "/tmp", "bad\nname").is_err());
        fs::write(source.path().join("bom.txt"), b"\xef\xbb\xbfexact").unwrap();
        let PreparedPreview::Ready(bom) = service
            .prepare(None, source.path().to_str().unwrap(), "bom.txt")
            .unwrap()
        else {
            panic!("ready");
        };
        assert_eq!(read_text(&bom).unwrap(), ("exact".into(), false));
        let mut bytes = vec![b'a'; TEXT_PREVIEW_LIMIT as usize + 1];
        bytes[TEXT_PREVIEW_LIMIT as usize - 1] = 0xe2;
        fs::write(source.path().join("large.txt"), bytes).unwrap();
        let PreparedPreview::Ready(large) = service
            .prepare(None, source.path().to_str().unwrap(), "large.txt")
            .unwrap()
        else {
            panic!("ready");
        };
        let (text, truncated) = read_text(&large).unwrap();
        assert!(truncated);
        assert!(text.ends_with('\u{fffd}'));
        let _guard = service.preparation.lock().unwrap();
        assert!(matches!(
            service.prepare(None, "/tmp", "a"),
            Ok(PreparedPreview::Busy)
        ));
    }
}
