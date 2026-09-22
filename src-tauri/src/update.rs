use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::quit::QuitState;

pub const CHECK_FOR_UPDATES_MENU_ID: &str = "tau-check-for-updates";
pub const CHECK_FOR_UPDATES_EVENT: &str = "tau://check-for-updates";
pub const UPDATE_PROGRESS_EVENT: &str = "tau://update-progress";
pub const UPDATE_STATUS_EVENT: &str = "tau://update-status";

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const UNKNOWN_TOTAL_PROGRESS_STEP: u64 = 1024 * 1024;
const MAX_PROGRESS_EVENTS: u64 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateErrorCategory {
    Unavailable,
    Unsupported,
    Busy,
    CheckFailed,
    DownloadFailed,
    VerificationFailed,
    PreflightFailed,
    AuthorizationExpired,
    InstallFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    category: UpdateErrorCategory,
    message: &'static str,
}

impl UpdateError {
    fn new(category: UpdateErrorCategory, message: &'static str) -> Self {
        Self { category, message }
    }

    fn unavailable() -> Self {
        Self::new(
            UpdateErrorCategory::Unavailable,
            "Updates are unavailable in this build.",
        )
    }

    fn unsupported() -> Self {
        Self::new(
            UpdateErrorCategory::Unsupported,
            "Updates are available only from an installed macOS app.",
        )
    }

    fn busy() -> Self {
        Self::new(
            UpdateErrorCategory::Busy,
            "Another update operation is already in progress.",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCandidateSnapshot {
    version: String,
    current_version: String,
    notes: Option<String>,
}

impl From<&Update> for UpdateCandidateSnapshot {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Prepared,
    Installing,
    RestartNeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    supported: bool,
    status: UpdateStatus,
    operation_id: Option<u64>,
    manual: bool,
    candidate: Option<UpdateCandidateSnapshot>,
    error: Option<UpdateError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusSnapshot {
    pub supported: bool,
    pub status: UpdateStatus,
    pub operation_id: u64,
    pub manual: bool,
    pub candidate: Option<UpdateCandidateSnapshot>,
    pub error_category: Option<UpdateErrorCategory>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheckStatus {
    Unavailable,
    Current,
    Available,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    status: UpdateCheckStatus,
    version: Option<String>,
    operation_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Checking(u64),
    UpToDate,
    Available,
    Downloading(u64),
    Prepared(u64),
    Installing(u64),
    RestartNeeded(u64),
    Failed(u64),
}

impl Phase {
    fn operation_id(self) -> Option<u64> {
        match self {
            Self::Checking(id)
            | Self::Downloading(id)
            | Self::Prepared(id)
            | Self::Installing(id)
            | Self::RestartNeeded(id)
            | Self::Failed(id) => Some(id),
            Self::Idle | Self::UpToDate | Self::Available => None,
        }
    }

    fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Checking(_) | Self::Downloading(_) | Self::Installing(_)
        )
    }
}

#[derive(Default)]
struct UpdateCoordinator {
    next_operation_id: u64,
    phase: Phase,
    manual: bool,
    candidate: Option<Update>,
    candidate_operation_id: Option<u64>,
    verified_bytes: Option<Vec<u8>>,
    error: Option<UpdateError>,
}

impl UpdateCoordinator {
    fn next_id(&mut self) -> u64 {
        self.next_operation_id = self.next_operation_id.wrapping_add(1).max(1);
        self.next_operation_id
    }

    fn begin_check(&mut self, manual: bool) -> Result<u64, UpdateError> {
        if self.phase.is_busy() {
            return Err(UpdateError::busy());
        }
        let operation_id = self.next_id();
        self.phase = Phase::Checking(operation_id);
        self.manual = manual;
        self.candidate = None;
        self.candidate_operation_id = None;
        self.verified_bytes = None;
        self.error = None;
        Ok(operation_id)
    }

    fn finish_check(&mut self, operation_id: u64, candidate: Option<Update>) {
        if self.phase != Phase::Checking(operation_id) {
            return;
        }
        self.phase = if candidate.is_some() {
            Phase::Available
        } else {
            Phase::UpToDate
        };
        self.candidate_operation_id = candidate.as_ref().map(|_| operation_id);
        self.candidate = candidate;
    }

    fn begin_download(&mut self, operation_id: u64) -> Result<Update, UpdateError> {
        if self.phase.is_busy() {
            return Err(UpdateError::busy());
        }
        if self.candidate_operation_id != Some(operation_id) {
            return Err(UpdateError::new(
                UpdateErrorCategory::AuthorizationExpired,
                "This update request has expired. Try again.",
            ));
        }
        let mut candidate = self.candidate.clone().ok_or_else(|| {
            UpdateError::new(
                UpdateErrorCategory::Unavailable,
                "No update is ready to download.",
            )
        })?;
        candidate.timeout = Some(DOWNLOAD_TIMEOUT);
        self.phase = Phase::Downloading(operation_id);
        self.verified_bytes = None;
        self.error = None;
        Ok(candidate)
    }

    fn finish_download(&mut self, operation_id: u64, bytes: Vec<u8>) {
        if self.phase == Phase::Downloading(operation_id) {
            self.verified_bytes = Some(bytes);
            self.phase = Phase::Prepared(operation_id);
        }
    }

    fn validate_prepared(&self, operation_id: u64) -> Result<(), UpdateError> {
        if self.phase == Phase::Prepared(operation_id)
            && self.candidate.is_some()
            && self.verified_bytes.is_some()
        {
            Ok(())
        } else {
            Err(UpdateError::new(
                UpdateErrorCategory::AuthorizationExpired,
                "This update request has expired. Try again.",
            ))
        }
    }

    fn begin_install(&mut self, operation_id: u64) -> Result<Vec<u8>, UpdateError> {
        self.validate_prepared(operation_id)?;
        let bytes = self.verified_bytes.clone().expect("validated bytes");
        self.phase = Phase::Installing(operation_id);
        self.error = None;
        Ok(bytes)
    }

    fn finish_install(&mut self, operation_id: u64) {
        if self.phase == Phase::Installing(operation_id) {
            self.phase = Phase::RestartNeeded(operation_id);
            self.verified_bytes = None;
            self.error = None;
        }
    }

    fn validate_restart_needed(&self, operation_id: u64) -> Result<(), UpdateError> {
        if self.phase == Phase::RestartNeeded(operation_id) {
            Ok(())
        } else {
            Err(UpdateError::new(
                UpdateErrorCategory::AuthorizationExpired,
                "This update request has expired. Try again.",
            ))
        }
    }

    fn fail(&mut self, operation_id: u64, error: UpdateError) {
        self.phase = Phase::Failed(operation_id);
        self.verified_bytes = None;
        self.error = Some(error);
    }

    fn fail_install(&mut self, operation_id: u64, error: UpdateError) {
        if self.phase == Phase::Installing(operation_id) {
            self.phase = Phase::Prepared(operation_id);
            self.error = Some(error);
        }
    }

    fn snapshot(&self, supported: bool) -> UpdateSnapshot {
        let status = match self.phase {
            Phase::Idle => UpdateStatus::Idle,
            Phase::Checking(_) => UpdateStatus::Checking,
            Phase::UpToDate => UpdateStatus::UpToDate,
            Phase::Available => UpdateStatus::Available,
            Phase::Downloading(_) => UpdateStatus::Downloading,
            Phase::Prepared(_) => UpdateStatus::Prepared,
            Phase::Installing(_) => UpdateStatus::Installing,
            Phase::RestartNeeded(_) => UpdateStatus::RestartNeeded,
            Phase::Failed(_) => UpdateStatus::Failed,
        };
        UpdateSnapshot {
            supported,
            status,
            operation_id: self.phase.operation_id().or(self.candidate_operation_id),
            manual: self.manual,
            candidate: self.candidate.as_ref().map(Into::into),
            error: self.error.clone(),
        }
    }

    fn status_snapshot(&self, operation_id: u64) -> UpdateStatusSnapshot {
        let status = match self.phase {
            Phase::Idle => UpdateStatus::Idle,
            Phase::Checking(_) => UpdateStatus::Checking,
            Phase::UpToDate => UpdateStatus::UpToDate,
            Phase::Available => UpdateStatus::Available,
            Phase::Downloading(_) => UpdateStatus::Downloading,
            Phase::Prepared(_) => UpdateStatus::Prepared,
            Phase::Installing(_) => UpdateStatus::Installing,
            Phase::RestartNeeded(_) => UpdateStatus::RestartNeeded,
            Phase::Failed(_) => UpdateStatus::Failed,
        };
        UpdateStatusSnapshot {
            supported: true,
            status,
            operation_id,
            manual: self.manual,
            candidate: self.candidate.as_ref().map(Into::into),
            error_category: self.error.as_ref().map(|error| error.category),
        }
    }
}

#[derive(Default)]
pub struct UpdateState(Mutex<UpdateCoordinator>);

impl UpdateState {
    fn with_coordinator<T>(&self, callback: impl FnOnce(&mut UpdateCoordinator) -> T) -> T {
        let mut coordinator = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        callback(&mut coordinator)
    }
}

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    let builder = tauri_plugin_updater::Builder::new();
    match updater_public_key() {
        Some(key) => builder.pubkey(key).build(),
        None => builder.build(),
    }
}

fn updater_public_key() -> Option<&'static str> {
    option_env!("TAU_UPDATER_PUBLIC_KEY")
        .map(str::trim)
        .filter(|key| !key.is_empty())
}

#[tauri::command]
pub fn update_snapshot(state: State<'_, UpdateState>) -> UpdateSnapshot {
    let availability = ensure_available();
    state.with_coordinator(|coordinator| {
        let mut snapshot = coordinator.snapshot(availability.is_ok());
        if let Err(error) = availability {
            snapshot.error = Some(error);
        }
        snapshot
    })
}

#[tauri::command]
pub async fn check_for_update<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
    manual: Option<bool>,
) -> Result<UpdateCheckResult, UpdateError> {
    if let Err(error) = ensure_available() {
        if matches!(
            error.category,
            UpdateErrorCategory::Unavailable | UpdateErrorCategory::Unsupported
        ) {
            return Ok(UpdateCheckResult {
                status: UpdateCheckStatus::Unavailable,
                version: None,
                operation_id: None,
            });
        }
        return Err(error);
    }
    let operation_id =
        state.with_coordinator(|coordinator| coordinator.begin_check(manual.unwrap_or(false)))?;

    let result = async {
        let updater = app
            .updater_builder()
            .pubkey(updater_public_key().expect("availability checked"))
            .timeout(CHECK_TIMEOUT)
            .build()
            .map_err(map_check_error)?;
        tokio::time::timeout(CHECK_TIMEOUT, updater.check())
            .await
            .map_err(|_| check_failed())?
            .map_err(map_check_error)
    }
    .await;

    match result {
        Ok(candidate) => {
            let version = candidate.as_ref().map(|update| update.version.clone());
            let status = if candidate.is_some() {
                UpdateCheckStatus::Available
            } else {
                UpdateCheckStatus::Current
            };
            let status_snapshot = state.with_coordinator(|coordinator| {
                coordinator.finish_check(operation_id, candidate);
                coordinator.status_snapshot(operation_id)
            });
            emit_status(&app, status_snapshot);
            Ok(UpdateCheckResult {
                status,
                version,
                operation_id: (status == UpdateCheckStatus::Available).then_some(operation_id),
            })
        }
        Err(error) => {
            let status = state.with_coordinator(|coordinator| {
                coordinator.fail(operation_id, error.clone());
                coordinator.status_snapshot(operation_id)
            });
            emit_status(&app, status);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn download_update<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
    operation_id: u64,
) -> Result<(), UpdateError> {
    ensure_available()?;
    let update = state.with_coordinator(|coordinator| coordinator.begin_download(operation_id))?;
    if let Err(error) = installed_app_path().and_then(|path| preflight_app_path(&path)) {
        let status = state.with_coordinator(|coordinator| {
            coordinator.fail(operation_id, error.clone());
            coordinator.status_snapshot(operation_id)
        });
        emit_status(&app, status);
        return Err(error);
    }

    let emitted = Arc::new(AtomicU64::new(0));
    let downloaded = Arc::new(AtomicU64::new(0));
    let last_bucket = Arc::new(AtomicU64::new(u64::MAX));
    emit_progress(
        &app,
        operation_id,
        0,
        None,
        UpdateProgressPhase::Downloading,
    );

    let progress_app = app.clone();
    let progress_emitted = Arc::clone(&emitted);
    let progress_downloaded = Arc::clone(&downloaded);
    let progress_bucket = Arc::clone(&last_bucket);
    let finish_app = app.clone();
    let finish_downloaded = Arc::clone(&downloaded);

    let result = match tokio::time::timeout(
        DOWNLOAD_TIMEOUT,
        update.download(
            move |chunk, total| {
                let current =
                    progress_downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                let bucket = total
                    .filter(|total| *total > 0)
                    .map(|total| current.saturating_mul(100) / total)
                    .unwrap_or(current / UNKNOWN_TOTAL_PROGRESS_STEP)
                    .min(99);
                let previous = progress_bucket.swap(bucket, Ordering::Relaxed);
                if bucket != previous
                    && progress_emitted.fetch_add(1, Ordering::Relaxed) < MAX_PROGRESS_EVENTS
                {
                    emit_progress(
                        &progress_app,
                        operation_id,
                        current,
                        total,
                        UpdateProgressPhase::Downloading,
                    );
                }
            },
            move || {
                emit_progress(
                    &finish_app,
                    operation_id,
                    finish_downloaded.load(Ordering::Relaxed),
                    None,
                    UpdateProgressPhase::Verifying,
                );
            },
        ),
    )
    .await
    {
        Ok(result) => result.map_err(map_download_error),
        Err(_) => Err(download_failed()),
    };

    match result {
        Ok(bytes) => {
            let status = state.with_coordinator(|coordinator| {
                coordinator.finish_download(operation_id, bytes);
                coordinator.status_snapshot(operation_id)
            });
            emit_status(&app, status);
            Ok(())
        }
        Err(error) => {
            let status = state.with_coordinator(|coordinator| {
                coordinator.fail(operation_id, error.clone());
                coordinator.status_snapshot(operation_id)
            });
            emit_status(&app, status);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn request_update_restart<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
    operation_id: u64,
) -> Result<(), UpdateError> {
    state.with_coordinator(|coordinator| coordinator.validate_prepared(operation_id))?;
    if crate::quit::request_update_restart(&app, operation_id) {
        Ok(())
    } else {
        Err(UpdateError::busy())
    }
}

#[tauri::command]
pub async fn install_update<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
    quit_state: State<'_, QuitState>,
    operation_id: u64,
    request_id: u64,
) -> Result<(), UpdateError> {
    ensure_available()?;
    #[cfg(target_os = "macos")]
    let app_path = installed_app_path()?;
    if !quit_state.consume_update_authorization(request_id, operation_id) {
        return Err(UpdateError::new(
            UpdateErrorCategory::AuthorizationExpired,
            "This update authorization has expired. Try again.",
        ));
    }

    let bytes = match state.with_coordinator(|coordinator| coordinator.begin_install(operation_id))
    {
        Ok(prepared) => prepared,
        Err(error) => {
            quit_state.release_update_install(request_id, operation_id);
            return Err(error);
        }
    };

    #[cfg(target_os = "macos")]
    let install_result =
        tokio::task::spawn_blocking(move || install_macos_update(&bytes, &app_path)).await;
    #[cfg(not(target_os = "macos"))]
    let install_result: Result<Result<(), std::io::Error>, tokio::task::JoinError> =
        unreachable!("updates are only available on macOS");

    if !matches!(install_result, Ok(Ok(()))) {
        let error = UpdateError::new(
            UpdateErrorCategory::InstallFailed,
            "The update could not be installed. Try again.",
        );
        let status = state.with_coordinator(|coordinator| {
            coordinator.fail_install(operation_id, error.clone());
            coordinator.status_snapshot(operation_id)
        });
        quit_state.release_update_install(request_id, operation_id);
        emit_status(&app, status);
        return Err(error);
    }

    let status = state.with_coordinator(|coordinator| {
        coordinator.finish_install(operation_id);
        coordinator.status_snapshot(operation_id)
    });
    quit_state.release_update_install(request_id, operation_id);
    emit_status(&app, status);
    app.request_restart();
    Ok(())
}

#[tauri::command]
pub fn restart_after_update<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
    operation_id: u64,
) -> Result<(), UpdateError> {
    state.with_coordinator(|coordinator| coordinator.validate_restart_needed(operation_id))?;
    app.request_restart();
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_macos_update(bytes: &[u8], app_path: &Path) -> std::io::Result<()> {
    install_macos_update_with_swap(bytes, app_path, swap_app_bundles)
}

#[cfg(target_os = "macos")]
fn install_macos_update_with_swap(
    bytes: &[u8],
    app_path: &Path,
    swap: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use flate2::read::GzDecoder;
    use std::io::{Error, ErrorKind};

    let parent = app_path.parent().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "installed app has no parent directory",
        )
    })?;
    let staging = match tempfile::Builder::new()
        .prefix(".tau-update-")
        .tempdir_in(parent)
    {
        Ok(staging) => staging,
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            tempfile::Builder::new().prefix("tau-update-").tempdir()?
        }
        Err(error) => return Err(error),
    };
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    archive.unpack(staging.path())?;

    let mut entries = std::fs::read_dir(staging.path())?;
    let staged_app = entries
        .next()
        .transpose()?
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir() && path.extension().is_some_and(|extension| extension == "app")
        })
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "update archive has no app bundle"))?;
    if entries.next().transpose()?.is_some() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "update archive has multiple top-level entries",
        ));
    }

    // The exchange is a single filesystem operation: failure leaves the
    // installed bundle untouched, while success moves the old bundle into the
    // private staging directory for best-effort cleanup.
    swap(app_path, &staged_app)
}

#[cfg(target_os = "macos")]
fn swap_app_bundles(installed: &Path, staged: &Path) -> std::io::Result<()> {
    match swap_app_bundles_atomic(installed, staged) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            swap_app_bundles_privileged(installed, staged)
        }
        result => result,
    }
}

#[cfg(target_os = "macos")]
fn swap_app_bundles_atomic(installed: &Path, staged: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::io::{Error, ErrorKind};
    use std::os::unix::ffi::OsStrExt;

    let installed = CString::new(installed.as_os_str().as_bytes())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "installed app path contains NUL"))?;
    let staged = CString::new(staged.as_os_str().as_bytes())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "staged app path contains NUL"))?;
    // SAFETY: both C strings live for the duration of the call and point to
    // existing paths. RENAME_SWAP leaves both paths present on success.
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            installed.as_ptr(),
            libc::AT_FDCWD,
            staged.as_ptr(),
            libc::RENAME_SWAP,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
const PRIVILEGED_SWAP_SCRIPT: &str = r#"
on run argv
  if (count of argv) is not 3 then error "invalid updater arguments"
  set helperPath to item 1 of argv
  set installedPath to item 2 of argv
  set stagedPath to item 3 of argv
  do shell script quoted form of helperPath & " --tau-apply-update " & quoted form of installedPath & " " & quoted form of stagedPath with administrator privileges
end run
"#;

#[cfg(target_os = "macos")]
fn privileged_swap_command(
    helper: &Path,
    installed: &Path,
    staged: &Path,
) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/osascript");
    command
        .args(["-e", PRIVILEGED_SWAP_SCRIPT, "--"])
        .arg(helper)
        .arg(installed)
        .arg(staged);
    command
}

#[cfg(target_os = "macos")]
fn swap_app_bundles_privileged(installed: &Path, staged: &Path) -> std::io::Result<()> {
    use std::io::Error;

    let helper = std::env::current_exe()?;
    let status = privileged_swap_command(&helper, installed, staged).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::other("privileged app exchange failed"))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn swap_helper_exit_code() -> Option<i32> {
    use std::ffi::OsStr;

    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(OsStr::new("--tau-apply-update")) {
        return None;
    }
    let installed = arguments.next().map(PathBuf::from);
    let staged = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        return Some(1);
    }
    let result = match installed.zip(staged) {
        Some((installed, staged)) => (|| -> Result<(), UpdateError> {
            let failure = || {
                UpdateError::new(
                    UpdateErrorCategory::InstallFailed,
                    "The update could not be installed. Try again.",
                )
            };
            let own_bundle = installed_app_path()?
                .canonicalize()
                .map_err(|_| failure())?;
            let installed = installed.canonicalize().map_err(|_| failure())?;
            let staged = staged.canonicalize().map_err(|_| failure())?;
            if own_bundle != installed
                || !staged.is_dir()
                || !staged
                    .extension()
                    .is_some_and(|extension| extension == "app")
            {
                return Err(failure());
            }
            swap_app_bundles_atomic(&installed, &staged).map_err(|_| failure())
        })(),
        None => Err(UpdateError::new(
            UpdateErrorCategory::InstallFailed,
            "The update could not be installed. Try again.",
        )),
    };
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn swap_helper_exit_code() -> Option<i32> {
    None
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum UpdateProgressPhase {
    Downloading,
    Verifying,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    operation_id: u64,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    phase: UpdateProgressPhase,
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    operation_id: u64,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    phase: UpdateProgressPhase,
) {
    let _ = app.emit(
        UPDATE_PROGRESS_EVENT,
        UpdateProgress {
            operation_id,
            downloaded_bytes,
            total_bytes,
            phase,
        },
    );
}

fn emit_status<R: Runtime>(app: &AppHandle<R>, snapshot: UpdateStatusSnapshot) {
    let _ = app.emit(UPDATE_STATUS_EVENT, snapshot);
}

fn ensure_available() -> Result<(), UpdateError> {
    if updater_public_key().is_none() {
        return Err(UpdateError::unavailable());
    }
    if !cfg!(all(target_os = "macos", not(any(dev, test)))) {
        return Err(UpdateError::unsupported());
    }
    // Tauri's executable_dir is the user's executable directory and is
    // unsupported on macOS; it is not the directory of this process.
    preflight_app_path(&installed_app_path()?)
}

fn installed_app_path() -> Result<PathBuf, UpdateError> {
    let executable = std::env::current_exe().map_err(|_| UpdateError::unsupported())?;
    installed_app_path_from_executable(&executable)
}

fn installed_app_path_from_executable(executable: &Path) -> Result<PathBuf, UpdateError> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(UpdateError::unsupported)
}

fn preflight_app_path(app_path: &Path) -> Result<(), UpdateError> {
    let canonical = app_path
        .canonicalize()
        .unwrap_or_else(|_| app_path.to_path_buf());
    if canonical.starts_with("/Volumes") {
        return Err(UpdateError::new(
            UpdateErrorCategory::PreflightFailed,
            "Move Tau to Applications before installing an update.",
        ));
    }

    let parent = canonical.parent().ok_or_else(|| {
        UpdateError::new(
            UpdateErrorCategory::PreflightFailed,
            "Tau's installation location cannot be updated.",
        )
    })?;
    let metadata = std::fs::metadata(parent).map_err(|_| {
        UpdateError::new(
            UpdateErrorCategory::PreflightFailed,
            "Tau's installation location cannot be updated.",
        )
    })?;
    if !parent_is_writable(&metadata) {
        return Err(UpdateError::new(
            UpdateErrorCategory::PreflightFailed,
            "Tau's installation location is not writable.",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn parent_is_writable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o222 != 0
}

#[cfg(not(unix))]
fn parent_is_writable(metadata: &std::fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}

fn check_failed() -> UpdateError {
    UpdateError::new(
        UpdateErrorCategory::CheckFailed,
        "Tau could not check for updates. Try again.",
    )
}

fn download_failed() -> UpdateError {
    UpdateError::new(
        UpdateErrorCategory::DownloadFailed,
        "The update could not be downloaded. Try again.",
    )
}

fn map_check_error(_error: tauri_plugin_updater::Error) -> UpdateError {
    check_failed()
}

fn map_download_error(error: tauri_plugin_updater::Error) -> UpdateError {
    use tauri_plugin_updater::Error;
    match error {
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => UpdateError::new(
            UpdateErrorCategory::VerificationFailed,
            "The update could not be verified and was not installed.",
        ),
        _ => download_failed(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(target_os = "macos")]
    use std::io::{Error, ErrorKind};
    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::Listener;

    use super::*;

    #[test]
    fn base_config_initializes_updater_without_a_release_key() {
        let root: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("Tauri config");
        let updater = root
            .pointer("/plugins/updater")
            .cloned()
            .expect("updater config");
        let config: tauri_plugin_updater::Config =
            serde_json::from_value(updater.clone()).expect("deserializable updater config");
        assert!(config.pubkey.is_empty());
        assert_eq!(config.endpoints.len(), 1);

        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), updater);
        let app = tauri::test::mock_builder()
            .plugin(plugin())
            .build(context)
            .expect("updater plugin initializes");
        assert!(app.updater_builder().build().is_ok());

        let error = ensure_available().expect_err("test builds cannot update");
        assert!(matches!(
            error.category,
            UpdateErrorCategory::Unavailable | UpdateErrorCategory::Unsupported
        ));
    }

    #[test]
    fn coordinator_enforces_single_flight_and_stale_operation_ids() {
        let mut coordinator = UpdateCoordinator::default();
        let check = coordinator.begin_check(true).expect("begin check");
        assert_eq!(coordinator.begin_check(false), Err(UpdateError::busy()));
        coordinator.finish_check(check + 1, None);
        assert_eq!(coordinator.phase, Phase::Checking(check));
        coordinator.finish_check(check, None);
        assert_eq!(coordinator.phase, Phase::UpToDate);
    }

    #[test]
    fn terminal_transitions_produce_reconcilable_status_snapshots() {
        let mut coordinator = UpdateCoordinator {
            phase: Phase::Downloading(7),
            ..Default::default()
        };
        coordinator.finish_download(7, vec![1, 2, 3]);
        assert_eq!(
            coordinator.status_snapshot(7),
            UpdateStatusSnapshot {
                supported: true,
                status: UpdateStatus::Prepared,
                operation_id: 7,
                manual: false,
                candidate: None,
                error_category: None,
            }
        );

        coordinator.phase = Phase::Installing(7);
        coordinator.fail_install(
            7,
            UpdateError::new(UpdateErrorCategory::InstallFailed, "private detail"),
        );
        assert_eq!(
            coordinator.status_snapshot(7),
            UpdateStatusSnapshot {
                supported: true,
                status: UpdateStatus::Prepared,
                operation_id: 7,
                manual: false,
                candidate: None,
                error_category: Some(UpdateErrorCategory::InstallFailed),
            }
        );

        coordinator.phase = Phase::Downloading(8);
        coordinator.fail(
            8,
            UpdateError::new(UpdateErrorCategory::DownloadFailed, "private detail"),
        );
        assert_eq!(
            coordinator.status_snapshot(8),
            UpdateStatusSnapshot {
                supported: true,
                status: UpdateStatus::Failed,
                operation_id: 8,
                manual: false,
                candidate: None,
                error_category: Some(UpdateErrorCategory::DownloadFailed),
            }
        );
    }

    #[test]
    fn completed_install_is_restartable_without_installing_again() {
        let mut coordinator = UpdateCoordinator {
            phase: Phase::Installing(7),
            verified_bytes: Some(vec![1, 2, 3]),
            ..Default::default()
        };

        coordinator.finish_install(7);

        assert_eq!(coordinator.phase, Phase::RestartNeeded(7));
        assert!(coordinator.verified_bytes.is_none());
        assert_eq!(coordinator.validate_restart_needed(7), Ok(()));
        assert!(coordinator.validate_restart_needed(8).is_err());
        assert_eq!(
            coordinator.status_snapshot(7).status,
            UpdateStatus::RestartNeeded
        );
    }

    #[test]
    fn status_event_payload_is_bounded_and_content_free() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app");
        let (sender, receiver) = mpsc::channel();
        app.listen(UPDATE_STATUS_EVENT, move |event| {
            let payload: serde_json::Value =
                serde_json::from_str(event.payload()).expect("status payload");
            sender.send(payload).expect("status receiver");
        });

        emit_status(
            app.handle(),
            UpdateStatusSnapshot {
                supported: true,
                status: UpdateStatus::Prepared,
                operation_id: 9,
                manual: false,
                candidate: None,
                error_category: Some(UpdateErrorCategory::InstallFailed),
            },
        );

        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
            serde_json::json!({
                "supported": true,
                "status": "prepared",
                "operationId": 9,
                "manual": false,
                "candidate": null,
                "errorCategory": "installFailed",
            })
        );
    }

    #[test]
    fn completed_check_has_a_terminal_status_snapshot() {
        let mut coordinator = UpdateCoordinator::default();
        let operation_id = coordinator.begin_check(false).unwrap();
        coordinator.finish_check(operation_id, None);

        assert_eq!(
            coordinator.status_snapshot(operation_id),
            UpdateStatusSnapshot {
                supported: true,
                status: UpdateStatus::UpToDate,
                operation_id,
                manual: false,
                candidate: None,
                error_category: None,
            }
        );
    }

    #[test]
    fn errors_expose_only_reviewed_copy() {
        let source = tauri_plugin_updater::Error::Network(
            "https://secret.example/token?credential=value".into(),
        );
        let error = map_download_error(source);
        assert_eq!(error.category, UpdateErrorCategory::DownloadFailed);
        assert_eq!(
            error.message,
            "The update could not be downloaded. Try again."
        );
        assert!(!error.message.contains("secret"));
    }

    #[test]
    fn signature_errors_have_a_distinct_safe_category() {
        let source = tauri_plugin_updater::Error::SignatureUtf8("private response".into());
        let error = map_download_error(source);
        assert_eq!(error.category, UpdateErrorCategory::VerificationFailed);
        assert!(!error.message.contains("private response"));
    }

    #[test]
    fn identifies_an_app_bundle_from_its_executable() {
        let path = installed_app_path_from_executable(Path::new(
            "/Applications/Tau.app/Contents/MacOS/Tau",
        ))
        .expect("app path");
        assert_eq!(path, Path::new("/Applications/Tau.app"));
        assert!(installed_app_path_from_executable(Path::new("/tmp/tau")).is_err());
    }

    #[test]
    fn preflight_rejects_apps_on_mounted_images() {
        let error = preflight_app_path(Path::new("/Volumes/Tau/Tau.app")).unwrap_err();
        assert_eq!(error.category, UpdateErrorCategory::PreflightFailed);
        assert_eq!(
            error.message,
            "Move Tau to Applications before installing an update."
        );
    }

    #[cfg(unix)]
    #[test]
    fn preflight_rejects_a_non_writable_parent() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory");
        let parent = directory.path().join("Applications");
        fs::create_dir(&parent).expect("applications directory");
        let app = parent.join("Tau.app");
        fs::create_dir(&app).expect("app bundle");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).expect("permissions");

        let error = preflight_app_path(&app).unwrap_err();
        assert_eq!(error.category, UpdateErrorCategory::PreflightFailed);
        assert_eq!(
            error.message,
            "Tau's installation location is not writable."
        );
    }

    #[test]
    fn preflight_accepts_a_writable_parent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let app = directory.path().join("Tau.app");
        fs::create_dir(&app).expect("app bundle");
        assert_eq!(preflight_app_path(&app), Ok(()));
    }

    #[cfg(target_os = "macos")]
    fn updater_archive(marker: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(marker.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "Tau.app/version", marker)
            .expect("archive marker");
        archive
            .into_inner()
            .expect("archive encoder")
            .finish()
            .expect("compressed archive")
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn failed_bundle_exchange_preserves_the_installed_app() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let app = directory.path().join("Tau.app");
        fs::create_dir(&app).expect("installed app");
        fs::write(app.join("version"), b"old").expect("old marker");

        let error = install_macos_update_with_swap(
            &updater_archive(b"new"),
            &app,
            |_installed, _staged| Err(Error::new(ErrorKind::PermissionDenied, "injected")),
        )
        .expect_err("injected exchange failure");

        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert_eq!(fs::read(app.join("version")).unwrap(), b"old");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn privileged_exchange_passes_shell_metacharacters_as_arguments() {
        use std::ffi::OsStr;

        let helper = Path::new("/Applications/Tau.app/Contents/MacOS/tau");
        let installed = Path::new("/Applications/Tau '$(touch injected)'.app");
        let staged = Path::new("/tmp/Tau '; rm -rf ~'.app");
        let command = privileged_swap_command(helper, installed, staged);
        let arguments = command.get_args().collect::<Vec<_>>();

        assert_eq!(arguments[0], OsStr::new("-e"));
        assert_eq!(arguments[1], OsStr::new(PRIVILEGED_SWAP_SCRIPT));
        assert_eq!(arguments[2], OsStr::new("--"));
        assert_eq!(arguments[3], helper.as_os_str());
        assert_eq!(arguments[4], installed.as_os_str());
        assert_eq!(arguments[5], staged.as_os_str());
        assert!(PRIVILEGED_SWAP_SCRIPT.contains("quoted form of installedPath"));
        assert!(!PRIVILEGED_SWAP_SCRIPT.contains("touch injected"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn atomic_exchange_handles_shell_metacharacters_as_plain_path_data() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let sentinel = directory.path().join("injected");
        let app = directory.path().join("Tau '$(touch injected)' ;.app");
        fs::create_dir(&app).expect("installed app");
        fs::write(app.join("version"), b"old").expect("old marker");

        install_macos_update(&updater_archive(b"new"), &app).expect("atomic exchange");

        assert_eq!(fs::read(app.join("version")).unwrap(), b"new");
        assert!(!sentinel.exists());
    }
}
