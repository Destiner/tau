use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const QUIT_MENU_ID: &str = "tau-quit";
pub const QUIT_REQUEST_EVENT: &str = "tau://quit-requested";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuitIntent {
    Ordinary,
    UpdateRestart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitRequest {
    request_id: u64,
    intent: QuitIntent,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<u64>,
}

#[derive(Debug, Default)]
struct QuitCoordinator {
    next_request_id: u64,
    status: QuitStatus,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum QuitStatus {
    #[default]
    Idle,
    Pending(QuitRequest),
    UpdateInstallAuthorized(QuitRequest),
    UpdateInstalling(QuitRequest),
    Exiting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuitResolution {
    Ignored,
    Cancelled,
    OrdinaryConfirmed,
    UpdateAuthorized,
}

impl QuitCoordinator {
    fn request(&mut self, intent: QuitIntent, operation_id: Option<u64>) -> Option<QuitRequest> {
        match self.status {
            QuitStatus::Idle => {
                self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
                let request = QuitRequest {
                    request_id: self.next_request_id,
                    intent,
                    operation_id,
                };
                self.status = QuitStatus::Pending(request);
                Some(request)
            }
            QuitStatus::Pending(request)
                if request.intent == intent && request.operation_id == operation_id =>
            {
                Some(request)
            }
            QuitStatus::Pending(_)
            | QuitStatus::UpdateInstallAuthorized(_)
            | QuitStatus::UpdateInstalling(_)
            | QuitStatus::Exiting => None,
        }
    }

    fn pending(&self) -> Option<QuitRequest> {
        match self.status {
            QuitStatus::Pending(request) | QuitStatus::UpdateInstallAuthorized(request) => {
                Some(request)
            }
            _ => None,
        }
    }

    fn resolve(&mut self, request_id: u64, confirmed: bool) -> QuitResolution {
        let request = match self.status {
            QuitStatus::Pending(request) | QuitStatus::UpdateInstallAuthorized(request) => request,
            _ => return QuitResolution::Ignored,
        };
        if request.request_id != request_id {
            return QuitResolution::Ignored;
        }
        if !confirmed {
            self.status = QuitStatus::Idle;
            return QuitResolution::Cancelled;
        }

        match request.intent {
            QuitIntent::Ordinary => {
                self.status = QuitStatus::Exiting;
                QuitResolution::OrdinaryConfirmed
            }
            QuitIntent::UpdateRestart => {
                self.status = QuitStatus::UpdateInstallAuthorized(request);
                QuitResolution::UpdateAuthorized
            }
        }
    }

    fn consume_update_authorization(&mut self, request_id: u64, operation_id: u64) -> bool {
        let QuitStatus::UpdateInstallAuthorized(request) = self.status else {
            return false;
        };
        if request.request_id != request_id || request.operation_id != Some(operation_id) {
            return false;
        }
        self.status = QuitStatus::UpdateInstalling(request);
        true
    }

    fn release_update_install(&mut self, request_id: u64, operation_id: u64) {
        if matches!(
            self.status,
            QuitStatus::UpdateInstalling(request)
                if request.request_id == request_id
                    && request.operation_id == Some(operation_id)
        ) {
            self.status = QuitStatus::Idle;
        }
    }
}

#[derive(Default)]
pub struct QuitState(Mutex<QuitCoordinator>);

impl QuitState {
    fn with_coordinator<T>(&self, callback: impl FnOnce(&mut QuitCoordinator) -> T) -> T {
        let mut coordinator = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        callback(&mut coordinator)
    }

    pub(crate) fn consume_update_authorization(&self, request_id: u64, operation_id: u64) -> bool {
        self.with_coordinator(|coordinator| {
            coordinator.consume_update_authorization(request_id, operation_id)
        })
    }

    pub(crate) fn release_update_install(&self, request_id: u64, operation_id: u64) {
        self.with_coordinator(|coordinator| {
            coordinator.release_update_install(request_id, operation_id)
        });
    }
}

pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    request(app, QuitIntent::Ordinary, None);
}

pub(crate) fn request_update_restart<R: Runtime>(app: &AppHandle<R>, operation_id: u64) -> bool {
    request(app, QuitIntent::UpdateRestart, Some(operation_id))
}

fn request<R: Runtime>(app: &AppHandle<R>, intent: QuitIntent, operation_id: Option<u64>) -> bool {
    let request = app
        .state::<QuitState>()
        .with_coordinator(|state| state.request(intent, operation_id));
    if let Some(request) = request {
        let _ = app.emit(QUIT_REQUEST_EVENT, request);
        true
    } else {
        false
    }
}

#[tauri::command]
pub fn pending_quit_request(state: State<'_, QuitState>) -> Option<QuitRequest> {
    state.with_coordinator(|coordinator| coordinator.pending())
}

#[tauri::command]
pub fn resolve_quit_request<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, QuitState>,
    request_id: u64,
    confirmed: bool,
) -> bool {
    let resolution =
        state.with_coordinator(|coordinator| coordinator.resolve(request_id, confirmed));
    if resolution == QuitResolution::OrdinaryConfirmed {
        app.exit(0);
    }
    resolution != QuitResolution::Ignored
}

#[cfg(test)]
mod tests {
    use super::{QuitCoordinator, QuitIntent, QuitResolution, QuitStatus};

    #[test]
    fn reuses_one_pending_request_until_it_is_resolved() {
        let mut coordinator = QuitCoordinator::default();

        let first = coordinator.request(QuitIntent::Ordinary, None).unwrap();
        let repeated = coordinator.request(QuitIntent::Ordinary, None).unwrap();

        assert_eq!(first, repeated);
        assert_eq!(coordinator.pending(), Some(first));
    }

    #[test]
    fn cancellation_returns_to_idle_and_the_next_request_is_new() {
        let mut coordinator = QuitCoordinator::default();
        let first = coordinator.request(QuitIntent::Ordinary, None).unwrap();

        assert_eq!(
            coordinator.resolve(first.request_id, false),
            QuitResolution::Cancelled
        );
        assert_eq!(coordinator.status, QuitStatus::Idle);

        let next = coordinator.request(QuitIntent::Ordinary, None).unwrap();
        assert_ne!(next, first);
    }

    #[test]
    fn ordinary_confirmation_authorizes_exit_once() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator.request(QuitIntent::Ordinary, None).unwrap();

        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::OrdinaryConfirmed
        );
        assert_eq!(coordinator.status, QuitStatus::Exiting);
        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::Ignored
        );
    }

    #[test]
    fn update_authorization_is_bound_to_both_ids_and_consumed_once() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator
            .request(QuitIntent::UpdateRestart, Some(42))
            .unwrap();
        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::UpdateAuthorized
        );

        assert!(!coordinator.consume_update_authorization(request.request_id + 1, 42));
        assert!(!coordinator.consume_update_authorization(request.request_id, 41));
        assert!(coordinator.consume_update_authorization(request.request_id, 42));
        assert!(!coordinator.consume_update_authorization(request.request_id, 42));
    }

    #[test]
    fn authorized_update_request_survives_reload_and_confirmation_replay() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator
            .request(QuitIntent::UpdateRestart, Some(42))
            .unwrap();

        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::UpdateAuthorized
        );
        assert_eq!(coordinator.pending(), Some(request));
        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::UpdateAuthorized
        );
        assert!(coordinator.consume_update_authorization(request.request_id, 42));
    }

    #[test]
    fn recovered_update_authorization_can_be_cancelled() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator
            .request(QuitIntent::UpdateRestart, Some(42))
            .unwrap();
        coordinator.resolve(request.request_id, true);

        assert_eq!(
            coordinator.resolve(request.request_id, false),
            QuitResolution::Cancelled
        );
        assert_eq!(coordinator.pending(), None);
        assert!(coordinator.request(QuitIntent::Ordinary, None).is_some());
    }

    #[test]
    fn failed_update_install_releases_the_coordinator() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator
            .request(QuitIntent::UpdateRestart, Some(7))
            .unwrap();
        coordinator.resolve(request.request_id, true);
        assert!(coordinator.consume_update_authorization(request.request_id, 7));

        coordinator.release_update_install(request.request_id, 7);
        assert_eq!(coordinator.status, QuitStatus::Idle);
        assert!(coordinator.request(QuitIntent::Ordinary, None).is_some());
    }

    #[test]
    fn ignores_stale_responses_and_conflicting_intents() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator.request(QuitIntent::Ordinary, None).unwrap();

        assert_eq!(
            coordinator.resolve(request.request_id + 1, false),
            QuitResolution::Ignored
        );
        assert!(coordinator
            .request(QuitIntent::UpdateRestart, Some(1))
            .is_none());
        assert_eq!(coordinator.pending(), Some(request));
    }
}
