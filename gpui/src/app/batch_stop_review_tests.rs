use super::*;
use crate::app::service;
use std::time::{Duration, Instant};

fn next(output: &async_channel::Receiver<Event>) -> Event {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match output.try_recv() {
            Ok(event) => return event,
            Err(async_channel::TryRecvError::Empty) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => panic!("service receipt deadline: {error:?}"),
        }
    }
}

#[test]
fn shutdown_save_uses_one_budget_for_thirteen_start_attempts_and_preserves_partial_success() {
    const CHILD: &str = "TAU_BATCH_STOP_REVIEW_CHILD";
    if std::env::var_os(CHILD).is_none() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("pi");
        std::fs::write(&fake, "#!/bin/sh\nwhile IFS= read -r line; do :; done\ncase \"$PWD\" in */clean) exit 0 ;; *) sleep 6 & exit 0 ;; esac\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "app::lifecycle::tests::batch_stop_review_tests::shutdown_save_uses_one_budget_for_thirteen_start_attempts_and_preserves_partial_success", "--nocapture"])
            .env(CHILD, "1").env("TAU_PI_PATH", fake)
            .env("PI_CODING_AGENT_DIR", dir.path().join("agent"))
            .status().unwrap();
        assert!(status.success());
        return;
    }
    tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let clean_dir = dir.path().join("clean");
    std::fs::create_dir(&clean_dir).unwrap();
    let clean =
        tau_core::storage::import_project(clean_dir.to_string_lossy().into_owned()).unwrap();
    let held =
        tau_core::storage::import_project(dir.path().to_string_lossy().into_owned()).unwrap();
    let (commands, output) = service::launch(dir.path().into());
    for index in 0..14 {
        commands
            .send_blocking(Command::Start {
                runtime: format!("r{index}"),
                attempt: 1,
                project: if index == 13 {
                    clean.clone()
                } else {
                    held.clone()
                },
                source: None,
            })
            .unwrap();
        loop {
            if let Event::Started { result, .. } = next(&output) {
                assert!(result.is_ok());
                break;
            }
        }
    }
    let started = Instant::now();
    commands
        .send_blocking(Command::Save {
            revision: 71,
            preferences: None,
            shutdown: true,
        })
        .unwrap();
    let stopped = loop {
        if let Event::Saved {
            revision,
            success,
            stopped,
        } = next(&output)
        {
            assert_eq!(revision, 71);
            assert!(!success, "inherited stdout is not a clean stop receipt");
            break stopped;
        }
    };
    assert!(started.elapsed() >= Duration::from_secs(3));
    assert!(started.elapsed() < Duration::from_millis(3500));
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].0, "r13");
    // Retained failures cannot be replaced by successful starts.
    commands
        .send_blocking(Command::Start {
            runtime: "r0".into(),
            attempt: 2,
            project: held,
            source: None,
        })
        .unwrap();
    loop {
        if let Event::Started { result, .. } = next(&output) {
            assert!(result.is_err());
            break;
        }
    }
    let (done, ack) = async_channel::bounded(1);
    commands.send_blocking(Command::Shutdown(done)).unwrap();
    ack.recv_blocking().unwrap();
}
