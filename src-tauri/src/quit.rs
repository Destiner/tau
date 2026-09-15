use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const QUIT_MENU_ID: &str = "tau-quit";
pub const QUIT_REQUEST_EVENT: &str = "tau://quit-requested";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitRequest {
    request_id: u64,
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
    Exiting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuitResolution {
    Ignored,
    Cancelled,
    Confirmed,
}

impl QuitCoordinator {
    fn request(&mut self) -> Option<QuitRequest> {
        match self.status {
            QuitStatus::Idle => {
                self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
                let request = QuitRequest {
                    request_id: self.next_request_id,
                };
                self.status = QuitStatus::Pending(request);
                Some(request)
            }
            QuitStatus::Pending(request) => Some(request),
            QuitStatus::Exiting => None,
        }
    }

    fn pending(&self) -> Option<QuitRequest> {
        match self.status {
            QuitStatus::Pending(request) => Some(request),
            QuitStatus::Idle | QuitStatus::Exiting => None,
        }
    }

    fn resolve(&mut self, request_id: u64, confirmed: bool) -> QuitResolution {
        let QuitStatus::Pending(request) = self.status else {
            return QuitResolution::Ignored;
        };
        if request.request_id != request_id {
            return QuitResolution::Ignored;
        }
        if confirmed {
            self.status = QuitStatus::Exiting;
            QuitResolution::Confirmed
        } else {
            self.status = QuitStatus::Idle;
            QuitResolution::Cancelled
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
}

pub fn request_quit<R: Runtime>(app: &AppHandle<R>) {
    let request = app
        .state::<QuitState>()
        .with_coordinator(|state| state.request());
    if let Some(request) = request {
        let _ = app.emit(QUIT_REQUEST_EVENT, request);
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
    if resolution == QuitResolution::Confirmed {
        app.exit(0);
    }
    resolution != QuitResolution::Ignored
}

#[cfg(test)]
mod tests {
    use super::{QuitCoordinator, QuitResolution, QuitStatus};

    #[test]
    fn reuses_one_pending_request_until_it_is_resolved() {
        let mut coordinator = QuitCoordinator::default();

        let first = coordinator.request().unwrap();
        let repeated = coordinator.request().unwrap();

        assert_eq!(first, repeated);
        assert_eq!(coordinator.pending(), Some(first));
    }

    #[test]
    fn cancellation_returns_to_idle_and_the_next_request_is_new() {
        let mut coordinator = QuitCoordinator::default();
        let first = coordinator.request().unwrap();

        assert_eq!(
            coordinator.resolve(first.request_id, false),
            QuitResolution::Cancelled
        );
        assert_eq!(coordinator.status, QuitStatus::Idle);
        assert_eq!(coordinator.pending(), None);

        let next = coordinator.request().unwrap();
        assert_ne!(next, first);
    }

    #[test]
    fn confirmation_authorizes_exit_once() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator.request().unwrap();

        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::Confirmed
        );
        assert_eq!(coordinator.status, QuitStatus::Exiting);
        assert_eq!(
            coordinator.resolve(request.request_id, true),
            QuitResolution::Ignored
        );
        assert_eq!(coordinator.request(), None);
    }

    #[test]
    fn ignores_a_stale_response() {
        let mut coordinator = QuitCoordinator::default();
        let request = coordinator.request().unwrap();

        assert_eq!(
            coordinator.resolve(request.request_id + 1, false),
            QuitResolution::Ignored
        );
        assert_eq!(coordinator.pending(), Some(request));
    }
}
