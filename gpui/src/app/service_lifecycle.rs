use super::*;

/// Storage receipts own exact payloads independently of the live transport map.
#[derive(Default)]
pub(super) struct RegistrationTickets {
    accepted: std::collections::HashSet<(String, u64, String, String, String, String)>,
    incarnations: std::collections::HashSet<(String, u64, String)>,
}
impl RegistrationTickets {
    pub(super) fn admit(
        &mut self,
        command: &Command,
        current: &HashMap<String, u64>,
        projects: &HashMap<String, String>,
        removed: &std::collections::HashSet<String>,
    ) -> bool {
        let Command::Register {
            runtime,
            generation,
            project,
            id,
            path,
            name,
        } = command
        else {
            return false;
        };
        let ticket = (
            runtime.clone(),
            *generation,
            project.clone(),
            id.clone(),
            path.clone(),
            name.clone(),
        );
        if removed.contains(project) {
            return false;
        }
        if self.accepted.contains(&ticket) {
            return true;
        }
        if !self
            .incarnations
            .contains(&(runtime.clone(), *generation, project.clone()))
            && (current.get(runtime) != Some(generation) || projects.get(runtime) != Some(project))
        {
            return false;
        }
        self.accepted.insert(ticket);
        true
    }
    pub(super) fn started(&mut self, runtime: &str, generation: u64, project: &str) {
        self.incarnations
            .insert((runtime.into(), generation, project.into()));
    }
    pub(super) fn revoke_project(&mut self, project: &str) {
        self.accepted.retain(|(_, _, p, _, _, _)| p != project);
        self.incarnations.retain(|(_, _, p)| p != project);
    }
}

pub(super) fn stop_cleanup(
    pi: &PiState,
    owner: &str,
    runtime: &str,
    attempt: u64,
    generation: Option<u64>,
    events: &Sender<Event>,
) -> bool {
    match pi.stop_checked(owner, runtime) {
        Ok(_) => true,
        Err(failure) => {
            if !failure.retryable {
                let _ = events.send_blocking(Event::CleanupBlocked {
                    runtime: runtime.into(),
                    attempt,
                    generation,
                });
            }
            false
        }
    }
}

pub(super) fn release_runtime(
    runtime: &str,
    attempt: u64,
    generation: u64,
    starts: &mut HashMap<String, u64>,
    generations: &mut HashMap<String, u64>,
    registry: &mut HashMap<String, u64>,
    stop: impl FnOnce() -> Result<(), ()>,
) -> bool {
    let current = starts.get(runtime) == Some(&attempt)
        && (generation == 0 || generations.get(runtime) == Some(&generation));
    if !current {
        return true;
    }
    if stop().is_err() {
        return false;
    }
    starts.remove(runtime);
    generations.remove(runtime);
    registry.remove(runtime);
    true
}

pub(super) fn retry_claim(
    claimed: &mut bool,
    revision: &mut Option<u64>,
    read: impl FnOnce() -> Option<u64>,
    claim: impl FnOnce(u64) -> bool,
) {
    if *claimed {
        return;
    }
    if revision.is_none() {
        *revision = read();
    }
    if let Some(expected) = *revision {
        *claimed = claim(expected);
    }
}

#[cfg(test)]
#[path = "service_registration_review_tests.rs"]
mod registration_review_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn fake_ssh_eof_exit_classifies_release_without_transport_or_auth() {
        use std::os::unix::fs::PermissionsExt;
        for code in [0, 255] {
            let dir = tempfile::tempdir().unwrap();
            let ssh = dir.path().join("ssh");
            std::fs::write(
                &ssh,
                format!("#!/bin/sh\nwhile IFS= read -r line; do :; done\nexit {code}\n"),
            )
            .unwrap();
            std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
            let pi = PiState::default();
            let revision = pi.read_revision().unwrap();
            pi.claim("owner".into(), revision).unwrap();
            let generation = pi
                .start_remote(
                    "owner".into(),
                    "remote".into(),
                    format!("'{}' unused", ssh.to_str().unwrap().replace('\'', "'\\''")),
                    "/unused".into(),
                    None,
                    PiSink::new(Arc::new(|_: PiEvent| {})),
                )
                .unwrap();
            let mut starts = HashMap::from([("remote".into(), 9)]);
            let mut generations = HashMap::from([("remote".into(), generation)]);
            let mut registry = generations.clone();
            let (events, receipts) = async_channel::bounded(8);
            let success = release_runtime(
                "remote",
                9,
                generation,
                &mut starts,
                &mut generations,
                &mut registry,
                || {
                    stop_cleanup(&pi, "owner", "remote", 9, Some(generation), &events)
                        .then_some(())
                        .ok_or(())
                },
            );
            assert_eq!(success, code == 0);
            if code == 0 {
                assert!(receipts.try_recv().is_err());
                assert!(starts.is_empty());
                assert!(generations.is_empty());
                assert!(registry.is_empty());
            } else {
                assert!(
                    matches!(receipts.try_recv().unwrap(), Event::CleanupBlocked {
                    runtime, attempt: 9, generation: Some(g),
                } if runtime == "remote" && g == generation)
                );
                assert_eq!(starts["remote"], 9);
                assert_eq!(generations["remote"], generation);
                assert_eq!(registry["remote"], generation);
                assert!(!pi.stop_checked("owner", "remote").err().unwrap().retryable);
            }
        }
    }

    #[test]
    fn failed_stop_retains_ack_ownership_and_stale_release_never_stops_successor() {
        let mut starts = HashMap::from([("r".into(), 2)]);
        let mut generations = HashMap::from([("r".into(), 7)]);
        let mut registry = generations.clone();
        assert!(!release_runtime(
            "r",
            2,
            7,
            &mut starts,
            &mut generations,
            &mut registry,
            || Err(())
        ));
        assert_eq!(generations["r"], 7);
        assert_eq!(registry["r"], 7);
        assert!(release_runtime(
            "r",
            1,
            6,
            &mut starts,
            &mut generations,
            &mut registry,
            || panic!("stale stop")
        ));
        assert!(release_runtime(
            "r",
            2,
            6,
            &mut starts,
            &mut generations,
            &mut registry,
            || panic!("stale generation stop")
        ));
        assert!(release_runtime(
            "r",
            2,
            7,
            &mut starts,
            &mut generations,
            &mut registry,
            || Ok(())
        ));
        assert!(starts.is_empty() && generations.is_empty() && registry.is_empty());
    }
    #[test]
    fn ownership_retry_uses_original_revision_not_a_new_takeover_revision() {
        let mut claimed = false;
        let mut revision = None;
        retry_claim(
            &mut claimed,
            &mut revision,
            || Some(4),
            |r| {
                assert_eq!(r, 4);
                false
            },
        );
        retry_claim(
            &mut claimed,
            &mut revision,
            || panic!("must not reacquire takeover revision"),
            |r| {
                assert_eq!(r, 4);
                true
            },
        );
        assert!(claimed);
        retry_claim(&mut claimed, &mut revision, || panic!(), |_| panic!());
    }
    #[test]
    fn worker_project_removal_is_registry_only_and_reimport_is_authoritative() {
        const CHILD: &str = "TAU_LIFECYCLE_STORAGE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "app::service::lifecycle::tests::worker_project_removal_is_registry_only_and_reimport_is_authoritative", "--nocapture"])
                .env(CHILD, "1").env("PI_CODING_AGENT_DIR", directory.path()).status().unwrap();
            assert!(status.success());
            return;
        }
        tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap())
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let project =
            storage::import_project(directory.path().to_string_lossy().into_owned()).unwrap();
        let session_directory = storage::default_session_dir(&project.path).unwrap();
        std::fs::create_dir_all(&session_directory).unwrap();
        let transcript = session_directory.join("2026-01-01_sentinel.jsonl");
        let bytes = serde_json::to_vec(&serde_json::json!({"type":"session","version":3,"id":"sentinel","timestamp":"2026-01-01T00:00:00Z","cwd":project.working_directory})).unwrap();
        std::fs::write(&transcript, &bytes).unwrap();
        let data = tempfile::tempdir().unwrap();
        let (commands, output) = launch(data.path().into());
        commands
            .send_blocking(Command::RemoveProject {
                project: project.path.clone(),
                token: 1,
            })
            .unwrap();
        assert!(matches!(
            output.recv_blocking().unwrap(),
            Event::ProjectRemoved {
                token: 1,
                success: true,
                ..
            }
        ));
        assert!(storage::load_workspace().unwrap().projects.is_empty());
        assert_eq!(std::fs::read(&transcript).unwrap(), bytes);
        commands
            .send_blocking(Command::Register {
                runtime: "retired".into(),
                generation: 7,
                project: project.path.clone(),
                id: "late".into(),
                path: transcript.to_string_lossy().into_owned(),
                name: String::new(),
            })
            .unwrap();
        assert!(matches!(
            output.recv_blocking().unwrap(),
            Event::RegistrationFailed(_)
        ));
        // FIFO sentinel proves a stale registration did not recreate the project.
        commands
            .send_blocking(Command::ReleaseRuntime {
                runtime: "retired".into(),
                attempt: 1,
                generation: 7,
            })
            .unwrap();
        assert!(matches!(
            output.recv_blocking().unwrap(),
            Event::RuntimeReleased { success: true, .. }
        ));
        assert!(storage::load_workspace().unwrap().projects.is_empty());
        commands
            .send_blocking(Command::ImportProject {
                path: project.path,
                token: 2,
            })
            .unwrap();
        assert!(matches!(
            output.recv_blocking().unwrap(),
            Event::ProjectImported {
                token: 2,
                result: Ok(_),
                ..
            }
        ));
        assert_eq!(storage::load_workspace().unwrap().projects.len(), 1);
        let (done, completed) = async_channel::bounded(1);
        commands.send_blocking(Command::Shutdown(done)).unwrap();
        completed.recv_blocking().unwrap();
    }
    #[test]
    fn actual_pi_ownership_conflict_is_not_force_claimed_by_retry() {
        let pi = PiState::default();
        let original = pi.read_revision().unwrap();
        pi.claim("other".into(), original).unwrap();
        let mut claimed = false;
        let mut revision = Some(original);
        retry_claim(
            &mut claimed,
            &mut revision,
            || pi.read_revision().ok(),
            |r| pi.claim("shell".into(), r).is_ok(),
        );
        assert!(!claimed);
        assert_eq!(revision, Some(original));
        // The original owner remains authoritative.
        assert!(
            !pi.claim("other".into(), pi.read_revision().unwrap())
                .unwrap()
                .replacement
        );
    }
}
