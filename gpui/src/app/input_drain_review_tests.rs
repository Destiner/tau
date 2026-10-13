use super::*;
use crate::app::service;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn next(output: &async_channel::Receiver<Event>) -> Event {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match output.try_recv() {
            Ok(event) => return event,
            Err(async_channel::TryRecvError::Empty) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("service receipt deadline: {error:?}"),
        }
    }
}

fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "fixture acknowledgement deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// Release every child-side wait before shutting down; otherwise panic cleanup
// can leave the fake holding stdout open while Core waits for its output drain.
fn release_waits(dir: &Path) {
    for name in ["state-release", "final-release"] {
        let _ = std::fs::write(dir.join(name), "release");
    }
}

struct FakeGuard {
    worker: async_channel::Sender<Command>,
    output: async_channel::Receiver<Event>,
    dir: PathBuf,
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        release_waits(&self.dir);
        let (done, ack) = async_channel::bounded(1);
        let sent = self.worker.send_blocking(Command::Shutdown(done)).is_ok();
        // Drain rather than close the receiver: this unblocks every producer and
        // fences worker/reader teardown, not just the early Shutdown receipt.
        while self.output.recv_blocking().is_ok() {}
        let acknowledged = sent && ack.recv_blocking().is_ok();
        while !self.worker.is_closed() {
            std::thread::yield_now();
        }
        let _ = std::fs::write(
            self.dir.join("cleanup-fenced"),
            if acknowledged { "done" } else { "failed" },
        );
    }
}

struct TestChildGuard {
    child: std::process::Child,
    dir: tempfile::TempDir,
}

impl Drop for TestChildGuard {
    fn drop(&mut self) {
        release_waits(self.dir.path());
        // The isolated test process owns Core and its fake. Let its unwind guard
        // finish Core shutdown rather than killing the parent out from under it.
        let _ = self.child.wait();
    }
}

#[test]
fn closed_child_input_keeps_final_output_admitted_through_real_worker_storage_and_save() {
    isolated_drain(false, false);
}

#[test]
fn closed_child_input_before_successor_state_keeps_final_output_admitted() {
    isolated_drain(true, false);
}

#[test]
fn panicking_drain_fixture_releases_child_waits_and_fences_owned_worker() {
    isolated_drain(true, true);
}

fn isolated_drain(input_first: bool, panic_on_closed: bool) {
    const CHILD: &str = "TAU_INPUT_DRAIN_REVIEW_CHILD";
    if std::env::var_os(CHILD).is_none() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("pi");
        std::fs::write(&fake, r#"#!/usr/bin/env python3
import json, os, sys, time
root = os.environ['TAU_DRAIN_DIR']
prior = False
def emit(value):
    print(json.dumps(value), flush=True)
def wait(name):
    while not os.path.exists(os.path.join(root, name)): time.sleep(.005)
def close_input():
    os.close(0)
    with open(os.path.join(root, 'fd-closed'), 'w') as f: f.write('closed')
def finish():
    wait('final-release')
    emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type':'text', 'text':'final after broken pipe'}], 'stopReason': 'stop'}})
    emit({'type': 'agent_end'})
for line in sys.stdin:
    r = json.loads(line)
    kind = r['type']
    if kind == 'abort' and r.get('id') == 'fixture-close':
        close_input()
        finish()
        break
    successor = kind == 'get_state' and os.path.exists(os.path.join(root, 'refresh'))
    if kind == 'get_state':
        ident = 'b' if successor else 'a'
        data = {'sessionId': ident, 'sessionFile': os.path.join(root, ident + '-' + os.path.basename(os.getcwd()) + '.jsonl'), 'isStreaming': False}
    elif kind == 'get_messages': data = {'messages': []}
    elif kind == 'get_commands': data = {'commands': []}
    else: data = {'models': [], 'levels': []}
    early = successor and os.environ['TAU_DRAIN_INPUT_FIRST'] == '1'
    if early:
        close_input()
        wait('state-release')
    emit({'type': 'response', 'id': r['id'], 'command': kind, 'success': True, 'data': data})
    if kind == 'get_messages' and not prior:
        prior = True
        emit({'type': 'message_end', 'message': {'role': 'assistant', 'content': [{'type':'text', 'text':'prior A completion'}], 'stopReason': 'stop'}})
        emit({'type': 'agent_end'})
    if successor:
        emit({'type': 'agent_start'})
    if early:
        finish()
        break
"#).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let name = if panic_on_closed {
            "panicking_drain_fixture_releases_child_waits_and_fences_owned_worker"
        } else if input_first {
            "closed_child_input_before_successor_state_keeps_final_output_admitted"
        } else {
            "closed_child_input_keeps_final_output_admitted_through_real_worker_storage_and_save"
        };
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("app::lifecycle::tests::input_drain_review_tests::{name}"),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("TAU_PI_PATH", fake)
            .env("PI_CODING_AGENT_DIR", dir.path().join("agent"))
            .env("TAU_DRAIN_DIR", dir.path())
            .env("TAU_DRAIN_INPUT_FIRST", if input_first { "1" } else { "0" })
            .spawn()
            .unwrap();
        let mut guard = TestChildGuard { child, dir };
        let status = guard.child.wait().unwrap();
        assert!(status.success());
        return;
    }
    tau_core::profile::initialize(tau_core::profile::StorageProfile::ephemeral().unwrap()).unwrap();
    // Repeat within one isolated profile/process, with a new owned runtime and
    // fresh handshake files each time; no retry-on-failure or deadline extension.
    for _ in 0..3 {
        if panic_on_closed {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_drain(input_first, true);
            }));
            let error = result.expect_err("fixture must exercise unwind cleanup");
            assert_eq!(
                error.downcast_ref::<&str>(),
                Some(&"intentional fixture panic")
            );
        } else {
            run_drain(input_first, false);
        }
        let fixture = PathBuf::from(std::env::var_os("TAU_DRAIN_DIR").unwrap());
        assert_eq!(
            std::fs::read_to_string(fixture.join("cleanup-fenced")).unwrap(),
            "done"
        );
    }
}

fn run_drain(input_first: bool, panic_on_closed: bool) {
    let fixture = PathBuf::from(std::env::var_os("TAU_DRAIN_DIR").unwrap());
    for name in [
        "refresh",
        "fd-closed",
        "state-release",
        "final-release",
        "cleanup-fenced",
    ] {
        let path = fixture.join(name);
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let p = tau_core::storage::import_project(dir.path().to_string_lossy().into_owned()).unwrap();
    for id in ["a", "b"] {
        std::fs::write(
            fixture.join(format!(
                "{id}-{}.jsonl",
                dir.path().file_name().unwrap().to_str().unwrap()
            )),
            json!({"type":"session", "version":3, "id":id,
            "timestamp":"2026-01-01T00:00:00Z", "cwd":p.working_directory})
            .to_string(),
        )
        .unwrap();
    }
    let (worker, output) = service::launch(dir.path().into());
    let _guard = FakeGuard {
        worker: worker.clone(),
        output: output.clone(),
        dir: fixture.clone(),
    };
    with_shell(512, |s, w, cx, requests| {
        s.workspace = Some(tau_core::storage::load_workspace().unwrap());
        s.create_live(
            "r".into(),
            p.clone(),
            String::new(),
            String::new(),
            "draft".into(),
            w,
            cx,
        );
        let forward = || {
            while let Ok(command) = requests.try_recv() {
                worker.send_blocking(command).unwrap();
            }
        };
        let send_probe = |id: &str, generation| {
            worker
                .send_blocking(Command::Send {
                    runtime: "r".into(),
                    generation,
                    request: json!({"type":"abort", "id":id}),
                })
                .unwrap();
        };
        forward();
        let mut prior_stored = false;
        while !s.sessions["r"].controller.state().ready || !prior_stored {
            let event = next(&output);
            prior_stored |= matches!(&event, Event::RegistrationStored { id, .. } if id == "a");
            s.ingest(event, cx);
            forward();
        }
        assert_eq!(s.sessions["r"].controller.state().identity.id, "a");
        std::fs::write(fixture.join("refresh"), "refresh B").unwrap();
        let generation = s.sessions["r"].controller.generation();
        let effects = s.sessions.get_mut("r").unwrap().controller.refresh();
        s.effects("r", effects, cx);
        forward();
        if !input_first {
            while s.sessions["r"].controller.state().identity.id != "b" {
                let event = next(&output);
                assert!(!matches!(&event, Event::Pi(e) if e.kind == "input_closed"));
                s.ingest(event, cx);
                forward();
            }
            assert_eq!(s.sessions["r"].controller.state().identity.id, "b");
            // This is an actual wire write through the owned Core worker, after
            // shell adoption of B, not a synthetic input_closed/response event.
            send_probe("fixture-close", generation);
        }
        wait_file(&fixture.join("fd-closed"));
        send_probe("fixture-failed-write", generation);
        let mut closed = false;
        let mut failed_write = false;
        while !closed || !failed_write {
            let event = next(&output);
            closed |= matches!(&event, Event::Pi(e) if e.kind == "input_closed");
            failed_write |= matches!(&event, Event::Written { request, success: false, .. }
                if request["id"] == "fixture-failed-write");
            s.ingest(event, cx);
            forward();
        }
        assert_eq!(
            s.sessions["r"].controller.state().identity.id,
            if input_first { "a" } else { "b" }
        );
        assert!(!s.sessions["r"].controller.state().ready);
        assert!(
            s.sessions
                .get_mut("r")
                .unwrap()
                .controller
                .submit("unsafe")
                .is_empty()
        );
        if panic_on_closed {
            panic!("intentional fixture panic");
        }
        if input_first {
            // Force the opposite producer ordering independently: the child has
            // not emitted B until the parent has ingested the input failure.
            std::fs::write(fixture.join("state-release"), "deliver B").unwrap();
            while s.sessions["r"].controller.state().identity.id != "b" {
                let event = next(&output);
                s.ingest(event, cx);
                forward();
            }
        }
        assert_eq!(s.sessions["r"].controller.state().identity.id, "b");
        assert!(!s.sessions["r"].controller.state().ready);
        assert!(
            s.sessions
                .get_mut("r")
                .unwrap()
                .controller
                .submit("unsafe")
                .is_empty()
        );
        // A stale input event must not affect a successor/current generation.
        s.ingest(
            Event::Pi(tau_core::pi::PiEvent {
                runtime_id: "r".into(),
                generation: generation + 1,
                kind: "input_closed",
                line: None,
                message: None,
                code: None,
            }),
            cx,
        );
        std::fs::write(fixture.join("final-release"), "go").unwrap();
        let mut exited = false;
        let mut stored = false;
        while !exited || !stored {
            let event = next(&output);
            exited |= matches!(&event, Event::Pi(e) if e.kind == "exited");
            stored |= matches!(&event, Event::RegistrationStored { id, .. } if id == "b");
            s.ingest(event, cx);
            if exited {
                let effects = s.sessions.get_mut("r").unwrap().controller.prepare_exit();
                s.effects("r", effects, cx);
            }
            forward();
        }
        assert!(
            s.sessions["r"]
                .controller
                .state()
                .transcript
                .iter()
                .any(|row| row.text == "final after broken pipe")
        );
        worker
            .send_blocking(Command::Save {
                revision: 44,
                preferences: None,
                shutdown: true,
            })
            .unwrap();
        loop {
            let event = next(&output);
            let saved = matches!(
                &event,
                Event::Saved {
                    revision: 44,
                    success: true,
                    ..
                }
            );
            s.ingest(event, cx);
            forward();
            if saved {
                break;
            }
        }
        worker
            .send_blocking(Command::ExitFence { revision: 44 })
            .unwrap();
        assert!(matches!(next(&output), Event::ExitDrained { revision: 44 }));
        assert!(
            tau_core::storage::load_workspace()
                .unwrap()
                .projects
                .iter()
                .find(|project| project.path == p.path)
                .unwrap()
                .sessions
                .iter()
                .any(|row| row.id == "b")
        );
    });
}
