//! Owned-window production app + installed-Pi acceptance; no clipboard, AX or global input.
#![cfg(feature = "native-os-tests")]
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("native-app requires macOS and an unlocked WindowServer");
    std::process::exit(2);
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn check_native_menus() {
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
            assert_ne!(
                CStr::from_ptr(pointer).to_bytes(),
                "Settings…".as_bytes(),
                "Tau must not expose a Settings menu"
            );
        }
    }
}

const EXTENSION: &str = r#"
import { Type } from "@earendil-works/pi-ai";

export default function(pi) {
  if (process.env.TAU_NATIVE_MODEL_SMOKE === "1") {
    pi.registerTool({
      name: "tau_native_wait",
      label: "Invented wait",
      description: "Safe native cancellation fixture. Takes no arguments, emits invented partial output, then waits only for cancellation. No filesystem, network, subprocesses or nested tools.",
      parameters: Type.Object({}, { additionalProperties: false }),
      annotations: { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
      async execute(_id, _params, signal, onUpdate) {
        if (!signal) throw new Error("Cancellation signal required");
        onUpdate?.({ content: [{ type: "text", text: "TAU_NATIVE_PARTIAL" }], details: { ready: true } });
        await new Promise(resolve => {
          if (signal.aborted) resolve();
          else signal.addEventListener("abort", resolve, { once: true });
        });
        return { content: [{ type: "text", text: "TAU_NATIVE_PARTIAL\nTAU_NATIVE_ABORTED" }], details: { aborted: true }, terminate: true };
      }
    });
    // --no-tools removes extension tools from Pi 0.99.2's registry entirely.
    pi.on("session_start", () => {
      pi.setActiveTools(["tau_native_wait"]);
      const active = pi.getActiveTools();
      if (active.length !== 1 || active[0] !== "tau_native_wait") throw new Error("Fixture-only loadout required");
    });
  }
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
// allowlisted event markers/booleans; call IDs remain only in proxy memory.
const WRAPPER: &str = r#"#!/usr/bin/env python3
import json, os, signal, subprocess, sys
root = os.path.dirname(os.path.realpath(__file__))
args = [os.environ['TAU_NATIVE_INSTALLED_PI']] + sys.argv[1:]
if os.environ.get('TAU_NATIVE_MODEL_SMOKE') != '1':
    args += ['--offline', '--no-tools']
else:
    args += ['--no-builtin-tools']
args += ['--no-extensions', '--no-skills', '--no-prompt-templates', '--no-themes', '--no-context-files', '--no-approve', '--extension', os.path.join(root, 'smoke.ts')]
child = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
def stop(signum, _frame):
    child.terminate()
    try: child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
    mark('child_exit_ack')
    sys.exit(128 + signum)
signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
calls = set()
def mark(marker):
    with open(os.path.join(root, 'events.jsonl'), 'a') as journal: journal.write(marker + '\n')
mark('child_started')
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
                mark(marker)
                if role == 'assistant' and record.get('message', {}).get('stopReason') in ('error', 'aborted'):
                    mark('assistant_terminal:' + record['message']['stopReason'])
            elif kind == 'tool_execution_start' and record.get('toolName') == 'tau_native_wait':
                calls.add(record.get('toolCallId'))
                mark('tool_execution_start:fixture')
            elif kind in ('tool_execution_update', 'tool_execution_end') and record.get('toolCallId') in calls:
                result = record.get('partialResult' if kind == 'tool_execution_update' else 'result', {})
                if kind == 'tool_execution_update' and result.get('details', {}).get('ready') is True:
                    mark('tool_execution_update:ready')
                elif kind == 'tool_execution_end' and result.get('details', {}).get('aborted') is True:
                    mark('tool_execution_end:aborted')
            elif kind == 'response' and record.get('command') == 'get_messages' and record.get('success') is True:
                messages = record.get('data', {}).get('messages', [])
                compact = any(m.get('role') == 'compactionSummary' and 'TAU_SYNTHETIC_COMPACTION_CONTEXT_ONLY' in m.get('summary', '') for m in messages)
                branch = any(m.get('role') == 'branchSummary' and 'TAU_SYNTHETIC_BRANCH_CONTEXT_ONLY' in m.get('summary', '') for m in messages)
                if compact and branch: mark('history:get_messages:fixture')
            elif kind == 'response' and record.get('command') == 'get_entries' and record.get('success') is True:
                data = record.get('data', {})
                entries = data.get('entries', [])
                prior = any(e.get('type') == 'message' and e.get('message', {}).get('content') == 'Invented prior layer' for e in entries)
                compact = any(e.get('type') == 'compaction' and 'TAU_SYNTHETIC_COMPACTION_CONTEXT_ONLY' in e.get('summary', '') for e in entries)
                branch = any(e.get('type') == 'branch_summary' and 'TAU_SYNTHETIC_BRANCH_CONTEXT_ONLY' in e.get('summary', '') for e in entries)
                by_id = {e.get('id'): e for e in entries}
                active = set()
                leaf = data.get('leafId')
                while leaf in by_id and leaf not in active:
                    active.add(leaf)
                    leaf = by_id[leaf].get('parentId')
                # Pi may append startup settings; validate the actual active ancestry.
                if prior and compact and branch and {'10000001', '10000003', '10000004', '10000005'}.issubset(active): mark('history:get_entries:fixture')
            elif kind == 'response' and record.get('command') == 'abort' and record.get('success') is True:
                mark('response:abort:success')
            elif kind == 'agent_settled':
                mark('agent_settled')
            elif kind == 'extension_error' or (kind == 'response' and record.get('success') is False):
                mark('protocol_or_extension_error')
        except (ValueError, TypeError): pass
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
    status = child.wait()
    mark('child_exit_ack')
    sys.exit(status)
finally:
    if child.poll() is None:
        child.terminate()
        try: child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
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
            check_native_menus,
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
    if let Some(directory) = std::env::var_os("TAU_NATIVE_CAPTURE_DIR") {
        let directory = PathBuf::from(directory);
        anyhow::ensure!(
            directory.is_absolute() && directory.starts_with("/tmp"),
            "native invented-data captures require an absolute /tmp directory"
        );
        std::fs::create_dir_all(&directory)?;
        command.env("TAU_NATIVE_CAPTURE_DIR", directory);
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
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let slice = if model { "model" } else { "credential-free" };
    let log = format!(
        "/tmp/tau-native-real-tool-run-{slice}-{}-{stamp}.log",
        std::process::id()
    );
    eprintln!("EVIDENCE native-app early phase log: {log}");
    command
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log)?);
    let child = command.spawn()?;
    let group = child.id();
    let result = child.wait_with_output();
    // Snapshot before harness cleanup: cleanup must not manufacture shutdown proof.
    let shutdown_journal =
        std::fs::read_to_string(root.path().join("events.jsonl")).unwrap_or_default();
    let starts = shutdown_journal
        .lines()
        .filter(|s| *s == "child_started")
        .count();
    let exits = shutdown_journal
        .lines()
        .filter(|s| *s == "child_exit_ack")
        .count();
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
    // Preserve only the content-free journal, never the temporary session payloads.
    if root.path().join("events.jsonl").is_file() {
        std::fs::copy(root.path().join("events.jsonl"), format!("{log}.events"))?;
    }
    let phases = std::fs::read(&log)?;
    std::io::stderr().write_all(&phases)?;
    let markers = String::from_utf8_lossy(&phases)
        .matches("PASS native-app completion-gated last CmdW quit")
        .count();
    anyhow::ensure!(
        output.status.success() && markers == 1,
        "native-app failed; reviewed phase log: {log}; completion markers={markers}"
    );
    let phases = String::from_utf8_lossy(&phases);
    for phase in [
        "PASS native-app compacted history real get_messages/get_entries + measured Load Earlier prior layer; stable tail identity/content/reader bounds",
        "PASS native-app synthetic history sidebar return retains original draft",
        "PASS native-app busy last CmdW cancel retains actual root/composer and unrelated draft",
        "PASS native-app palette visible measured root panel + focused empty input",
        "PASS native-app remote visible measured root panel + focused empty input",
        "PASS native-app reopened root palette visible measured panel + focused empty input",
        "PASS native-app CmdK/query/Enter remote form + Escape/reopen/Escape retain draft and live composer focus without SSH",
    ] {
        anyhow::ensure!(
            phases.matches(phase).count() == 1,
            "native-app missing or repeated acceptance phase; reviewed phase log: {log}"
        );
    }
    anyhow::ensure!(
        starts >= 2 && exits == starts,
        "native-app exited before all installed Pi children acknowledged exit; starts={starts}, exits={exits}; reviewed phase log: {log}"
    );
    eprintln!(
        "PASS native-app last CmdW child exited after gated production shutdown + all Pi child exit acknowledgements"
    );
    Ok(())
}
