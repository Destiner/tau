//! Credential-free installed-Pi canary; see the crate README for explicit invocation.
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    process::Command,
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use tau_core::pi::{PiEvent, PiSink, PiState};
use tau_session::session::{Controller, Effect, ExtensionResponse};

const OWNER: &str = "tau-live-canary";
const RUNTIME: &str = "tau-live-canary";
const EXTENSION: &str = r#"
export default function(pi) {
  pi.registerCommand("tau-native-smoke", {
    description: "Credential-free native canary",
    handler: async (_args, ctx) => {
      if (ctx.mode !== "rpc") throw new Error("RPC required");
      const value = await ctx.ui.input("Tau invented canary", "invented reply", { timeout: 8000 });
      if (value !== "invented reply") throw new Error("Unexpected canary reply");
      pi.setSessionName("Tau invented smoke");
      pi.appendEntry("tau-native-smoke", { ok: true });
      pi.sendMessage({ customType: "tau-native-smoke", content: "Invented custom output", display: true }, { triggerTurn: false });
      ctx.ui.notify("Invented completion", "info");
    }
  });
}
"#;

struct Host {
    // Drop the process manager before deleting the isolated directories.
    pi: PiState,
    controller: Controller,
    rx: mpsc::Receiver<PiEvent>,
    root: tempfile::TempDir,
    outstanding: BTreeMap<String, String>,
    responses: BTreeSet<String>,
    events: BTreeSet<String>,
    last_record: Option<Value>,
    registrations: usize,
    custom_history: bool,
    custom_message_event: bool,
    custom_entry: bool,
    handled: bool,
}
impl Host {
    fn effects(&mut self, effects: Vec<Effect>) -> Result<(), String> {
        let mut queue = VecDeque::from(effects);
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Send(v) => {
                    let id = v["id"].as_str().ok_or("Missing outbound ID")?.to_owned();
                    let kind = v["type"]
                        .as_str()
                        .ok_or("Missing outbound type")?
                        .to_owned();
                    if kind != "extension_ui_response" {
                        if !id.starts_with(&format!(
                            "tau:{RUNTIME}:g{}:",
                            self.controller.generation()
                        )) && !id.starts_with("canary:")
                        {
                            return Err("Outbound generation ID mismatch".into());
                        }
                        if self.outstanding.insert(id.clone(), kind.clone()).is_some() {
                            return Err("Duplicate request ID".into());
                        }
                    }
                    self.pi
                        .send(OWNER, RUNTIME.into(), v)
                        .map_err(|_| "Transport send failed")?;
                    if kind == "extension_ui_response" {
                        // This confirms only the actual pipe write, not a fabricated Pi acknowledgement.
                        queue.extend(self.controller.extension_sent(&id, true));
                    }
                }
                Effect::RegisterSession { .. } => self.registrations += 1,
                Effect::IdentityChanged { .. } => {}
            }
        }
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        let binary =
            std::env::var_os("TAU_LIVE_PI").unwrap_or_else(|| "/opt/homebrew/bin/pi".into());
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env(
                "PATH",
                "/opt/homebrew/opt/node@24/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
            )
            .env("HOME", self.root.path().join("home"))
            .env("PI_CODING_AGENT_DIR", self.root.path().join("agent"))
            .env("PI_OFFLINE", "1")
            .env("PI_SKIP_VERSION_CHECK", "1")
            .env("PI_TELEMETRY", "0")
            .current_dir(self.root.path().join("project"))
            .args([
                "--mode",
                "rpc",
                "--offline",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--no-tools",
                "--no-approve",
                "--session-dir",
            ])
            .arg(self.root.path().join("sessions"))
            .arg("--extension")
            .arg(self.root.path().join("smoke.ts"));
        let (tx, rx) = mpsc::channel();
        self.rx = rx;
        let sink = PiSink::new(Arc::new(move |event: PiEvent| {
            let _ = tx.send(event);
        }));
        let generation = self
            .pi
            .start_command(OWNER.into(), RUNTIME.into(), command, sink)
            .map_err(|_| "Pi startup failed")?;
        self.outstanding.clear();
        let effects = self.controller.begin_generation(generation);
        self.effects(effects)
    }
    fn pump_until(&mut self, predicate: impl Fn(&Self) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !predicate(self) {
            if Instant::now() >= deadline {
                return Err("Canary phase deadline exceeded".into());
            }
            let event = self.rx.recv_timeout(Duration::from_millis(500));
            let event = match event {
                Ok(e) => e,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err("Event channel closed".into()),
            };
            if event.runtime_id != RUNTIME || event.generation != self.controller.generation() {
                return Err("Transport generation mismatch".into());
            }
            if event.kind == "exited" {
                return Err("Pi exited unexpectedly".into());
            }
            let Some(line) = event.line else { continue };
            let record: Value = serde_json::from_str(&line).map_err(|_| "Invalid JSONL record")?;
            let kind = record["type"].as_str().ok_or("Missing record type")?;
            if kind == "response" {
                let id = record["id"].as_str().ok_or("Response missing ID")?;
                let command = self.outstanding.remove(id).ok_or("Uncorrelated response")?;
                if record["command"] != command || record["success"] != true {
                    return Err("Pi command failed or shape mismatched".into());
                }
                self.responses.insert(command.clone());
                if command == "prompt" {
                    self.handled = record["data"]["disposition"] == "handled";
                }
                if command == "get_messages" {
                    let messages = record["data"]["messages"]
                        .as_array()
                        .ok_or("Missing messages array")?;
                    self.custom_history |= messages.iter().any(|m| {
                        m["role"] == "custom"
                            && m["customType"] == "tau-native-smoke"
                            && m["content"] == "Invented custom output"
                    });
                }
                if command == "get_entries" {
                    self.custom_entry = record["data"]["entries"]
                        .as_array()
                        .ok_or("Missing entries array")?
                        .iter()
                        .any(|e| {
                            e["type"] == "custom"
                                && e["customType"] == "tau-native-smoke"
                                && e["data"]["ok"] == true
                        });
                }
            } else {
                if matches!(kind, "message_start" | "message_end") {
                    if record["message"]["role"] == "assistant" {
                        return Err("Unexpected assistant output in model-free canary".into());
                    }
                    if kind == "message_end"
                        && record["message"]["role"] == "custom"
                        && record["message"]["customType"] == "tau-native-smoke"
                        && record["message"]["content"] == "Invented custom output"
                    {
                        self.custom_message_event = true;
                    }
                }
                self.events.insert(kind.to_owned());
                if kind == "extension_error" {
                    return Err("Canary extension failed".into());
                }
            }
            let effects =
                self.controller
                    .ingest(&event.runtime_id, event.generation, record.clone());
            self.last_record = Some(record);
            self.effects(effects)?;
            if self.controller.state().error.is_some() {
                return Err("Controller entered error state".into());
            }
        }
        Ok(())
    }
    fn bootstrapped(&self) -> bool {
        let s = self.controller.state();
        s.ready
            && !s.history_loading
            && self.controller.pending_count() == 0
            && s.commands
                .iter()
                .any(|c| c["name"] == "tau-native-smoke" && c["source"] == "extension")
    }
}

pub fn run_canary() -> Result<(), String> {
    if std::env::var("TAU_LIVE_RPC_CHECK").as_deref() != Ok("1") {
        return Err("Opt in with TAU_LIVE_RPC_CHECK=1".into());
    }
    let root = tempfile::Builder::new()
        .prefix("tau-live-rpc-")
        .tempdir()
        .map_err(|_| "Temporary directory failed")?;
    for dir in ["home", "agent", "project", "sessions"] {
        std::fs::create_dir(root.path().join(dir)).map_err(|_| "Isolation setup failed")?;
    }
    std::fs::write(root.path().join("smoke.ts"), EXTENSION)
        .map_err(|_| "Extension setup failed")?;
    let pi = PiState::default();
    pi.claim(
        OWNER.into(),
        pi.read_revision()
            .map_err(|_| "Ownership revision failed")?,
    )
    .map_err(|_| "Ownership claim failed")?;
    let (_, rx) = mpsc::channel();
    let mut host = Host {
        pi,
        controller: Controller::new(RUNTIME),
        rx,
        root,
        outstanding: BTreeMap::new(),
        responses: BTreeSet::new(),
        events: BTreeSet::new(),
        last_record: None,
        registrations: 0,
        custom_history: false,
        custom_message_event: false,
        custom_entry: false,
        handled: false,
    };
    host.start()?;
    host.pump_until(Host::bootstrapped)?;
    for command in [
        "get_state",
        "get_messages",
        "get_available_models",
        "get_commands",
    ] {
        if !host.responses.contains(command) {
            return Err("Bootstrap command missing".into());
        }
    }
    if !host.controller.state().models.is_empty() {
        return Err("Credential-free model registry unexpectedly populated".into());
    }
    if !std::path::Path::new(&host.controller.state().identity.path)
        .starts_with(host.root.path().join("sessions"))
    {
        return Err("Session escaped isolation".into());
    }
    let effects = host.controller.submit("/tau-native-smoke");
    if effects.is_empty() {
        return Err("Controller refused extension submission".into());
    }
    host.effects(effects)?;
    host.pump_until(|h| !h.controller.state().extension_dialogs.is_empty())?;
    let dialog = host.controller.state().extension_dialogs[0].clone();
    if dialog.request["method"] != "input" {
        return Err("Unexpected actual dialog shape".into());
    }
    // Stop is a real abort RPC while the extension command awaits interaction.
    let effects = host.controller.abort();
    if effects.is_empty() {
        return Err("Controller refused Stop".into());
    }
    host.effects(effects)?;
    host.pump_until(|h| h.responses.contains("abort"))?;
    let effects = host.controller.extension_response(
        &dialog.id,
        ExtensionResponse::Value("invented reply".into()),
    );
    host.effects(effects)?;
    host.pump_until(|h| {
        h.handled
            && h.controller.state().identity.name == "Tau invented smoke"
            && h.custom_history
            && h.controller.pending_count() == 0
    })?;
    host.effects(vec![Effect::Send(
        serde_json::json!({"id":"canary:entries", "type":"get_entries"}),
    )])?;
    host.pump_until(|h| h.custom_entry)?;
    for event in [
        "session_info_changed",
        "entry_appended",
        "message_start",
        "message_end",
        "extension_ui_request",
    ] {
        if !host.events.contains(event) {
            return Err("Expected public-API Pi event missing".into());
        }
    }
    if !host.custom_message_event {
        return Err("Actual custom message completion missing".into());
    }
    if std::path::Path::new(&host.controller.state().identity.path).exists() {
        return Err("Command-only session unexpectedly materialized".into());
    }
    if host.events.contains("message_update")
        || host.events.contains("tool_execution_start")
        || host.registrations != 0
        || host.controller.state().durable
    {
        return Err("Unexpected assistant/tool/durability claim".into());
    }
    let old_generation = host.controller.generation();
    let old_record = host
        .last_record
        .clone()
        .ok_or("Missing actual prior record")?;
    if host
        .pi
        .stop(OWNER, RUNTIME)
        .map_err(|_| "Native stop failed")?
        != Some(old_generation)
    {
        return Err("Native stop generation mismatch".into());
    }
    host.start()?;
    if host.controller.generation() <= old_generation {
        return Err("Restart did not advance generation".into());
    }
    let before = serde_json::to_value(host.controller.state()).map_err(|_| "Snapshot failed")?;
    if !host
        .controller
        .ingest(RUNTIME, old_generation, old_record)
        .is_empty()
        || serde_json::to_value(host.controller.state()).map_err(|_| "Snapshot failed")? != before
    {
        return Err("Stale real response crossed generation fence".into());
    }
    host.pump_until(Host::bootstrapped)?;
    host.pi
        .stop(OWNER, RUNTIME)
        .map_err(|_| "Final native stop failed")?;
    println!("PASS real Pi: isolated bootstrap, correlated IDs, extension interaction, custom history/entries, session name, abort, native stop/restart, stale-generation fence");
    println!("NOT PROVED: assistant streaming, model-issued tools, assistant persistence/durability, shell UI");
    Ok(())
}
fn main() {
    if let Err(error) = run_canary() {
        eprintln!("FAIL live RPC canary: {error}");
        std::process::exit(1);
    }
}
