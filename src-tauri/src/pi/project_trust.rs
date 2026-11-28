use super::{
    environment::{
        capture_success, configure_child_path, find_on_path, resolve_node_binary,
        resolve_pi_binary, CaptureFailure,
    },
    ownership::{PiManager, PiState},
};
use crate::{
    models::ProjectRecord,
    ssh::{remote_posix_login_command, SshConnection},
    storage::registered_project,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;

#[cfg(all(test, unix))]
mod tests;

pub(super) const PRIVATE_FRAME: &[u8] = b"__TAU_PI_TRUST__";
const SOURCE: &str = include_str!("../../pi-extensions/project-trust.mjs");
const LOCAL_TIMEOUT: Duration = Duration::from_secs(5);
const SSH_TIMEOUT: Duration = Duration::from_secs(12);
const SSH_CONFIG_TIMEOUT: Duration = Duration::from_secs(1);
const INSPECTION_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_INSPECTIONS: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustTarget {
    folder: String,
    destination: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustRuntime {
    owner_id: String,
    runtime_id: String,
    generation: u64,
    session_id: String,
    session_file: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustResult {
    target: TrustTarget,
    saved: bool,
    exact: bool,
    inspection_token: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TrustError {
    Mismatch,
    Unsupported,
    Unavailable,
    Unconfirmed,
}

type TrustReply<T> = Result<T, TrustError>;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HelperReply {
    Success {
        folder: String,
        saved: bool,
        exact: bool,
        identity: Value,
    },
    Failure(TrustError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Location {
    Local(String),
    Remote { folder: String, connection: String },
}

impl From<ProjectRecord> for Location {
    fn from(record: ProjectRecord) -> Self {
        match record.remote {
            Some(remote) => Self::Remote {
                folder: remote.working_directory,
                connection: remote.connection_string,
            },
            None => Self::Local(record.path),
        }
    }
}

impl Location {
    fn folder(&self) -> &str {
        match self {
            Self::Local(folder) | Self::Remote { folder, .. } => folder,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Observation {
    nonce: String,
    cwd: String,
    session_id: String,
    session_file: Option<String>,
    trusted: bool,
    identity: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SshConfig {
    digest: [u8; 32],
    executable: PathBuf,
    executable_stamp: SshExecutableStamp,
    destination: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SshExecutableStamp {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    unix: (u64, u64, i64, i64),
}

impl SshExecutableStamp {
    fn read(path: &Path) -> Option<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        let info = fs::metadata(path).ok()?;
        Some(Self {
            len: info.len(),
            modified: info.modified().ok()?,
            #[cfg(unix)]
            unix: (info.dev(), info.ino(), info.ctime(), info.ctime_nsec()),
        })
    }
}

impl SshConfig {
    fn read(command: &Command) -> TrustReply<Self> {
        let command = SshConnection::trust_config_command(command);
        let executable = Path::new(command.get_program())
            .canonicalize()
            .map_err(|_| TrustError::Unavailable)?;
        let stamp = SshExecutableStamp::read(&executable).ok_or(TrustError::Unavailable)?;
        let config =
            capture_success(command, SSH_CONFIG_TIMEOUT).map_err(|_| TrustError::Unavailable)?;
        if SshExecutableStamp::read(&executable).as_ref() != Some(&stamp) {
            return Err(TrustError::Mismatch);
        }
        let mut digest = Sha256::new();
        for line in config.split_inclusive(|byte| *byte == b'\n') {
            let option = line.split(|byte| *byte == b' ').next().unwrap_or_default();
            // The trust connection tightens host-key checking and disables forwarding/TTY.
            if !matches!(
                option,
                b"stricthostkeychecking"
                    | b"requesttty"
                    | b"clearallforwardings"
                    | b"forwardagent"
                    | b"forwardx11"
                    | b"localforward"
                    | b"remoteforward"
                    | b"dynamicforward"
                    | b"batchmode"
                    | b"connecttimeout"
                    | b"connectionattempts"
            ) {
                digest.update(line);
            }
        }
        Ok(Self {
            digest: digest.finalize().into(),
            executable,
            executable_stamp: stamp,
            destination: display_destination(&config).ok_or(TrustError::Unavailable)?,
        })
    }
}

#[derive(Clone)]
pub(super) struct TrustLaunch {
    nonce: String,
    location: Location,
    ssh: Option<SshConfig>,
    observed: Option<Observation>,
    session_revision: u64,
}

impl TrustLaunch {
    pub(super) fn local(command: &mut Command, pi: &Path, folder: String) -> Self {
        let launch = Self::new(Location::Local(folder), None);
        command
            .env("TAU_PROJECT_TRUST_NONCE", launch.nonce())
            .env("TAU_PROJECT_TRUST_CLI", pi);
        launch
    }

    pub(super) fn remote(raw: String, folder: String) -> Self {
        Self::new(
            Location::Remote {
                folder,
                connection: raw,
            },
            None,
        )
    }

    pub(super) fn capture_ssh_config(&mut self, command: &Command) {
        self.ssh = SshConfig::read(command).ok();
    }

    fn new(location: Location, ssh: Option<SshConfig>) -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            location,
            ssh,
            observed: None,
            session_revision: 0,
        }
    }

    pub(super) fn folder(&self) -> &str {
        self.location.folder()
    }

    pub(super) fn nonce(&self) -> &str {
        &self.nonce
    }

    pub(super) fn observe(&mut self, frame: &[u8]) {
        if frame.len() > 64 * 1024 {
            return;
        }
        let Ok(mut observation) = serde_json::from_slice::<Observation>(frame) else {
            return;
        };
        if observation.nonce != self.nonce || observation.session_id.is_empty() {
            return;
        }
        if let Some(previous) = &self.observed {
            if previous.session_id != observation.session_id
                || previous.session_file != observation.session_file
                || previous.cwd != observation.cwd
                || previous.trusted != observation.trusted
            {
                self.session_revision += 1;
            }
            // Reload and session replacement can update session evidence, not the launch identity.
            observation.identity.clone_from(&previous.identity);
        }
        self.observed = Some(observation);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveBinding {
    identity: Value,
    ssh: Option<SshConfig>,
    session_revision: u64,
}

fn live_binding(
    manager: &Mutex<PiManager>,
    runtime: &TrustRuntime,
    location: &Location,
    runtime_cwd: &str,
) -> TrustReply<LiveBinding> {
    let manager = manager.lock().map_err(|_| TrustError::Unavailable)?;
    manager
        .require_owner(&runtime.owner_id)
        .map_err(|_| TrustError::Mismatch)?;
    let process = manager
        .processes
        .get(&runtime.runtime_id)
        .filter(|process| {
            process.owner_id == runtime.owner_id
                && process.generation == runtime.generation
                && process.usable.load(Ordering::Acquire)
        })
        .ok_or(TrustError::Mismatch)?;
    if process
        .child
        .lock()
        .map_err(|_| TrustError::Unavailable)?
        .try_wait()
        .map_err(|_| TrustError::Unavailable)?
        .is_some()
    {
        return Err(TrustError::Mismatch);
    }
    let launch = process.trust.as_ref().ok_or(TrustError::Unsupported)?;
    if &launch.location != location {
        return Err(TrustError::Mismatch);
    }
    let observed = launch.observed.as_ref().ok_or(TrustError::Unsupported)?;
    if observed.trusted
        || observed.session_id != runtime.session_id
        || observed.session_file != runtime.session_file
        || observed.cwd != runtime_cwd
    {
        return Err(TrustError::Mismatch);
    }
    let identity = observed
        .identity
        .as_ref()
        .filter(|item| item.is_object())
        .ok_or(TrustError::Unsupported)?;
    let peer = identity
        .get("ssh")
        .and_then(Value::as_str)
        .ok_or(TrustError::Unsupported)?;
    if matches!(location, Location::Remote { .. }) && (peer.is_empty() || launch.ssh.is_none()) {
        return Err(TrustError::Unsupported);
    }
    Ok(LiveBinding {
        identity: identity.clone(),
        ssh: launch.ssh.clone(),
        session_revision: launch.session_revision,
    })
}

struct Inspection {
    at: Instant,
    project_key: String,
    location: Location,
    runtime_cwd: String,
    runtime: TrustRuntime,
    session_revision: u64,
    target: TrustTarget,
    identity: Value,
}

#[derive(Default)]
pub struct ProjectTrustState(Mutex<HashMap<String, Inspection>>);

impl ProjectTrustState {
    fn record(&self, inspection: Inspection) -> TrustReply<String> {
        let mut records = self.0.lock().map_err(|_| TrustError::Unavailable)?;
        records.retain(|_, entry| entry.at.elapsed() < INSPECTION_TTL);
        if records.len() >= MAX_INSPECTIONS {
            if let Some(oldest) = records
                .iter()
                .min_by_key(|(_, entry)| entry.at)
                .map(|(key, _)| key.clone())
            {
                records.remove(&oldest);
            }
        }
        let token = uuid::Uuid::new_v4().to_string();
        records.insert(token.clone(), inspection);
        Ok(token)
    }

    fn take(
        &self,
        token: &str,
        project_key: &str,
        location: &Location,
        runtime_cwd: &str,
        runtime: &TrustRuntime,
        target: &TrustTarget,
    ) -> TrustReply<Inspection> {
        let mut records = self.0.lock().map_err(|_| TrustError::Unavailable)?;
        let previously_inspected = records.get(token).is_some_and(|entry| {
            entry.at.elapsed() < INSPECTION_TTL
                && entry.project_key == project_key
                && &entry.location == location
                && entry.runtime_cwd == runtime_cwd
                && &entry.runtime == runtime
                && &entry.target == target
        });
        if !previously_inspected {
            return Err(TrustError::Mismatch);
        }
        records.remove(token).ok_or(TrustError::Mismatch)
    }
}

#[tauri::command]
pub async fn inspect_project_trust(
    state: State<'_, ProjectTrustState>,
    pi: State<'_, PiState>,
    project_path: String,
    runtime_cwd: String,
    runtime: TrustRuntime,
) -> TrustReply<TrustResult> {
    let manager = Arc::clone(&pi.inner);
    let (mut result, inspection) = tauri::async_runtime::spawn_blocking(move || {
        let location = lookup(&project_path, &runtime_cwd)?;
        let context = TrustCall {
            manager: &manager,
            project_key: &project_path,
            location: &location,
            runtime_cwd: &runtime_cwd,
            runtime: &runtime,
        };
        let (result, live) = context.run(None)?;
        let inspection = Inspection {
            at: Instant::now(),
            project_key: project_path,
            location,
            runtime_cwd,
            runtime,
            session_revision: live.session_revision,
            target: result.target.clone(),
            identity: live.identity,
        };
        Ok((result, inspection))
    })
    .await
    .map_err(|_| TrustError::Unavailable)??;
    result.inspection_token = Some(state.record(inspection)?);
    Ok(result)
}

#[tauri::command]
pub async fn grant_project_trust(
    state: State<'_, ProjectTrustState>,
    pi: State<'_, PiState>,
    project_path: String,
    runtime_cwd: String,
    runtime: TrustRuntime,
    expected_target: TrustTarget,
    inspection_token: String,
) -> TrustReply<TrustResult> {
    let manager = Arc::clone(&pi.inner);
    let location = tauri::async_runtime::spawn_blocking({
        let (project_path, runtime_cwd) = (project_path.clone(), runtime_cwd.clone());
        move || lookup(&project_path, &runtime_cwd)
    })
    .await
    .map_err(|_| TrustError::Unavailable)??;
    let inspection = state.take(
        &inspection_token,
        &project_path,
        &location,
        &runtime_cwd,
        &runtime,
        &expected_target,
    )?;
    tauri::async_runtime::spawn_blocking(move || {
        TrustCall {
            manager: &manager,
            project_key: &project_path,
            location: &location,
            runtime_cwd: &runtime_cwd,
            runtime: &runtime,
        }
        .run(Some(&inspection))
        .map(|reply| reply.0)
    })
    .await
    .map_err(|_| TrustError::Unconfirmed)?
}

fn lookup(key: &str, runtime_cwd: &str) -> TrustReply<Location> {
    if key.len() > 16_384
        || runtime_cwd.len() > 8_192
        || !Path::new(runtime_cwd).is_absolute()
        || key.contains('\0')
        || runtime_cwd.contains('\0')
    {
        return Err(TrustError::Mismatch);
    }
    registered_project(key)
        .map_err(|_| TrustError::Unavailable)?
        .map(Location::from)
        .ok_or(TrustError::Mismatch)
}

struct HelperCommand {
    command: Command,
    destination: Option<String>,
    timeout: Duration,
}

struct TrustCall<'a> {
    manager: &'a Mutex<PiManager>,
    project_key: &'a str,
    location: &'a Location,
    runtime_cwd: &'a str,
    runtime: &'a TrustRuntime,
}

impl TrustCall<'_> {
    fn binding(&self) -> TrustReply<LiveBinding> {
        live_binding(self.manager, self.runtime, self.location, self.runtime_cwd)
    }

    fn run(&self, inspection: Option<&Inspection>) -> TrustReply<(TrustResult, LiveBinding)> {
        let live = self.binding()?;
        if inspection.is_some_and(|item| {
            item.identity != live.identity || item.session_revision != live.session_revision
        }) {
            return Err(TrustError::Mismatch);
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        let request = serde_json::json!({
            "operation": if inspection.is_some() { "grant" } else { "inspect" },
            "projectFolder": self.location.folder(),
            "runtimeCwd": self.runtime_cwd,
            "startup": &live.identity,
            "previous": inspection.map(|item| &item.identity),
            "nonce": nonce,
        })
        .to_string();
        let node = live
            .identity
            .get("node")
            .and_then(Value::as_str)
            .ok_or(TrustError::Unsupported)?;
        let helper = match self.location {
            Location::Local(folder) => local_command(folder, &request, node)?,
            Location::Remote { folder, connection } => {
                let connection =
                    SshConnection::parse(connection).map_err(|_| TrustError::Unavailable)?;
                remote_command(
                    folder,
                    &connection,
                    &request,
                    &nonce,
                    node,
                    live.ssh.as_ref().ok_or(TrustError::Unsupported)?,
                )?
            }
        };
        if lookup(self.project_key, self.runtime_cwd)? != *self.location
            || self.binding()? != live
            || inspection.is_some_and(|item| helper.destination != item.target.destination)
        {
            return Err(TrustError::Mismatch);
        }
        let reply = invoke_helper(helper, &nonce, inspection)?;
        if self.binding().as_ref() != Ok(&live)
            || lookup(self.project_key, self.runtime_cwd).as_ref() != Ok(self.location)
            || reply.1 != live.identity
        {
            return Err(if inspection.is_some() {
                TrustError::Unconfirmed
            } else {
                TrustError::Mismatch
            });
        }
        Ok((reply.0, live))
    }
}

fn invoke_helper(
    mut helper: HelperCommand,
    nonce: &str,
    inspection: Option<&Inspection>,
) -> TrustReply<(TrustResult, Value)> {
    let reject = if inspection.is_some() {
        TrustError::Unconfirmed
    } else {
        TrustError::Unavailable
    };
    helper.command.env_remove("NODE_PATH");
    let output = capture_success(helper.command, helper.timeout).map_err(|error| match error {
        CaptureFailure::NotStarted => TrustError::Unavailable,
        CaptureFailure::Unknown => reject,
    })?;
    let (folder, saved, exact, identity) = decode_helper_reply(&output, nonce).ok_or(reject)??;
    let target = TrustTarget {
        folder,
        destination: helper.destination,
    };
    if inspection
        .is_some_and(|item| item.target != target || item.identity != identity || !saved || !exact)
    {
        return Err(TrustError::Unconfirmed);
    }
    Ok((
        TrustResult {
            target,
            saved,
            exact,
            inspection_token: None,
        },
        identity,
    ))
}

fn decode_helper_reply(
    output: &[u8],
    nonce: &str,
) -> Option<TrustReply<(String, bool, bool, Value)>> {
    let prefix = format!("__TAU_PROJECT_TRUST_{nonce}__");
    let mut frames = output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(prefix.as_bytes()));
    let frame = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    match serde_json::from_slice::<HelperReply>(frame).ok()? {
        HelperReply::Success {
            folder,
            saved,
            exact,
            identity,
        } if (!exact || saved)
            && identity.get("folder").and_then(Value::as_str) == Some(&folder) =>
        {
            Some(Ok((folder, saved, exact, identity)))
        }
        HelperReply::Success { .. } => None,
        HelperReply::Failure(error) => Some(Err(error)),
    }
}

fn local_command(folder: &str, request: &str, expected_node: &str) -> TrustReply<HelperCommand> {
    let folder = Path::new(folder)
        .canonicalize()
        .map_err(|_| TrustError::Mismatch)?;
    if !folder.is_dir() {
        return Err(TrustError::Mismatch);
    }
    if ["NODE_OPTIONS", "PI_PACKAGE_DIR"]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Err(TrustError::Unsupported);
    }
    let pi = resolve_pi_binary()
        .filter(|path| path.is_absolute())
        .ok_or(TrustError::Unsupported)?;
    let node = resolve_node_binary();
    let mut executables = vec![pi.as_path()];
    if let Some(node) = node.as_deref() {
        executables.push(node);
    }
    let mut command = Command::new("node");
    configure_child_path(&mut command, &executables).map_err(|_| TrustError::Unavailable)?;
    let runtime_path = command
        .get_envs()
        .find_map(|(key, value)| (key == "PATH").then_some(value))
        .flatten();
    let absolute_path = env::join_paths(
        env::split_paths(runtime_path.ok_or(TrustError::Unsupported)?)
            .map(|part| folder.join(part)),
    )
    .map_err(|_| TrustError::Unsupported)?;
    let runtime_node = find_on_path("node", Some(&absolute_path)).ok_or(TrustError::Unsupported)?;
    let helper_node = runtime_node
        .canonicalize()
        .map_err(|_| TrustError::Unsupported)?;
    if helper_node.starts_with(&folder) || !helper_node.is_absolute() {
        return Err(TrustError::Unsupported);
    }
    if helper_node != Path::new(expected_node) {
        return Err(TrustError::Mismatch);
    }
    let mut helper = Command::new(helper_node);
    helper.envs(
        command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key, value))),
    );
    helper
        .args([
            "--input-type=module",
            "--eval",
            SOURCE,
            "--",
            "tau-project-trust",
        ])
        .arg(pi)
        .arg(request)
        .arg(runtime_node);
    Ok(HelperCommand {
        command: helper,
        destination: None,
        timeout: LOCAL_TIMEOUT,
    })
}

fn remote_command(
    folder: &str,
    connection: &SshConnection,
    request: &str,
    nonce: &str,
    node: &str,
    launch: &SshConfig,
) -> TrustReply<HelperCommand> {
    let command = connection.trust_command(&remote_script(folder, request, nonce, node));
    if &SshConfig::read(&command)? != launch {
        return Err(TrustError::Mismatch);
    }
    Ok(HelperCommand {
        command,
        destination: Some(launch.destination.clone()),
        timeout: SSH_TIMEOUT,
    })
}

fn remote_script(folder: &str, request: &str, nonce: &str, node: &str) -> String {
    // Resolve from the runtime's login shell and cwd, but never execute project Node.
    let script = r#"nonce=$4
fail() { printf '\n__TAU_PROJECT_TRUST_%s__{"kind":"%s"}\n' "$nonce" "$1"; exit 0; }
[ -z "${NODE_OPTIONS:-}" ] && [ -z "${PI_PACKAGE_DIR:-}" ] || fail unsupported
unset CDPATH
cd -P "$1" && project=$(pwd -P) || fail mismatch
pi=$(command -v pi) && runtime_node=$(command -v node) || fail unsupported
case "$pi" in /*) ;; *) fail unsupported ;; esac
case "$runtime_node" in /*) ;; *) fail unsupported ;; esac
if [ -x /usr/bin/realpath ]; then
  node=$(/usr/bin/realpath "$runtime_node") || fail unsupported
elif [ -x /bin/realpath ]; then
  node=$(/bin/realpath "$runtime_node") || fail unsupported
else
  node=$(/usr/bin/readlink -f "$runtime_node") || fail unsupported
fi
case "$node" in "$project"|"${project%/}"/*) fail unsupported ;; /*) ;; *) fail unsupported ;; esac
[ "$node" = "$5" ] || fail mismatch
cd / || fail unsupported
unset NODE_PATH
exec "$node" --input-type=module --eval "$2" -- tau-project-trust "$pi" "$3" "$runtime_node""#;
    remote_posix_login_command(script, &[folder, SOURCE, request, nonce, node])
}

fn display_destination(config: &[u8]) -> Option<String> {
    let config = std::str::from_utf8(config).ok()?;
    let field = |key| {
        config.lines().find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            (name == key
                && !value.is_empty()
                && value.len() <= 255
                && !value
                    .chars()
                    .any(|ch| ch.is_control() || ch.is_whitespace()))
            .then_some(value)
        })
    };
    let user = field("user")?;
    let host = field("hostname")?;
    let port = field("port")?.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }
    if port == 22 {
        Some(format!("{user}@{host}"))
    } else if host.contains(':') && !host.starts_with('[') {
        Some(format!("{user}@[{host}]:{port}"))
    } else {
        Some(format!("{user}@{host}:{port}"))
    }
}
