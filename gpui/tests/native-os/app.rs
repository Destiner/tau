//! Owned-window production app + installed-Pi acceptance; no clipboard, AX or global input.
#![cfg(feature = "native-os-tests")]
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native-app requires macOS and an unlocked WindowServer");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn settings_menu() {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::ffi::CStr;
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let menu: *mut Object = msg_send![app, mainMenu];
        let item: *mut Object = msg_send![menu, itemAtIndex: 0isize];
        let submenu: *mut Object = msg_send![item, submenu];
        let _: () = msg_send![submenu, update];
        let count: isize = msg_send![submenu, numberOfItems];
        for i in 0..count {
            let item: *mut Object = msg_send![submenu, itemAtIndex: i];
            let title: *mut Object = msg_send![item, title];
            let pointer: *const std::ffi::c_char = msg_send![title, UTF8String];
            if CStr::from_ptr(pointer).to_bytes() == "Settings…".as_bytes() {
                let enabled: objc::runtime::BOOL = msg_send![item, isEnabled];
                assert_eq!(enabled, objc::runtime::YES);
                let _: () = msg_send![submenu, performActionForItemAtIndex: i];
                return;
            }
        }
        panic!("native Settings menu missing");
    }
}

const EXTENSION: &str = r#"
export default function(pi) {
  pi.registerCommand("tau-native-smoke", {
    description: "Credential-free native acceptance",
    handler: async (_args, ctx) => {
      if (ctx.mode !== "rpc") throw new Error("RPC required");
      const value = await ctx.ui.input("Tau invented canary", "invented reply", { timeout: 12000 });
      if (value !== "invented reply") throw new Error("Unexpected canary reply");
      pi.setSessionName("Tau invented smoke");
      pi.appendEntry("tau-native-smoke", { ok: true });
      pi.sendMessage({ customType: "tau-native-smoke", content: "Invented custom output", display: true }, { triggerTurn: false });
      ctx.ui.notify("Invented completion", "info");
    }
  });
}
"#;

// The proxy forwards actual installed-Pi bytes unchanged. Its only journal is
// event kind/role, to prove real streaming without exposing provider payloads.
const WRAPPER: &str = r#"#!/usr/bin/env python3
import json, os, signal, subprocess, sys
root = os.path.dirname(os.path.realpath(__file__))
args = [os.environ['TAU_NATIVE_INSTALLED_PI']] + sys.argv[1:]
if os.environ.get('TAU_NATIVE_MODEL_SMOKE') != '1':
    args += ['--offline']
args += ['--no-extensions', '--no-skills', '--no-prompt-templates', '--no-themes', '--no-context-files', '--no-tools', '--no-approve', '--extension', os.path.join(root, 'smoke.ts')]
child = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
def stop(signum, _frame):
    child.terminate()
    try: child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
    sys.exit(128 + signum)
signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
try:
    for line in child.stdout:
        try:
            record = json.loads(line)
            kind = record.get('type')
            if kind in ('message_update', 'message_end'):
                role = record.get('message', {}).get('role')
                # RPC updates omit the cumulative SDK message; identify the wire delta itself.
                delta = record.get('assistantMessageEvent', {}).get('type')
                marker = kind + (':text_delta' if kind == 'message_update' and delta == 'text_delta' else ':assistant' if role == 'assistant' else '')
                with open(os.path.join(root, 'events.jsonl'), 'a') as journal: journal.write(marker + '\n')
        except (ValueError, TypeError): pass
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    sys.exit(child.wait())
finally:
    if child.poll() is None:
        child.terminate()
        child.wait()
"#;

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use std::{
        io::Write,
        os::unix::{fs::PermissionsExt, process::CommandExt},
        path::PathBuf,
        process::Command,
    };
    if std::env::args().nth(1).as_deref() == Some("--owned-app") {
        let root = PathBuf::from(std::env::args().nth(2).expect("owned directory"));
        return tau_gpui_prototype::app::run_native_smoke(
            root.clone(),
            root.join("project").to_string_lossy().into_owned(),
            settings_menu,
        )
        .map_err(|_| anyhow::anyhow!("native-app startup or acceptance failed"));
    }
    let root = tempfile::Builder::new()
        .prefix("tau-native-app-")
        .tempdir()?;
    for name in ["project", "home", "agent"] {
        std::fs::create_dir(root.path().join(name))?;
    }
    let executable = root.path().join("native-app");
    std::fs::copy(std::env::current_exe()?, &executable)?;
    let wrapper = root.path().join("pi-wrapper");
    std::fs::write(&wrapper, WRAPPER)?;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
    std::fs::write(root.path().join("smoke.ts"), EXTENSION)?;
    let installed =
        std::env::var_os("TAU_PI_PATH").unwrap_or_else(|| "/opt/homebrew/bin/pi".into());
    anyhow::ensure!(
        PathBuf::from(&installed).is_file(),
        "installed Pi not found"
    );
    let model = std::env::var("TAU_NATIVE_MODEL_SMOKE").as_deref() == Ok("1");
    let mut command = Command::new(&executable);
    if !model {
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root.path().join("home"))
            .env("PI_CODING_AGENT_DIR", root.path().join("agent"));
    }
    command
        .arg("--owned-app")
        .arg(root.path())
        .current_dir(root.path().join("project"))
        .env("TAU_PI_PATH", &wrapper)
        .env("TAU_NATIVE_INSTALLED_PI", installed)
        .env("PI_SKIP_VERSION_CHECK", "1")
        .env("PI_TELEMETRY", "0")
        .env("TAU_NATIVE_MODEL_SMOKE", if model { "1" } else { "0" });
    eprintln!(
        "START native-app owned production window + {}",
        if model {
            "opt-in real model using existing Pi auth"
        } else {
            "credential-free isolated installed Pi"
        }
    );
    // The entire owned Pi tree inherits this fresh process group. Clean it even
    // when an assertion/watchdog exits before GPUI can await worker shutdown.
    command
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = command.spawn()?;
    let group = child.id();
    let result = child.wait_with_output();
    let _ = Command::new("/bin/kill")
        .args(["-TERM", "--", &format!("-{group}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while Command::new("/bin/kill")
        .args(["-0", "--", &format!("-{group}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?
        .success()
    {
        if std::time::Instant::now() >= deadline {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{group}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let output = result?;
    // Keep failed evidence outside the owned temp tree, with no payloads or raw stderr.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let slice = if model { "model" } else { "credential-free" };
    let log = format!(
        "/tmp/tau-native-app-reopen-{slice}-{}-{stamp}.log",
        std::process::id()
    );
    std::fs::write(&log, &output.stderr)?;
    std::io::stderr().write_all(&output.stderr)?;
    let markers = String::from_utf8_lossy(&output.stderr)
        .matches("PASS native-app completion-gated quit")
        .count();
    anyhow::ensure!(
        output.status.success() && markers == 1,
        "native-app failed; reviewed phase log: {log}; completion markers={markers}"
    );
    eprintln!("PASS native-app child exited after gated production shutdown");
    Ok(())
}
