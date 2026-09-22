use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
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
    Failed(u64),
}

impl Phase {
    fn operation_id(self) -> Option<u64> {
        match self {
            Self::Checking(id)
            | Self::Downloading(id)
            | Self::Prepared(id)
            | Self::Installing(id)
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

    fn begin_install(&mut self, operation_id: u64) -> Result<(Update, Vec<u8>), UpdateError> {
        self.validate_prepared(operation_id)?;
        let update = self.candidate.clone().expect("validated candidate");
        let bytes = self.verified_bytes.clone().expect("validated bytes");
        self.phase = Phase::Installing(operation_id);
        self.error = None;
        Ok((update, bytes))
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
pub fn update_snapshot<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdateState>,
) -> UpdateSnapshot {
    let availability = ensure_available(&app);
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
    if let Err(error) = ensure_available(&app) {
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
    ensure_available(&app)?;
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
    ensure_available(&app)?;
    if !quit_state.consume_update_authorization(request_id, operation_id) {
        return Err(UpdateError::new(
            UpdateErrorCategory::AuthorizationExpired,
            "This update authorization has expired. Try again.",
        ));
    }

    let (update, bytes) =
        match state.with_coordinator(|coordinator| coordinator.begin_install(operation_id)) {
            Ok(prepared) => prepared,
            Err(error) => {
                quit_state.release_update_install(request_id, operation_id);
                return Err(error);
            }
        };

    let install_result = tokio::task::spawn_blocking(move || update.install(&bytes)).await;
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

    app.request_restart();
    Ok(())
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

fn ensure_available<R: Runtime>(app: &AppHandle<R>) -> Result<(), UpdateError> {
    if updater_public_key().is_none() {
        return Err(UpdateError::unavailable());
    }
    if !cfg!(all(target_os = "macos", not(any(dev, test)))) {
        return Err(UpdateError::unsupported());
    }
    preflight_app_path(&installed_app_path_from_executable(
        &app.path()
            .executable_dir()
            .map_err(|_| UpdateError::unsupported())?,
    )?)
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

        let error = ensure_available(app.handle()).expect_err("test builds cannot update");
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
}
