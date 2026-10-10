//! The shared backend owns both gates and persistence; replies contain no user content.
use super::super::admin::{Command, Reply};
use std::sync::Arc;
use tau_observability::{
    Telemetry,
    telemetry::native::{Lifecycle, Operation, Outcome},
};

pub(super) struct Backend(pub Option<Arc<Telemetry>>);
impl Backend {
    pub(super) fn finish(&mut self) {
        if let Some(telemetry) = self.0.take() {
            telemetry.record_lifecycle(Lifecycle::Exited);
            telemetry.shutdown();
        }
    }

    pub(super) fn execute(&self, command: Command) -> Reply {
        let Some(telemetry) = &self.0 else {
            return failed(command, false);
        };
        match command {
            Command::Load { revision } => Reply::Loaded {
                revision,
                enabled: telemetry.read_admin_mode(),
            },
            Command::SetMode { revision, enabled } => {
                let saved = telemetry.set_admin_mode(enabled).is_ok();
                // The shared backend intentionally applies the live gate before saving.
                Reply::ModeSet {
                    revision,
                    enabled: telemetry.read_admin_mode(),
                    saved,
                }
            }
            Command::Submit {
                revision,
                description,
                session_id,
            } => {
                let timer = telemetry.start_operation(
                    Operation::command("submit_issue_report").expect("reviewed command"),
                );
                let saved = telemetry
                    .submit_issue_report(description, session_id)
                    .is_ok();
                timer.finish(if saved {
                    Outcome::Success
                } else {
                    Outcome::Error
                });
                Reply::Submitted { revision, saved }
            }
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        self.finish();
    }
}

pub(super) fn failed(command: Command, actual_enabled: bool) -> Reply {
    match command {
        Command::Load { revision } => Reply::Loaded {
            revision,
            enabled: actual_enabled,
        },
        Command::SetMode { revision, .. } => Reply::ModeSet {
            revision,
            enabled: actual_enabled,
            saved: false,
        },
        Command::Submit { revision, .. } => Reply::Submitted {
            revision,
            saved: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_observability::telemetry::Environment;

    #[test]
    fn real_backend_persistence_failure_reports_actual_gate_and_keeps_submit_guard() {
        let profile = tau_core::profile::StorageProfile::ephemeral().unwrap();
        let telemetry = Telemetry::init_for_profile(&profile, Environment::Development).unwrap();
        let backend = Backend(Some(telemetry));
        // A directory at the preferences filename deterministically fails persistence.
        std::fs::create_dir_all(profile.data_dir().join("preferences.json")).unwrap();
        assert_eq!(
            backend.execute(Command::SetMode {
                revision: 1,
                enabled: true
            }),
            Reply::ModeSet {
                revision: 1,
                enabled: true,
                saved: false
            }
        );
        assert_eq!(
            backend.execute(Command::SetMode {
                revision: 2,
                enabled: false
            }),
            Reply::ModeSet {
                revision: 2,
                enabled: false,
                saved: false
            }
        );
        assert_eq!(
            backend.execute(Command::Submit {
                revision: 3,
                description: "explicit report".into(),
                session_id: None
            }),
            Reply::Submitted {
                revision: 3,
                saved: false
            }
        );
    }
}
