mod environment;
mod ownership;
mod process;
mod transport;
use super::*;
use crate::telemetry::Telemetry;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use std::sync::mpsc::{Receiver, TryRecvError};
use tauri::{Listener, Manager};

const TEST_SCENARIO_ENV: &str = "TAU_PI_TEST_SCENARIO";
const TEST_FAULT_ENV: &str = "TAU_PI_TEST_FAULT";
const TEST_PROJECT_ENV: &str = "TAU_PI_TEST_PROJECT_PATH";
const TEST_SESSION_ENV: &str = "TAU_PI_TEST_SESSION_PATH";
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);
static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

struct EnvironmentGuard {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl EnvironmentGuard {
    fn set(values: &[(&'static str, Option<&OsStr>)]) -> Self {
        let previous = values
            .iter()
            .map(|(key, _)| (*key, env::var_os(key)))
            .collect();
        for (key, value) in values {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
        Self { previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}

struct NativePiFixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    session: PathBuf,
    _environment: EnvironmentGuard,
}

impl NativePiFixture {
    fn new(fault: Option<&OsStr>) -> Self {
        let root = tempfile::tempdir().expect("native Pi fixture directory");
        let project = root.path().join("project");
        std::fs::create_dir(&project).expect("fixture project");
        let project = project.canonicalize().expect("canonical fixture project");
        let session = project.join("session.jsonl");
        std::fs::write(&session, "").expect("fixture session");
        let session = session.canonicalize().expect("canonical fixture session");
        let adapter = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../scripts/pi-stdio-adapter.ts")
            .canonicalize()
            .expect("fake Pi adapter");
        #[cfg(dev)]
        let session_dir =
            Some(crate::storage::default_session_dir(&project.to_string_lossy()).unwrap());
        #[cfg(not(dev))]
        let session_dir: Option<PathBuf> = None;
        let environment = EnvironmentGuard::set(&[
            ("TAU_PI_PATH", Some(adapter.as_os_str())),
            (
                TEST_SCENARIO_ENV,
                Some(OsStr::new("saved-session-conversation")),
            ),
            (TEST_FAULT_ENV, fault),
            (TEST_PROJECT_ENV, Some(project.as_os_str())),
            (TEST_SESSION_ENV, Some(session.as_os_str())),
            (
                "TAU_PI_TEST_SESSION_DIR",
                session_dir.as_deref().map(Path::as_os_str),
            ),
        ]);
        Self {
            _root: root,
            project,
            session,
            _environment: environment,
        }
    }

    fn app_and_events(
        &self,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        Receiver<serde_json::Value>,
    ) {
        let telemetry_dir = self._root.path().join("telemetry");
        let app = tauri::test::mock_builder()
            .manage(PiState::default())
            .manage(Telemetry::for_test(telemetry_dir))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app");
        let (sender, receiver) = mpsc::channel();
        app.listen("pi-event", move |event| {
            let value = serde_json::from_str(event.payload()).expect("serialized Pi event");
            let _ = sender.send(value);
        });
        (app, receiver)
    }

    fn project_string(&self) -> String {
        self.project.to_string_lossy().into_owned()
    }

    fn session_string(&self) -> String {
        self.session.to_string_lossy().into_owned()
    }
}

fn claim_for_test(state: &PiState, owner_id: &str) {
    claim_pi_frontend_inner(&state.inner, owner_id.into(), 0).expect("claim Pi owner");
}

fn expect_outer_event(
    events: &Receiver<serde_json::Value>,
    kind: &str,
    generation: u64,
) -> serde_json::Value {
    let event = events
        .recv_timeout(EVENT_TIMEOUT)
        .unwrap_or_else(|error| panic!("waiting for {kind} Pi event: {error}"));
    assert_eq!(event["generation"], generation);
    assert_eq!(event["kind"], kind);
    event
}

fn expect_rpc(
    events: &Receiver<serde_json::Value>,
    generation: u64,
    rpc_type: &str,
    id: Option<&str>,
) {
    let event = expect_outer_event(events, "rpc", generation);
    let line: serde_json::Value =
        serde_json::from_str(event["line"].as_str().expect("RPC event line"))
            .expect("production-shaped RPC JSON");
    assert_eq!(line["type"], rpc_type);
    if let Some(id) = id {
        assert_eq!(line["id"], id);
    }
}

fn eof_process(generation: u64) -> PiProcess {
    scripted_process(
        "while IFS= read -r line; do :; done; sleep 0.05; exit 0",
        generation,
    )
}

fn assert_clean_exit(process: &PiProcess) {
    assert!(process
        .child
        .lock()
        .expect("child lock")
        .try_wait()
        .expect("child status")
        .expect("child exited")
        .success());
    assert!(!process.usable.load(Ordering::Acquire));
    assert!(process.writer.lock().expect("writer lock").is_none());
}

fn sleeping_process(generation: u64) -> PiProcess {
    scripted_process("exec sleep 60", generation)
}

fn scripted_process(script: &str, generation: u64) -> PiProcess {
    let mut child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .spawn()
        .expect("scripted process");
    let stdin = child.stdin.take().expect("scripted stdin");
    PiProcess {
        owner_id: "owner-a".into(),
        generation,
        child: Arc::new(Mutex::new(child)),
        writer: spawn_stdin_writer(stdin).expect("stdin writer"),
        usable: Arc::new(AtomicBool::new(true)),
    }
}
