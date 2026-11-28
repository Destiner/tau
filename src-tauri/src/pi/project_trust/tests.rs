use super::*;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};

const NONCE: &str = "d6a5042a-2216-4653-9ec1-a008cd0c7710";
const SDK: &str = r#"
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
export const getAgentDir = () => process.env.PI_CODING_AGENT_DIR;
export class ProjectTrustStore {
  constructor(dir) {
    mkdirSync(dir, { recursive: true });
    this.file = join(dir, 'trust.json');
  }
  data() {
    try { return JSON.parse(readFileSync(this.file, 'utf8')); }
    catch (e) { if (e.code === 'ENOENT') return {}; throw e; }
  }
  getEntry(path) {
    const decisions = this.data();
    for (;;) {
      if (Object.hasOwn(decisions, path)) return { path, decision: decisions[path] };
      if (dirname(path) === path) return null;
      path = dirname(path);
    }
  }
  set(path, decision) {
    writeFileSync(this.file, JSON.stringify({ ...this.data(), [path]: decision }));
    if (process.env.TEST_FAIL_AFTER_WRITE) throw new Error('private SDK failure');
  }
}
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
    parent: PathBuf,
    sibling: PathBuf,
    node: PathBuf,
    pi: PathBuf,
    agent: PathBuf,
}

impl Fixture {
    fn new() -> Option<Self> {
        let node = resolve_node_binary()?.canonicalize().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("isolated-home");
        fs::create_dir(&home).unwrap();
        let parent = root.join("projects");
        let project = parent.join("A folder's !project\nname");
        let sibling = parent.join("sibling");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir(&sibling).unwrap();
        let installation = root.join("install");
        fs::create_dir(&installation).unwrap();
        fs::write(
            installation.join("package.json"),
            r#"{"name":"@earendil-works/pi-coding-agent","version":"1.5.2","bin":{"pi":"cli.js"},"exports":{".":{"import":"./sdk.mjs"}}}"#,
        )
        .unwrap();
        fs::write(installation.join("sdk.mjs"), SDK).unwrap();
        let bin = root.join("fixture-bin");
        fs::create_dir(&bin).unwrap();
        let pi = bin.join("pi");
        let launcher = installation.join("cli.js");
        executable(&launcher, "#!/usr/bin/env node\n");
        symlink(launcher, &pi).unwrap();
        symlink(&node, bin.join("node")).unwrap();
        Some(Self {
            _dir: dir,
            root: root.clone(),
            home,
            project,
            parent,
            sibling,
            node,
            pi,
            agent: root.join("custom agent"),
        })
    }

    fn helper(&self, pi: &Path, request: &Value) -> HelperCommand {
        let mut command = Command::new(&self.node);
        command
            .args([
                "--input-type=module",
                "--eval",
                SOURCE,
                "--",
                "tau-project-trust",
            ])
            .arg(pi)
            .arg(request.to_string())
            .arg(&self.node)
            .env_remove("NODE_OPTIONS")
            .env("HOME", &self.home)
            .env("PI_CODING_AGENT_DIR", &self.agent);
        HelperCommand {
            command,
            destination: None,
            timeout: LOCAL_TIMEOUT,
        }
    }

    fn inspect(&self, project: &Path, cwd: &Path) -> TrustReply<(TrustResult, Value)> {
        invoke_helper(
            self.helper(&self.pi, &request(project, cwd, None)),
            NONCE,
            None,
        )
    }

    fn grant(&self, project: &Path, inspection: &Inspection) -> TrustReply<(TrustResult, Value)> {
        invoke_helper(
            self.helper(
                &self.pi,
                &request(
                    project,
                    Path::new(&inspection.runtime_cwd),
                    Some(inspection),
                ),
            ),
            NONCE,
            Some(inspection),
        )
    }

    fn trust_file(&self) -> PathBuf {
        self.agent.join("trust.json")
    }
}

fn executable(path: &Path, source: &str) {
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn request(folder: &Path, cwd: &Path, inspection: Option<&Inspection>) -> Value {
    json!({
        "operation": if inspection.is_some() { "grant" } else { "inspect" },
        "projectFolder": folder,
        "runtimeCwd": cwd,
        "previous": inspection.map(|item| &item.identity),
        "nonce": NONCE,
    })
}

fn inspection(location: Location, cwd: &Path, reply: (TrustResult, Value)) -> Inspection {
    Inspection {
        at: Instant::now(),
        location,
        runtime_cwd: cwd.to_str().unwrap().into(),
        project_key: "project".into(),
        runtime: runtime(),
        session_revision: 0,
        target: reply.0.target,
        identity: reply.1,
    }
}

#[test]
fn exact_folder_alias_inheritance_and_sibling_isolation() {
    let Some(f) = Fixture::new() else { return };
    let alias = f.root.join("alias ' !");
    symlink(&f.project, &alias).unwrap();
    assert_eq!(
        f.inspect(&alias, &f.sibling).unwrap_err(),
        TrustError::Mismatch
    );
    let first = f.inspect(&alias, &f.project).unwrap();
    assert!(!first.0.saved);
    assert_eq!(first.0.target.folder, f.project.to_str().unwrap());
    assert_eq!(first.1["agentDir"], f.agent.to_str().unwrap());
    assert!(!f.trust_file().exists());
    assert!(!f.home.join(".pi/agent/trust.json").exists());
    let first = inspection(
        Location::Local(alias.to_str().unwrap().into()),
        &f.project,
        first,
    );
    assert!(f.grant(&alias, &first).unwrap().0.saved);
    assert!(!f.inspect(&f.sibling, &f.sibling).unwrap().0.saved);
    let child = f.project.join("child");
    fs::create_dir(&child).unwrap();
    let inherited = f.inspect(&child, &child).unwrap().0;
    assert!(inherited.saved);
    assert!(!inherited.exact);
    assert!(f.inspect(&f.project, &f.project).unwrap().0.exact);
    let data: Value = serde_json::from_slice(&fs::read(f.trust_file()).unwrap()).unwrap();
    assert_eq!(data, json!({ f.project.to_str().unwrap(): true }));

    fs::write(
        f.trust_file(),
        json!({ f.parent.to_str().unwrap(): true, f.project.to_str().unwrap(): false }).to_string(),
    )
    .unwrap();
    assert!(!f.inspect(&child, &child).unwrap().0.saved);
    assert!(f.inspect(&f.sibling, &f.sibling).unwrap().0.saved);
    let denied = inspection(
        Location::Local(alias.to_str().unwrap().into()),
        &f.project,
        f.inspect(&alias, &f.project).unwrap(),
    );
    assert!(f.grant(&alias, &denied).unwrap().0.saved);
    assert!(f.inspect(&child, &child).unwrap().0.saved);
}

#[test]
fn moved_replaced_or_missing_folder_and_changed_agent_dir_do_not_write() {
    let Some(f) = Fixture::new() else { return };
    let alias = f.root.join("alias");
    symlink(&f.project, &alias).unwrap();
    let record = inspection(
        Location::Local(alias.to_str().unwrap().into()),
        &f.project,
        f.inspect(&alias, &f.project).unwrap(),
    );
    let mut helper = f.helper(&f.pi, &request(&alias, &f.project, Some(&record)));
    let other_agent = f.root.join("another agent");
    fs::create_dir(&other_agent).unwrap();
    helper.command.env("PI_CODING_AGENT_DIR", &other_agent);
    assert_eq!(
        invoke_helper(helper, NONCE, Some(&record)).unwrap_err(),
        TrustError::Mismatch
    );
    assert!(!other_agent.join("trust.json").exists());
    fs::remove_file(&alias).unwrap();
    assert_eq!(f.grant(&alias, &record).unwrap_err(), TrustError::Mismatch);
    symlink(&f.sibling, &alias).unwrap();
    assert_eq!(f.grant(&alias, &record).unwrap_err(), TrustError::Mismatch);
    fs::rename(&f.project, f.project.with_extension("old")).unwrap();
    fs::create_dir(&f.project).unwrap();
    fs::remove_file(&alias).unwrap();
    symlink(&f.project, &alias).unwrap();
    assert_eq!(f.grant(&alias, &record).unwrap_err(), TrustError::Mismatch);
    assert!(!f.trust_file().exists());
}

#[test]
fn malformed_store_and_symlink_are_not_overwritten_and_post_write_error_is_unconfirmed() {
    let Some(f) = Fixture::new() else { return };
    let record = inspection(
        Location::Local(f.project.to_str().unwrap().into()),
        &f.project,
        f.inspect(&f.project, &f.project).unwrap(),
    );
    let file = f.trust_file();
    fs::write(&file, "private malformed JSON").unwrap();
    assert_eq!(
        f.grant(&f.project, &record).unwrap_err(),
        TrustError::Unavailable
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "private malformed JSON");
    fs::remove_file(&file).unwrap();
    let unrelated = f.root.join("unrelated");
    fs::write(&unrelated, "{}").unwrap();
    symlink(&unrelated, &file).unwrap();
    assert_eq!(
        f.grant(&f.project, &record).unwrap_err(),
        TrustError::Unavailable
    );
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "{}");
    fs::remove_file(&file).unwrap();
    let mut helper = f.helper(&f.pi, &request(&f.project, &f.project, Some(&record)));
    helper.command.env("TEST_FAIL_AFTER_WRITE", "1");
    assert_eq!(
        invoke_helper(helper, NONCE, Some(&record)).unwrap_err(),
        TrustError::Unconfirmed
    );
    assert!(f.inspect(&f.project, &f.project).unwrap().0.saved);
}

#[test]
fn startup_identity_changes_are_rejected_before_sdk_import() {
    for change in ["folder", "agent", "sdk", "installation"] {
        let Some(f) = Fixture::new() else { return };
        fs::create_dir(&f.agent).unwrap();
        let marker = f.root.join("sdk-imported");
        let sdk = f.root.join("install/sdk.mjs");
        let marked_sdk = format!(
            "import {{ writeFileSync as mark }} from 'node:fs'; mark({:?}, 'yes');\n{}",
            marker.to_str().unwrap(),
            SDK
        );
        fs::write(&sdk, &marked_sdk).unwrap();
        let baseline = f.inspect(&f.project, &f.project).unwrap().1;
        fs::remove_file(&marker).unwrap();
        match change {
            "folder" => {
                fs::rename(&f.project, f.project.with_extension("old")).unwrap();
                fs::create_dir(&f.project).unwrap();
            }
            "agent" => {
                fs::rename(&f.agent, f.agent.with_extension("old")).unwrap();
                fs::create_dir(&f.agent).unwrap();
            }
            "sdk" => {
                fs::rename(&sdk, sdk.with_extension("old")).unwrap();
                fs::write(&sdk, &marked_sdk).unwrap();
            }
            "installation" => {
                let install = f.root.join("install");
                fs::rename(&install, f.root.join("old-install")).unwrap();
                fs::create_dir(&install).unwrap();
                fs::copy(
                    f.root.join("old-install/package.json"),
                    install.join("package.json"),
                )
                .unwrap();
                fs::write(&sdk, &marked_sdk).unwrap();
                executable(&install.join("cli.js"), "#!/usr/bin/env node\n");
            }
            _ => unreachable!(),
        }
        let mut req = request(&f.project, &f.project, None);
        req["startup"] = baseline;
        assert_eq!(
            invoke_helper(f.helper(&f.pi, &req), NONCE, None).unwrap_err(),
            TrustError::Mismatch,
            "{change}"
        );
        assert!(!marker.exists(), "SDK imported after {change} changed");
        assert!(!f.trust_file().exists());
    }
}

#[test]
fn unsupported_layout_never_imports_project_node_modules() {
    let Some(f) = Fixture::new() else { return };
    let fake = f
        .project
        .join("node_modules/@earendil-works/pi-coding-agent");
    fs::create_dir_all(&fake).unwrap();
    fs::copy(
        f.root.join("install/package.json"),
        fake.join("package.json"),
    )
    .unwrap();
    executable(&fake.join("cli.js"), "#!/usr/bin/env node\n");
    let marker = f.root.join("loaded-project-module");
    fs::write(
        fake.join("sdk.mjs"),
        format!(
            "import {{ writeFileSync }} from 'node:fs'; writeFileSync({:?}, 'loaded');",
            marker.to_str().unwrap()
        ),
    )
    .unwrap();
    let wrapper = f.root.join("unknown-wrapper");
    executable(&wrapper, "#!/bin/sh\nexit 1\n");
    let req = request(&f.project, &f.project, None);
    for pi in [&wrapper, &fake.join("cli.js")] {
        let mut helper = f.helper(pi, &req);
        helper.command.current_dir(&f.project);
        assert_eq!(
            invoke_helper(helper, NONCE, None).unwrap_err(),
            TrustError::Unsupported
        );
    }
    assert!(!marker.exists());
    assert!(!f.trust_file().exists());
}

fn runtime() -> TrustRuntime {
    TrustRuntime {
        owner_id: "owner".into(),
        runtime_id: "runtime".into(),
        generation: 1,
        session_id: "session".into(),
        session_file: None,
    }
}

#[test]
fn inspections_are_exact_single_use_and_bound_to_the_registered_connection() {
    let state = ProjectTrustState::default();
    let target = TrustTarget {
        folder: "/workspace".into(),
        destination: Some("dev@example:2222".into()),
    };
    let original = Location::Remote {
        folder: target.folder.clone(),
        connection: "ssh -p 2222 dev@example".into(),
    };
    let changed = Location::Remote {
        folder: target.folder.clone(),
        connection: "ssh -p 2222 dev@other".into(),
    };
    let token = state
        .record(Inspection {
            at: Instant::now(),
            location: original.clone(),
            runtime_cwd: target.folder.clone(),
            project_key: "project".into(),
            runtime: runtime(),
            session_revision: 0,
            target: target.clone(),
            identity: json!({}),
        })
        .unwrap();
    for (key, location, cwd, expected) in [
        ("unknown", &original, target.folder.as_str(), target.clone()),
        ("project", &changed, target.folder.as_str(), target.clone()),
        ("project", &original, "/wrong", target.clone()),
        (
            "project",
            &original,
            target.folder.as_str(),
            TrustTarget {
                folder: "/".into(),
                ..target.clone()
            },
        ),
        (
            "project",
            &original,
            target.folder.as_str(),
            TrustTarget {
                destination: None,
                ..target.clone()
            },
        ),
    ] {
        assert!(matches!(
            state.take(&token, key, location, cwd, &runtime(), &expected),
            Err(TrustError::Mismatch)
        ));
    }
    let mut changed_runtime = runtime();
    changed_runtime.runtime_id = "other".into();
    assert!(matches!(
        state.take(
            &token,
            "project",
            &original,
            &target.folder,
            &changed_runtime,
            &target
        ),
        Err(TrustError::Mismatch)
    ));
    changed_runtime = runtime();
    changed_runtime.generation += 1;
    assert!(matches!(
        state.take(
            &token,
            "project",
            &original,
            &target.folder,
            &changed_runtime,
            &target
        ),
        Err(TrustError::Mismatch)
    ));
    assert!(matches!(
        state.take(
            "stale-token",
            "project",
            &original,
            &target.folder,
            &runtime(),
            &target
        ),
        Err(TrustError::Mismatch)
    ));
    assert!(state
        .take(
            &token,
            "project",
            &original,
            &target.folder,
            &runtime(),
            &target
        )
        .is_ok());
    assert!(matches!(
        state.take(
            &token,
            "project",
            &original,
            &target.folder,
            &runtime(),
            &target
        ),
        Err(TrustError::Mismatch)
    ));
    let expired = state
        .record(Inspection {
            at: Instant::now() - INSPECTION_TTL - Duration::from_secs(1),
            project_key: "project".into(),
            location: original.clone(),
            runtime_cwd: target.folder.clone(),
            runtime: runtime(),
            session_revision: 0,
            target: target.clone(),
            identity: json!({}),
        })
        .unwrap();
    assert!(matches!(
        state.take(
            &expired,
            "project",
            &original,
            &target.folder,
            &runtime(),
            &target
        ),
        Err(TrustError::Mismatch)
    ));
    assert_eq!(
        serde_json::to_value(TrustError::Unconfirmed).unwrap(),
        json!({"kind":"unconfirmed"})
    );
}

#[test]
fn native_binding_checks_the_managed_process_and_pins_startup_identity() {
    use super::super::{
        ownership::PiOwnership,
        process::{spawn_child, PiProcess},
    };
    use std::sync::atomic::AtomicBool;

    let runtime = runtime();
    let cwd = "/workspace";
    let location = Location::Local(cwd.into());
    let mut launch = TrustLaunch::new(location.clone(), None);
    let report = |nonce: &str, session: &str, identity: Value| {
        serde_json::to_vec(&json!({
            "nonce": nonce, "cwd": cwd, "sessionId": session,
            "sessionFile": runtime.session_file, "trusted": false, "identity": identity,
        }))
        .unwrap()
    };
    let nonce = launch.nonce().to_owned();
    let startup = json!({"ssh":"", "folderId":"1:2"});
    launch.observe(&report("wrong-nonce", &runtime.session_id, startup.clone()));
    assert!(launch.observed.is_none());
    launch.observe(&report(&nonce, &runtime.session_id, startup.clone()));
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "while IFS= read -r line; do :; done"]);
    let (child, writer, _stdout, _stderr) = spawn_child(&mut command).unwrap();
    let manager = Mutex::new(PiManager {
        ownership: PiOwnership::Active {
            owner_id: runtime.owner_id.clone(),
        },
        processes: HashMap::from([(
            runtime.runtime_id.clone(),
            PiProcess {
                owner_id: runtime.owner_id.clone(),
                generation: runtime.generation,
                child,
                writer,
                usable: Arc::new(AtomicBool::new(true)),
                trust: Some(launch),
            },
        )]),
        ..PiManager::default()
    });
    let bind = |binding: &TrustRuntime| live_binding(&manager, binding, &location, cwd);
    let first = bind(&runtime).unwrap();
    assert_eq!(first.identity, startup);
    for stale in [
        TrustRuntime {
            owner_id: "other".into(),
            ..runtime.clone()
        },
        TrustRuntime {
            runtime_id: "other".into(),
            ..runtime.clone()
        },
        TrustRuntime {
            generation: runtime.generation + 1,
            ..runtime.clone()
        },
        TrustRuntime {
            session_file: Some("/stale".into()),
            ..runtime.clone()
        },
    ] {
        assert_eq!(bind(&stale), Err(TrustError::Mismatch));
    }
    let observe = |session| {
        manager
            .lock()
            .unwrap()
            .processes
            .get_mut(&runtime.runtime_id)
            .unwrap()
            .trust
            .as_mut()
            .unwrap()
            .observe(&report(&nonce, session, json!({"ssh":""})));
    };
    observe("replacement-session");
    assert_eq!(bind(&runtime), Err(TrustError::Mismatch));
    observe(&runtime.session_id);
    let returned = bind(&runtime).unwrap();
    assert_eq!(returned.identity, startup);
    assert_ne!(returned.session_revision, first.session_revision);
    super::super::ownership::stop_runtime_process(
        &mut manager.lock().unwrap(),
        &runtime.runtime_id,
    )
    .unwrap();
    assert_eq!(bind(&runtime), Err(TrustError::Mismatch));
}

fn test_ssh_config(connection: &SshConnection) -> SshConfig {
    SshConfig::read(&connection.pi_command("launching-pi")).unwrap()
}

fn fake_ssh(f: &Fixture, extra: &str) -> SshConnection {
    let ssh = f.root.join("ssh");
    executable(
        &ssh,
        &format!(
            r#"#!/bin/sh
mode=connect; [ "$1" != '-G' ] || mode=config
for arg; do dest="$last"; last="$arg"; done
case "$dest" in
  a) host=first.example; address=192.0.2.1 ;;
  b) host=second.example; address=192.0.2.2 ;;
  *) exit 9 ;;
esac
if [ "$mode" = config ]; then
    printf 'user dev\nhostname %s\nport 2222\n' "$host"
    exit 0
fi
export HOME={home} PI_CODING_AGENT_DIR={agent} PATH={bin}:/usr/bin:/bin SHELL=/bin/sh
[ ! -e {peer_flag} ] || address=192.0.2.99
export SSH_CONNECTION="192.0.2.50 54321 $address 2222"
unset NODE_OPTIONS
printf 'login banner\n'
{extra}
exec /bin/sh -c "$last"
"#,
            peer_flag = shell_words::quote(f.root.join("peer-change").to_str().unwrap()),
            home = shell_words::quote(f.home.to_str().unwrap()),
            agent = shell_words::quote(f.agent.to_str().unwrap()),
            bin = shell_words::quote(f.pi.parent().unwrap().to_str().unwrap())
        ),
    );
    SshConnection::parse(&format!("{} a", shell_words::quote(ssh.to_str().unwrap()))).unwrap()
}

#[test]
fn fake_ssh_runs_quoted_exact_folder_and_detects_changed_remote_endpoint() {
    let Some(f) = Fixture::new() else { return };
    let first = fake_ssh(&f, "");
    let folder = f.project.to_str().unwrap();
    let config = test_ssh_config(&first);
    let run = |connection: &SshConnection, previous| {
        let req = request(&f.project, &f.project, previous).to_string();
        invoke_helper(
            remote_command(
                folder,
                connection,
                &req,
                NONCE,
                f.node.to_str().unwrap(),
                &config,
            )?,
            NONCE,
            previous,
        )
    };
    let identified = run(&first, None).unwrap();
    assert_eq!(
        identified.0.target.destination.as_deref(),
        Some("dev@first.example:2222")
    );
    assert_eq!(identified.0.target.folder, folder);
    assert!(!identified.0.saved);
    let record = inspection(
        Location::Remote {
            folder: folder.into(),
            connection: "a".into(),
        },
        &f.project,
        identified,
    );
    // Same canonical path, login account, and installation, but a different SSH endpoint.
    let other = SshConnection::parse(&format!(
        "{} b",
        shell_words::quote(f.root.join("ssh").to_str().unwrap())
    ))
    .unwrap();
    assert_eq!(
        run(&other, Some(&record)).unwrap_err(),
        TrustError::Mismatch
    );
    assert!(!f.trust_file().exists());
    // Keep ssh -G unchanged while changing the peer reported by the remote process.
    let peer_flag = f.root.join("peer-change");
    fs::write(&peer_flag, "").unwrap();
    assert_eq!(
        run(&first, Some(&record)).unwrap_err(),
        TrustError::Mismatch
    );
    fs::remove_file(peer_flag).unwrap();
    assert!(run(&first, Some(&record)).unwrap().0.saved);
    assert!(!f.home.join(".pi/agent/trust.json").exists());
}

#[test]
fn remote_project_path_node_is_never_executed() {
    let Some(f) = Fixture::new() else { return };
    let marker = f.root.join("node-executed");
    let script = f.project.join("evil-node");
    executable(
        &script,
        &format!(
            "#!/bin/sh\ntouch {}\n",
            shell_words::quote(marker.to_str().unwrap())
        ),
    );
    let node = f.project.join("node");
    symlink(&script, &node).unwrap();
    let shell = f.root.join("login-shell");
    executable(
        &shell,
        &format!(
            "#!/bin/sh\nexport PATH={}:$PATH\nexec /bin/sh -c \"$2\"\n",
            shell_words::quote(f.project.to_str().unwrap())
        ),
    );
    let connection = fake_ssh(
        &f,
        &format!(
            "export SHELL={}",
            shell_words::quote(shell.to_str().unwrap())
        ),
    );
    let config = test_ssh_config(&connection);
    let req = request(&f.project, &f.project, None).to_string();
    let helper = remote_command(
        f.project.to_str().unwrap(),
        &connection,
        &req,
        NONCE,
        f.node.to_str().unwrap(),
        &config,
    )
    .unwrap();
    assert_eq!(
        invoke_helper(helper, NONCE, None).unwrap_err(),
        TrustError::Unsupported
    );
    assert!(!marker.exists());
}

#[test]
fn fake_ssh_disconnects_and_timeouts_do_not_fall_back_or_claim_success() {
    let Some(f) = Fixture::new() else { return };
    let first = fake_ssh(&f, "");
    let folder = f.project.to_str().unwrap();
    let req = request(&f.project, &f.project, None).to_string();
    let config = test_ssh_config(&first);
    let verified = invoke_helper(
        remote_command(
            folder,
            &first,
            &req,
            NONCE,
            f.node.to_str().unwrap(),
            &config,
        )
        .unwrap(),
        NONCE,
        None,
    )
    .unwrap();
    let record = inspection(
        Location::Remote {
            folder: folder.into(),
            connection: "a".into(),
        },
        &f.project,
        verified,
    );
    let grant = request(&f.project, &f.project, Some(&record)).to_string();
    let stalled = fake_ssh(&f, "exec /bin/sleep 2");
    let mut helper = remote_command(
        folder,
        &stalled,
        &grant,
        NONCE,
        f.node.to_str().unwrap(),
        &test_ssh_config(&stalled),
    )
    .unwrap();
    helper.timeout = Duration::from_millis(200);
    assert_eq!(
        invoke_helper(helper, NONCE, Some(&record)).unwrap_err(),
        TrustError::Unconfirmed
    );
    assert!(!f.trust_file().exists());

    let lost = fake_ssh(&f, "/bin/sh -c \"$last\" >/dev/null; exec /bin/sleep 5");
    let mut helper = remote_command(
        folder,
        &lost,
        &grant,
        NONCE,
        f.node.to_str().unwrap(),
        &test_ssh_config(&lost),
    )
    .unwrap();
    helper.timeout = Duration::from_secs(2);
    assert_eq!(
        invoke_helper(helper, NONCE, Some(&record)).unwrap_err(),
        TrustError::Unconfirmed
    );
    assert!(f.inspect(&f.project, &f.project).unwrap().0.saved);
    assert!(!f.home.join(".pi/agent/trust.json").exists());

    // If no command could be started, there was no dispatch to leave unconfirmed.
    fs::remove_file(f.root.join("ssh")).unwrap();
    assert!(matches!(
        remote_command(
            folder,
            &first,
            &grant,
            NONCE,
            f.node.to_str().unwrap(),
            &config
        ),
        Err(TrustError::Unavailable)
    ));
}
