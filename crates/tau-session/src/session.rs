use crate::transcript::{content_text, message_key, project, string, Entry};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const REQUEST_TIMEOUT_MS: u64 = 10_000;
pub const STOP_TIMEOUT_MS: u64 = 10_000;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub path: String,
    pub name: String,
}
impl Identity {
    fn binding_eq(&self, other: &Self) -> bool {
        self.id == other.id && self.path == other.path
    }
    fn saved_candidate(&self) -> bool {
        !self.id.is_empty() && !self.path.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Send(Value),
    RegisterSession {
        runtime_id: String,
        generation: u64,
        id: String,
        path: String,
        name: String,
    },
    IdentityChanged {
        previous: Identity,
        current: Identity,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Starting,
    Ready,
    Working,
    Stopping,
    Error,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    Steer,
    FollowUp,
}
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensionResponse {
    Value(String),
    Confirmed(bool),
    Cancelled,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueSnapshot {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionDialog {
    pub id: String,
    pub request: Value,
    pub deadline_ms: Option<u64>,
    pub submitting: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub identity: Identity,
    pub model: Value,
    pub effort: String,
    pub status: Status,
    pub ready: bool,
    pub working: bool,
    pub stopping: bool,
    pub history_loading: bool,
    pub transcript: Vec<Entry>,
    pub draft: String,
    pub recovered_drafts: Vec<String>,
    pub queue: QueueSnapshot,
    pub extension_dialogs: Vec<ExtensionDialog>,
    pub models: Vec<Value>,
    pub commands: Vec<Value>,
    pub thinking_levels: Vec<String>,
    pub error: Option<String>,
    pub durable: bool,
}
#[derive(Clone, Debug)]
struct Pending {
    command: String,
    identity: Identity,
    epoch: u64,
    revision: u64,
    serial: u64,
    deadline: u64,
    payload: Value,
    barrier: bool,
    draft: Option<String>,
    optimistic: Option<String>,
    accepted: bool,
}
/// Pure protocol state. The host owns IO, persistence, rendering and scheduling.
pub struct Controller {
    runtime_id: String,
    generation: u64,
    sequence: u64,
    now: u64,
    epoch: u64,
    revision: u64,
    state: State,
    pending: BTreeMap<String, Pending>,
    latest: BTreeMap<String, String>,
    active_assistant: Option<String>,
    message_occurrences: BTreeMap<String, usize>,
    registered: HashSet<(String, String)>,
    startup_deadline: Option<u64>,
    stop_deadline: Option<u64>,
    admission: Option<(u64, String)>,
    queue_submission: Option<(String, QueueKind)>,
    queue_version: u64,
    state_serial: u64,
    connected: bool,
}
impl Controller {
    pub fn new(runtime_id: impl Into<String>) -> Self {
        Self {
            runtime_id: runtime_id.into(),
            generation: 0,
            sequence: 0,
            now: 0,
            epoch: 0,
            revision: 0,
            state: State::default(),
            pending: BTreeMap::new(),
            latest: BTreeMap::new(),
            active_assistant: None,
            message_occurrences: BTreeMap::new(),
            registered: HashSet::new(),
            startup_deadline: None,
            stop_deadline: None,
            admission: None,
            queue_submission: None,
            queue_version: 0,
            state_serial: 0,
            connected: false,
        }
    }
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn set_draft(&mut self, text: impl Into<String>) {
        self.state.draft = text.into();
    }
    pub fn take_recovered_drafts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.state.recovered_drafts)
    }
    /// Optional requested saved identity, set before bootstrap; never itself a durability proof.
    pub fn bind_source(&mut self, id: impl Into<String>, path: impl Into<String>) {
        self.state.identity = Identity {
            id: id.into(),
            path: path.into(),
            name: String::new(),
        };
        self.epoch += 1;
    }
    fn next_id(&mut self) -> String {
        self.sequence += 1;
        format!(
            "tau:{}:g{}:{}",
            self.runtime_id, self.generation, self.sequence
        )
    }
    fn send(&mut self, command: &str, mut payload: Value, barrier: bool) -> Effect {
        let id = self.next_id();
        payload["type"] = json!(command);
        payload["id"] = json!(id);
        self.pending.insert(
            id.clone(),
            Pending {
                command: command.into(),
                identity: self.state.identity.clone(),
                epoch: self.epoch,
                revision: self.revision,
                serial: self.sequence,
                deadline: self.now.saturating_add(REQUEST_TIMEOUT_MS),
                payload: payload.clone(),
                barrier,
                draft: None,
                optimistic: None,
                accepted: false,
            },
        );
        if command != "prompt" && !barrier {
            self.latest.insert(command.into(), id);
        }
        Effect::Send(payload)
    }
    pub fn begin_generation(&mut self, generation: u64) -> Vec<Effect> {
        if generation <= self.generation {
            return vec![];
        }
        self.recover_pending();
        self.finalize_partial();
        self.generation = generation;
        self.epoch += 1;
        self.pending.clear();
        self.latest.clear();
        self.message_occurrences.clear();
        self.connected = true;
        self.state_serial = 0;
        self.admission = None;
        self.state.ready = false;
        self.state.working = false;
        self.state.stopping = false;
        self.state.durable = false;
        self.state.error = None;
        self.state.status = Status::Starting;
        self.state.history_loading = true;
        self.state.extension_dialogs.clear();
        self.lose_queue();
        self.active_assistant = None;
        self.startup_deadline = Some(self.now.saturating_add(REQUEST_TIMEOUT_MS));
        self.stop_deadline = None;
        vec![
            self.send("get_state", json!({}), false),
            self.send("get_messages", json!({}), false),
            self.send("get_available_models", json!({}), false),
            self.send("get_commands", json!({}), false),
        ]
    }
    /// All process records must enter here unless the caller already fenced runtime and generation.
    pub fn ingest(&mut self, runtime_id: &str, generation: u64, record: Value) -> Vec<Effect> {
        if runtime_id != self.runtime_id || generation != self.generation || generation == 0 {
            return vec![];
        }
        self.handle(record)
    }
    pub fn handle(&mut self, v: Value) -> Vec<Effect> {
        if self.generation == 0 || !self.connected {
            return vec![];
        }
        let mut effects = vec![];
        match v["type"].as_str().unwrap_or_default() {
            "response" => return self.response(v),
            "agent_start" => {
                self.confirm_prompt();
                self.state.working = true;
                self.revision += 1;
                effects.push(self.send("get_state", json!({}), false));
            }
            "message_start" => {
                let m = &v["message"];
                let fallback = self.next_id();
                let key = occurrence_key(m, &fallback, &mut self.message_occurrences);
                match m["role"].as_str() {
                    Some("user") => {
                        self.confirm_prompt();
                        let text = content_text(&m["content"]);
                        if let Some(r) = self.state.transcript.iter_mut().rev().find(|r| {
                            r.kind == "user" && (r.pending || r.source.starts_with("optimistic:"))
                        }) {
                            r.replace_text(text);
                            r.source = key;
                            r.pending = false;
                            r.touch();
                        } else {
                            self.merge_rows(project(m, &key), false);
                        }
                    }
                    Some("assistant") => {
                        self.confirm_prompt();
                        self.active_assistant = Some(key.clone());
                        self.merge_rows(project(m, &key), true);
                    }
                    _ => {}
                }
                self.revision += 1;
            }
            "message_update" => {
                self.stream(&v["assistantMessageEvent"]);
                self.revision += 1;
            }
            "message_end" => {
                let m = &v["message"];
                if m["role"] == "assistant" {
                    let key = if let Some(key) = self.active_assistant.clone() {
                        key
                    } else {
                        let fallback = self.next_id();
                        occurrence_key(m, &fallback, &mut self.message_occurrences)
                    };
                    self.merge_rows(project(m, &key), false);
                    self.active_assistant = None;
                    self.revision += 1;
                    if !self
                        .pending
                        .values()
                        .any(|p| p.barrier && p.epoch == self.epoch)
                    {
                        effects.push(self.send("get_state", json!({}), true));
                    }
                } else if m["role"] == "toolResult" {
                    let key = message_key(m, "result");
                    self.merge_rows(project(m, &key), false);
                    self.revision += 1;
                }
            }
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                self.tool(&v);
                self.revision += 1;
            }
            "agent_settled" => {
                self.state.working = false;
                self.state.stopping = false;
                self.stop_deadline = None;
                self.finalize_partial();
                self.revision += 1;
                effects.push(self.send("get_state", json!({}), false));
            }
            "queue_update" => {
                if let (Some(s), Some(f)) = (strings(&v["steering"]), strings(&v["followUp"])) {
                    self.state.queue = QueueSnapshot {
                        steering: s,
                        follow_up: f,
                    };
                    self.queue_version += 1;
                }
            }
            "extension_ui_request" => self.extension(&v),
            "session_info_changed" => effects.push(self.send("get_state", json!({}), false)),
            "thinking_level_changed" => self.state.effort = string(&v["level"]),
            "extension_error" => {
                self.fail("A Pi extension failed. Review the extension setup and try again.")
            }
            "compaction_start" | "auto_retry_start" | "summarization_retry_scheduled" => {
                self.state.working = true
            }
            "compaction_end" => {
                if v["result"].is_object() {
                    effects.push(self.send("get_messages", json!({}), false));
                } else if v["errorMessage"].is_string() {
                    self.fail(
                        "The conversation could not be shortened. Try another model or session.",
                    );
                }
            }
            _ => {}
        }
        self.update_status();
        effects
    }
    fn response(&mut self, v: Value) -> Vec<Effect> {
        let id = string(&v["id"]);
        let Some(p) = self.pending.get(&id).cloned() else {
            return vec![];
        };
        if v["command"] != p.command {
            return vec![];
        }
        self.pending.remove(&id);
        if p.epoch != self.epoch || !p.identity.binding_eq(&self.state.identity) {
            return vec![];
        }
        if !p.barrier && p.command != "prompt" && self.latest.get(&p.command) != Some(&id) {
            return vec![];
        }
        if v["success"] != true {
            self.reject(p, false);
            self.update_status();
            return vec![];
        }
        let d = &v["data"];
        let mut effects = vec![];
        match p.command.as_str() {
            "get_state" => {
                if !d.is_object() {
                    self.reject(p, false);
                    return vec![];
                }
                let current = Identity {
                    id: string(&d["sessionId"]),
                    path: string(&d["sessionFile"]),
                    name: string(&d["sessionName"]),
                };
                let changed = !current.binding_eq(&self.state.identity);
                // A barrier proves the captured predecessor was appended even if Pi already switched.
                if p.barrier && p.identity.saved_candidate() {
                    self.register(&p.identity, &mut effects);
                }
                if p.serial < self.state_serial {
                    return effects;
                }
                self.state_serial = p.serial;
                if changed {
                    let previous = self.state.identity.clone();
                    let missing_source = previous.saved_candidate()
                        && previous.path == current.path
                        && previous.id != current.id;
                    self.recover_pending();
                    self.pending.clear();
                    self.latest.clear();
                    self.epoch += 1;
                    self.state.transcript.clear();
                    self.message_occurrences.clear();
                    self.active_assistant = None;
                    self.state.extension_dialogs.clear();
                    self.lose_queue();
                    self.state.durable = false;
                    self.state.identity = current.clone();
                    effects.push(Effect::IdentityChanged { previous, current });
                    effects.push(self.send("get_available_models", json!({}), false));
                    effects.push(self.send("get_commands", json!({}), false));
                    if missing_source {
                        self.fail("Pi opened a new identity at the requested path. The old session may not have been saved.");
                    }
                } else {
                    self.state.identity.name = current.name;
                }
                self.state.model = d["model"].clone();
                self.state.effort = string(&d["thinkingLevel"]);
                // A durability read is not a run-lifecycle signal; a later event may already have started work.
                if !p.barrier && p.revision == self.revision {
                    self.state.working = d["isStreaming"] == true || d["isCompacting"] == true;
                }
                self.state.ready = true;
                if p.barrier && !changed {
                    self.state.durable = self.state.identity.saved_candidate();
                }
                if changed || !p.barrier {
                    self.state.history_loading = true;
                    effects.push(self.send("get_messages", json!({}), false));
                }
                effects.push(self.send("get_available_thinking_levels", json!({}), false));
            }
            "get_messages" => {
                if p.revision != self.revision {
                    self.state.history_loading = false;
                    if !self.state.working {
                        return vec![self.send("get_messages", json!({}), false)];
                    }
                    return vec![];
                }
                if let Some(messages) = d["messages"].as_array() {
                    let old = self.state.transcript.clone();
                    self.state.transcript.clear();
                    let mut occurrences = BTreeMap::new();
                    for (i, m) in messages.iter().enumerate() {
                        let key = occurrence_key(m, &format!("history-{i}"), &mut occurrences);
                        let mut rows = project(m, &key);
                        for r in &mut rows {
                            if let Some(prior) = old
                                .iter()
                                .find(|o| o.source == r.source && o.kind == r.kind)
                            {
                                r.id = prior.id.clone();
                                r.revision = prior.revision + 1;
                                r.text_reset_revision = r.revision;
                            } else if let Some(prior) = old.iter().find(|o| {
                                o.kind == r.kind
                                    && o.text == r.text
                                    && o.source.starts_with("optimistic:")
                            }) {
                                r.id = prior.id.clone();
                                r.revision = prior.revision + 1;
                                r.text_reset_revision = r.revision;
                            }
                        }
                        self.merge_rows(rows, false);
                    }
                    self.message_occurrences = occurrences;
                    // Local notices and failed/uncertain partial output are not in Pi history.
                    for r in old {
                        if (r.source.starts_with("local:") || r.interrupted)
                            && !self.state.transcript.iter().any(|n| n.id == r.id)
                        {
                            self.state.transcript.push(r);
                        }
                    }
                    self.state.history_loading = false;
                } else {
                    self.reject(p, false);
                }
            }
            "get_available_models" => self.state.models = array(&d["models"]),
            "get_commands" => self.state.commands = array(&d["commands"]),
            "get_available_thinking_levels" => {
                self.state.thinking_levels = strings(&d["levels"]).unwrap_or_default()
            }
            "prompt" => {
                if let Some(rid) = &p.optimistic {
                    if let Some(r) = self.state.transcript.iter_mut().find(|r| &r.id == rid) {
                        r.pending = false;
                        r.touch();
                    }
                }
                if d["disposition"] == "handled" {
                    self.admission = None;
                    if !p.accepted {
                        self.state.working = false;
                    }
                    effects.push(self.send("get_state", json!({}), false));
                } else if d["disposition"] == "started" && p.revision == self.revision {
                    self.state.working = true;
                }
            }
            "abort" => {
                self.state.stopping = false;
                self.stop_deadline = None;
                if p.revision == self.revision {
                    self.state.working = false;
                    self.finalize_partial();
                }
                effects.push(self.send("get_state", json!({}), false));
            }
            "set_model" | "set_thinking_level" => {
                effects.push(self.send("get_state", json!({}), false))
            }
            "set_steering_mode" | "set_follow_up_mode" => effects.extend(self.continue_queue()),
            "clear_queue" if p.payload["queueVersion"].as_u64() == Some(self.queue_version) => {
                self.state.queue = QueueSnapshot::default();
            }
            _ => {}
        }
        if self.state.ready && !self.state.history_loading {
            self.startup_deadline = None;
        }
        self.update_status();
        effects
    }
    fn register(&mut self, identity: &Identity, effects: &mut Vec<Effect>) {
        if identity.saved_candidate() && identity.binding_eq(&self.state.identity) {
            self.state.durable = true;
        }
        if identity.saved_candidate()
            && self
                .registered
                .insert((identity.id.clone(), identity.path.clone()))
        {
            effects.push(Effect::RegisterSession {
                runtime_id: self.runtime_id.clone(),
                generation: self.generation,
                id: identity.id.clone(),
                path: identity.path.clone(),
                name: identity.name.clone(),
            });
        }
    }
    pub fn submit(&mut self, text: impl Into<String>) -> Vec<Effect> {
        self.submit_inner(text.into(), None)
    }
    fn submit_inner(&mut self, draft: String, queue: Option<QueueKind>) -> Vec<Effect> {
        if !self.state.ready
            || self.state.stopping
            || !self.state.extension_dialogs.is_empty()
            || draft.trim().is_empty()
            || self.pending.values().any(|p| p.command == "prompt")
            || (self.state.working && queue.is_none())
        {
            return vec![];
        }
        let text = draft.trim();
        let name = text
            .strip_prefix('/')
            .unwrap_or_default()
            .split(' ')
            .next()
            .unwrap_or_default();
        let command = self
            .state
            .commands
            .iter()
            .any(|c| c["name"] == name && c["source"] == "extension");
        if queue.is_some() && command {
            self.fail("Extension commands cannot be queued. Wait for the current work to finish.");
            return vec![];
        }
        let mut payload = json!({"message":text});
        if let Some(kind) = queue {
            payload["streamingBehavior"] = json!(match kind {
                QueueKind::Steer => "steer",
                QueueKind::FollowUp => "followUp",
            });
        }
        let effect = self.send("prompt", payload, false);
        let Effect::Send(v) = &effect else {
            return vec![];
        };
        let id = string(&v["id"]);
        let optimistic = if !command && queue.is_none() {
            let rid = self.next_id();
            let mut r = Entry::new(rid.clone(), "user", text.into(), format!("optimistic:{id}"));
            r.pending = true;
            self.state.transcript.push(r);
            Some(rid)
        } else {
            None
        };
        if queue.is_none() {
            self.admission = Some((self.now.saturating_add(REQUEST_TIMEOUT_MS), draft.clone()));
        }
        if let Some(p) = self.pending.get_mut(&id) {
            p.draft = Some(draft);
            p.optimistic = optimistic;
        }
        self.state.draft.clear();
        self.state.error = None;
        if queue.is_none() {
            self.state.working = true;
        }
        self.update_status();
        vec![effect]
    }
    pub fn submit_queued(&mut self, text: impl Into<String>, kind: QueueKind) -> Vec<Effect> {
        if !self.state.working {
            return self.submit(text);
        }
        if !self.state.ready || self.state.stopping || !self.state.extension_dialogs.is_empty() {
            return vec![];
        }
        if self.queue_submission.is_some() || self.pending.values().any(|p| p.command == "prompt") {
            return vec![];
        }
        self.latest.remove("set_follow_up_mode");
        self.queue_submission = Some((text.into(), kind));
        // Pi persists these delivery defaults. Serialize confirmations before the prompt.
        vec![self.send("set_steering_mode", json!({"mode":"all"}), false)]
    }
    fn continue_queue(&mut self) -> Vec<Effect> {
        if self.queue_submission.is_none() {
            return vec![];
        }
        if !self.latest.contains_key("set_follow_up_mode") {
            return vec![self.send("set_follow_up_mode", json!({"mode":"one-at-a-time"}), false)];
        }
        let Some((text, kind)) = self.queue_submission.take() else {
            return vec![];
        };
        self.latest.remove("set_follow_up_mode");
        self.submit_inner(text, Some(kind))
    }
    pub fn abort(&mut self) -> Vec<Effect> {
        if !self.state.working || self.state.stopping {
            return vec![];
        }
        self.state.stopping = true;
        self.stop_deadline = Some(self.now.saturating_add(STOP_TIMEOUT_MS));
        self.update_status();
        vec![self.send("abort", json!({}), false)]
    }
    pub fn set_model(&mut self, provider: &str, model_id: &str) -> Vec<Effect> {
        if !self.state.ready {
            return vec![];
        }
        vec![self.send(
            "set_model",
            json!({"provider":provider,"modelId":model_id}),
            false,
        )]
    }
    pub fn set_thinking(&mut self, level: &str) -> Vec<Effect> {
        if !self.state.ready {
            return vec![];
        }
        vec![self.send("set_thinking_level", json!({"level":level}), false)]
    }
    pub fn refresh(&mut self) -> Vec<Effect> {
        if self.generation == 0 || !self.connected {
            return vec![];
        }
        self.state.error = None;
        vec![self.send("get_state", json!({}), false)]
    }
    pub fn queue_snapshot(&self) -> &QueueSnapshot {
        &self.state.queue
    }
    pub fn clear_queue(&mut self) -> Vec<Effect> {
        if !self.state.ready
            || self
                .pending
                .values()
                .any(|p| p.command == "clear_queue" || p.command == "prompt")
            || self.queue_submission.is_some()
        {
            return vec![];
        }
        let effect = self.send("clear_queue", json!({}), false);
        if let Effect::Send(v) = &effect {
            if let Some(p) = self.pending.get_mut(v["id"].as_str().unwrap_or_default()) {
                p.payload["queueVersion"] = json!(self.queue_version);
            }
        }
        vec![effect]
    }
    pub fn extension_response(&mut self, id: &str, response: ExtensionResponse) -> Vec<Effect> {
        let Some(index) = self.state.extension_dialogs.iter().position(|d| d.id == id) else {
            return vec![];
        };
        let method = string(&self.state.extension_dialogs[index].request["method"]);
        let mut v = json!({"type":"extension_ui_response","id":id});
        match response {
            ExtensionResponse::Value(text) if method != "confirm" => v["value"] = json!(text),
            ExtensionResponse::Confirmed(b) if method == "confirm" => v["confirmed"] = json!(b),
            ExtensionResponse::Cancelled => v["cancelled"] = json!(true),
            _ => return vec![],
        }
        if self.state.extension_dialogs[index].submitting {
            return vec![];
        }
        self.state.extension_dialogs[index].submitting = true;
        vec![Effect::Send(v)]
    }
    /// Extension responses have no Pi response envelope. Confirm host transport delivery explicitly.
    pub fn extension_sent(&mut self, id: &str, success: bool) -> Vec<Effect> {
        let Some(index) = self
            .state
            .extension_dialogs
            .iter()
            .position(|d| d.id == id && d.submitting)
        else {
            return vec![];
        };
        if !success {
            self.state.extension_dialogs[index].submitting = false;
            self.fail("The extension response could not be sent. Try again.");
            self.update_status();
            return vec![];
        }
        self.state.extension_dialogs.remove(index);
        vec![self.send("get_state", json!({}), false)]
    }
    fn extension(&mut self, v: &Value) {
        let id = string(&v["id"]);
        if id.is_empty() {
            return;
        }
        match v["method"].as_str().unwrap_or_default() {
            "select" | "confirm" | "input" | "editor" => {
                if !self.state.extension_dialogs.iter().any(|d| d.id == id) {
                    self.state.extension_dialogs.push(ExtensionDialog {
                        id,
                        request: v.clone(),
                        submitting: false,
                        deadline_ms: v["timeout"]
                            .as_u64()
                            .filter(|n| *n > 0)
                            .map(|n| self.now.saturating_add(n)),
                    });
                }
            }
            "set_editor_text" => self.state.draft = string(&v["text"]),
            "notify" => {
                let source = format!("local:notify:{id}");
                if !self.state.transcript.iter().any(|r| r.source == source) {
                    let rid = self.next_id();
                    self.state.transcript.push(Entry::new(
                        rid,
                        "notice",
                        string(&v["message"]),
                        source,
                    ));
                }
            }
            _ => {}
        }
    }
    fn stream(&mut self, v: &Value) {
        let key = if let Some(key) = &self.active_assistant {
            key.clone()
        } else {
            let key = self.next_id();
            self.active_assistant = Some(key.clone());
            key
        };
        let index = v["contentIndex"].as_u64().unwrap_or(0);
        let typ = v["type"].as_str().unwrap_or_default();
        let kind = if typ.starts_with("thinking_") {
            "thinking"
        } else if typ.starts_with("text_") {
            "assistant"
        } else {
            return;
        };
        let source = format!("{key}:{index}");
        if !self.state.transcript.iter().any(|r| r.source == source) {
            let id = self.next_id();
            let mut r = Entry::new(id, kind, String::new(), source.clone());
            r.activity = true;
            self.state.transcript.push(r);
        }
        if let Some(r) = self
            .state
            .transcript
            .iter_mut()
            .find(|r| r.source == source)
        {
            if typ.ends_with("_delta") {
                r.append_text(v["delta"].as_str().unwrap_or_default());
            }
            if typ.ends_with("_end") {
                r.replace_text(string(&v["content"]));
            }
        }
    }
    fn tool(&mut self, v: &Value) {
        let call = string(&v["toolCallId"]);
        if call.is_empty() {
            return;
        }
        let source = format!("tool:{call}");
        if !self.state.transcript.iter().any(|r| r.source == source) {
            let id = self.next_id();
            let mut r = Entry::new(id, "tool", String::new(), source.clone());
            r.tool_call_id = Some(call);
            r.tool_name = v["toolName"].as_str().map(str::to_owned);
            self.state.transcript.push(r);
        }
        if let Some(r) = self
            .state
            .transcript
            .iter_mut()
            .find(|r| r.source == source)
        {
            r.touch();
            if !v["args"].is_null() {
                r.arguments = v["args"].clone();
            }
            r.activity = v["type"] != "tool_execution_end";
            if v["type"] == "tool_execution_update" {
                r.result = content_text(&v["partialResult"]["content"]);
            }
            if v["type"] == "tool_execution_end" {
                r.result = content_text(&v["result"]["content"]);
                r.is_error = v["isError"] == true;
            }
        }
    }
    fn merge_rows(&mut self, rows: Vec<Entry>, activity: bool) {
        for mut row in rows {
            if let Some(old) = self
                .state
                .transcript
                .iter_mut()
                .find(|r| r.source == row.source && r.kind == row.kind)
            {
                row.id = old.id.clone();
                row.activity = activity;
                if row.kind == "tool" && row.result.is_empty() {
                    row.result = old.result.clone();
                    row.is_error = old.is_error;
                    row.activity = old.activity;
                }
                if row.kind == "tool" && row.arguments.is_null() {
                    row.arguments = old.arguments.clone();
                }
                row.revision = old.revision + 1;
                row.text_reset_revision = row.revision;
                *old = row;
            } else {
                if row.id.is_empty() {
                    row.id = self.next_id();
                }
                row.activity = activity;
                self.state.transcript.push(row);
            }
        }
    }
    fn confirm_prompt(&mut self) {
        self.admission = None;
        for p in self
            .pending
            .values_mut()
            .filter(|p| p.command == "prompt" && p.payload["streamingBehavior"].is_null())
        {
            p.accepted = true;
        }
    }
    fn restore(&mut self, text: String) {
        if self.state.draft == text {
            return;
        }
        if self.state.draft.is_empty() {
            self.state.draft = text;
        } else {
            self.state.draft = format!("{text}\n\n{}", self.state.draft);
        }
    }
    fn recover_pending(&mut self) {
        let ps: Vec<_> = self.pending.values().cloned().collect();
        for p in ps {
            if !p.accepted {
                if let Some(draft) = p.draft {
                    self.restore(draft);
                }
                if let Some(id) = p.optimistic {
                    self.state.transcript.retain(|r| r.id != id);
                }
            }
        }
        if let Some((draft, _)) = self.queue_submission.take() {
            self.restore(draft);
        }
    }
    fn reject(&mut self, p: Pending, timeout: bool) {
        if p.command == "prompt" {
            self.admission = None;
        }
        if !p.accepted {
            if let Some(draft) = p.draft {
                if timeout {
                    self.state.recovered_drafts.push(draft);
                } else {
                    self.restore(draft);
                }
            }
            if let Some(id) = p.optimistic {
                self.state.transcript.retain(|r| r.id != id);
            }
        }
        if p.command == "prompt" && !p.accepted && p.payload["streamingBehavior"].is_null() {
            self.state.working = false;
        }
        if p.command == "abort" {
            self.state.stopping = false;
            self.stop_deadline = None;
            self.finalize_partial();
        }
        if p.command == "get_messages" {
            self.state.history_loading = false;
        }
        if p.command.starts_with("set_") {
            if let Some((draft, _)) = self.queue_submission.take() {
                self.restore(draft);
            }
        }
        self.fail(match p.command.as_str() {
            "prompt" if timeout => {
                "Message confirmation was lost. Check the transcript before resending."
            }
            "prompt" => "The message was not accepted. Review the draft and try again.",
            "abort" => "Stop was not confirmed. Pi may still be working. Try Stop again.",
            "clear_queue" => "Could not clear the queue. Check the Pi version and retry.",
            "set_model" | "set_thinking_level" => {
                "The setting was not confirmed. Refresh the session and try again."
            }
            _ => "The Pi request failed or timed out. Refresh the session and try again.",
        });
    }
    pub fn request_failed(&mut self, id: &str) -> Vec<Effect> {
        if let Some(p) = self.pending.remove(id) {
            if p.epoch == self.epoch
                && p.identity.binding_eq(&self.state.identity)
                && (p.barrier
                    || p.command == "prompt"
                    || self
                        .latest
                        .get(&p.command)
                        .is_some_and(|latest| latest == id))
            {
                self.reject(p, false);
            }
        }
        self.update_status();
        vec![]
    }
    pub fn transport_failed(&mut self, generation: u64) -> Vec<Effect> {
        if generation != self.generation {
            return vec![];
        }
        self.connected = false;
        self.admission = None;
        self.recover_pending();
        self.pending.clear();
        self.latest.clear();
        self.finalize_partial();
        self.lose_queue();
        self.state.extension_dialogs.clear();
        self.state.ready = false;
        self.state.working = false;
        self.state.stopping = false;
        self.state.history_loading = false;
        self.startup_deadline = None;
        self.stop_deadline = None;
        self.fail("The Pi connection was lost. Reconnect before sending more work.");
        self.update_status();
        vec![]
    }
    /// Nondecreasing caller-supplied logical milliseconds; no wall clock or timers inside the crate.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        self.now = self.now.max(now_ms);
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline <= self.now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(p) = self.pending.remove(&id) {
                if p.epoch == self.epoch
                    && (p.barrier
                        || p.command == "prompt"
                        || self.latest.get(&p.command) == Some(&id))
                {
                    self.reject(p, true);
                }
            }
        }
        if self
            .admission
            .as_ref()
            .is_some_and(|(deadline, _)| *deadline <= self.now)
        {
            if let Some((_, draft)) = self.admission.take() {
                self.state.recovered_drafts.push(draft);
            }
            self.state.working = false;
            self.fail("Pi accepted the message but did not start work. Check the transcript before resending.");
        }
        self.state
            .extension_dialogs
            .retain(|d| d.deadline_ms.is_none_or(|n| n > self.now));
        if self.startup_deadline.is_some_and(|n| n <= self.now) {
            self.startup_deadline = None;
            self.state.ready = false;
            self.state.history_loading = false;
            self.fail("Pi did not become ready. Restart the connection and try again.");
        }
        if self.stop_deadline.is_some_and(|n| n <= self.now) {
            self.stop_deadline = None;
            self.state.stopping = false;
            self.finalize_partial();
            self.fail("Stop was not confirmed. Pi may still be working. Try Stop again.");
        }
        self.update_status();
        vec![]
    }
    fn finalize_partial(&mut self) {
        for r in &mut self.state.transcript {
            if r.activity {
                r.activity = false;
                r.interrupted = true;
                r.touch();
            }
        }
        self.active_assistant = None;
    }
    fn lose_queue(&mut self) {
        if !self.state.queue.steering.is_empty() || !self.state.queue.follow_up.is_empty() {
            self.state
                .recovered_drafts
                .extend(self.state.queue.steering.clone());
            self.state
                .recovered_drafts
                .extend(self.state.queue.follow_up.clone());
        }
        self.state.queue = QueueSnapshot::default();
    }
    fn fail(&mut self, message: &str) {
        self.state.error = Some(message.into());
    }
    fn update_status(&mut self) {
        self.state.status = if self.state.stopping {
            Status::Stopping
        } else if self.state.error.is_some() {
            Status::Error
        } else if self.state.working {
            Status::Working
        } else if self.state.ready {
            Status::Ready
        } else {
            Status::Starting
        };
    }
}
fn occurrence_key(m: &Value, fallback: &str, counts: &mut BTreeMap<String, usize>) -> String {
    let base = message_key(m, fallback);
    if m["role"] == "toolResult" {
        return base;
    }
    let count = counts.entry(base.clone()).or_default();
    let key = format!("{base}:occ{count}");
    *count += 1;
    key
}
fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn stream_revisions_leave_large_history_sources_untouched() {
        let mut controller = Controller::new("test");
        controller.begin_generation(1);
        for i in 0..2000 {
            controller.state.transcript.push(Entry::new(
                format!("history-{i}"),
                "assistant",
                "history".repeat(1000),
                format!("history:{i}"),
            ));
        }
        let pointers: Vec<_> = controller
            .state
            .transcript
            .iter()
            .map(|r| r.text.as_ptr())
            .collect();
        controller.handle(json!({"type":"message_start", "message":{"role":"assistant", "timestamp":1,"content":[]}}));
        for (kind, index) in [("text", 0), ("thinking", 1)] {
            for _ in 0..10 {
                controller.handle(json!({"type":"message_update", "assistantMessageEvent":{
                    "type":format!("{kind}_delta"), "contentIndex":index, "delta":"delta"
                }}));
            }
        }
        controller
            .handle(json!({"type":"tool_execution_start", "toolCallId":"call", "toolName":"bash"}));
        controller.handle(json!({"type":"tool_execution_update", "toolCallId":"call", "partialResult":{"content":[{"type":"text","text":"output"}]}}));
        for (row, pointer) in controller.state.transcript.iter().take(2000).zip(pointers) {
            assert_eq!(row.revision, 0);
            assert_eq!(row.text.as_ptr(), pointer);
        }
        let prose = &controller.state.transcript[2000];
        assert_eq!(prose.revision, 10);
        assert_eq!(prose.text_reset_revision, 0);
        assert_eq!(controller.state.transcript[2001].revision, 10);
        assert_eq!(controller.state.transcript[2002].revision, 2);
        controller.handle(json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_end", "contentIndex":0, "content":"replacement"
        }}));
        let prose = &controller.state.transcript[2000];
        assert_eq!(prose.text_reset_revision, prose.revision);
        let reset = prose.revision;
        controller.handle(json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":0, "delta":" appended"
        }}));
        assert_eq!(controller.state.transcript[2000].text_reset_revision, reset);
        assert_eq!(controller.state.transcript[2000].revision, reset + 1);
    }

    #[test]
    fn authoritative_merge_and_finalization_advance_row_metadata() {
        let mut controller = Controller::new("test");
        controller.begin_generation(1);
        let message =
            json!({"role":"assistant", "timestamp":1,"content":[{"type":"text","text":"initial"}]});
        controller.handle(json!({"type":"message_start", "message":message}));
        let id = controller.state.transcript[0].id.clone();
        controller.handle(json!({"type":"message_end", "message":message}));
        let row = &controller.state.transcript[0];
        assert_eq!(row.id, id);
        assert_eq!(row.revision, 1);
        assert_eq!(row.text_reset_revision, 1);
        controller.state.transcript[0].activity = true;
        controller.finalize_partial();
        assert_eq!(controller.state.transcript[0].revision, 2);
        assert!(controller.state.transcript[0].interrupted);
    }
}
