use crate::model_scope::{resolved_scope, scope_models, valid_model};
use crate::transcript::{
    content_text, history_layers, history_prefix, message_key, project, string, Entry, EntryOrigin,
    HistoryLayer,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

#[cfg(test)]
#[path = "session_notice_tests.rs"]
mod notice_tests;

#[path = "session_input_failure.rs"]
mod input_failure;
#[path = "session_reconciliation.rs"]
mod reconciliation;
use reconciliation::{ReadPurpose, Reconciliation};

fn meaningful_completed_assistant(message: &Value) -> bool {
    message["role"] == "assistant"
        && !matches!(message["stopReason"].as_str(), Some("error" | "aborted"))
        && project(message, "proof").iter().any(|row| {
            (matches!(row.kind.as_str(), "assistant" | "thinking") && !row.text.trim().is_empty())
                || (row.kind == "tool"
                    && row
                        .tool_name
                        .as_ref()
                        .is_some_and(|name| !name.trim().is_empty()))
        })
}

fn replace_changed<T: PartialEq>(current: &mut T, next: T) -> bool {
    if *current == next {
        return false;
    }
    *current = next;
    true
}

pub const REQUEST_TIMEOUT_MS: u64 = 10_000;
pub const STOP_TIMEOUT_MS: u64 = 10_000;

pub fn normalize_session_name(name: &str) -> String {
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut units = 0;
    normalized
        .chars()
        .take_while(|ch| {
            units += ch.len_utf16();
            units <= 240
        })
        .collect()
}
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
    SessionNameChanged {
        identity: Identity,
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
/// Reviewed, content-free auto-retry presentation. Never retain the provider error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryReason {
    RateLimit,
    Unavailable,
    Network,
    Failed,
}
impl RetryReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::RateLimit => "The model provider is receiving too many requests.",
            Self::Unavailable => "The model provider is temporarily unavailable.",
            Self::Network => "The model provider could not be reached.",
            Self::Failed => "The reply failed.",
        }
    }
    fn from_provider_error(raw: &str) -> Self {
        let text = raw.to_lowercase();
        let has = |terms: &[&str]| terms.iter().any(|term| text.contains(term));
        let code = |n: &str| {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == n)
        };
        // Match Vue's classification precedence; credentials/credit are never rate-limit copy.
        if code("401")
            || has(&[
                "unauthorized",
                "unauthorised",
                "authentication",
                "api key",
                "api_key",
                "api-key",
                "apikey",
                "credential",
                "insufficient_quota",
                "insufficient quota",
                "insufficient-quota",
                "insufficientquota",
                "credit",
                "billing",
                "insufficient balance",
                "insufficient funds",
                "exceeded your current quota",
            ])
        {
            Self::Failed
        } else if code("429")
            || has(&[
                "rate limit",
                "rate_limit",
                "rate-limit",
                "ratelimit",
                "too many requests",
            ])
        {
            Self::RateLimit
        } else if code("402")
            || has(&[
                "quota",
                "context length",
                "context limit",
                "context window",
                "maximum context",
                "max tokens",
                "maximum tokens",
                "conversation is too long",
                "unsupported model",
                "does not exist",
            ])
            || text.lines().any(|line| {
                line.contains("model")
                    && ["not found", "unavailable", "not available", "access denied"]
                        .iter()
                        .any(|term| line.contains(term))
            })
        {
            Self::Failed
        } else if has(&[
            "network",
            "fetch failed",
            "connection failed",
            "connection reset",
            "connection refused",
            "time out",
            "timed out",
            "timeout",
            "dns",
            "socket",
            "unreachable",
        ]) {
            Self::Network
        } else if ["502", "503", "504", "529"].iter().any(|n| code(n))
            || has(&[
                "overload",
                "temporarily unavailable",
                "server unavailable",
                "capacity",
            ])
        {
            Self::Unavailable
        } else {
            Self::Failed
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryState {
    pub attempt: Option<u64>,
    pub max_attempts: Option<u64>,
    pub reason: RetryReason,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub identity: Identity,
    pub model: Value,
    pub effort: String,
    pub status: Status,
    pub ready: bool,
    pub working: bool,
    #[serde(default)]
    pub compacting: bool,
    pub stopping: bool,
    #[serde(default)]
    pub retry: Option<RetryState>,
    pub history_loading: bool,
    pub transcript: Vec<Entry>,
    pub draft: String,
    pub recovered_drafts: Vec<String>,
    pub queue: QueueSnapshot,
    pub extension_dialogs: Vec<ExtensionDialog>,
    pub models: Vec<Value>,
    #[serde(default)]
    model_scope: Vec<String>,
    #[serde(default)]
    scoped_models: Option<Vec<Value>>,
    pub commands: Vec<Value>,
    pub thinking_levels: Vec<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub queue_feedback: Option<String>,
    pub durable: bool,
}
impl State {
    /// Selectable catalogue, in scope order. The current model remains display-only
    /// when unavailable or outside the scope, matching Vue's fallback label.
    pub fn selectable_models(&self) -> Vec<&Value> {
        if let Some(scope) = &self.scoped_models {
            // Keep the first catalogue entry for each identity, as the old lookup did.
            let mut catalogue = HashMap::with_capacity(self.models.len());
            for (index, model) in self.models.iter().enumerate() {
                if let (Some(provider), Some(id)) =
                    (model["provider"].as_str(), model["id"].as_str())
                {
                    catalogue.entry((provider, id)).or_insert(index);
                }
            }
            let mut seen = HashSet::with_capacity(scope.len());
            let selected: Vec<_> = scope
                .iter()
                .filter_map(|scoped| {
                    let identity = (scoped["provider"].as_str()?, scoped["id"].as_str()?);
                    let index = *catalogue.get(&identity)?;
                    seen.insert(index).then(|| &self.models[index])
                })
                .collect();
            if !selected.is_empty() {
                return selected;
            }
        }
        scope_models(&self.models, &self.model_scope)
    }
}

/// Capture before a host settings read; stale completion cannot mutate scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelScopeToken {
    generation: u64,
    epoch: u64,
    revision: u64,
}

/// Opaque ownership of one displayed error occurrence, not a workflow acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorToken {
    runtime_id: String,
    generation: u64,
    epoch: u64,
    revision: u64,
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
    completed_assistant: bool,
    draft: Option<String>,
    optimistic: Option<String>,
    accepted: bool,
    write_accepted: bool,
    previous_name: Option<String>,
    purpose: ReadPurpose,
    extension_command: bool,
    command_incarnation: u64,
}
/// Pure protocol state. The host owns IO, persistence, rendering and scheduling.
#[path = "session_saved_preview.rs"]
mod saved_preview;
pub use saved_preview::SavedPreviewToken;

/// Display-only snapshot. It carries no runtime identity, readiness or admission.
#[derive(Clone)]
pub struct SettingsPresentation {
    pub models: Vec<Value>,
    pub model: Value,
    pub efforts: Vec<String>,
    pub effort: String,
}

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
    materialisation: BTreeMap<(String, String), (Identity, u64)>,
    startup_deadline: Option<u64>,
    stop_deadline: Option<u64>,
    admission: Option<(u64, String)>,
    queue_submission: Option<(String, QueueKind)>,
    queue_version: u64,
    state_serial: u64,
    connected: bool,
    scope_revision: u64,
    model_choices_revision: u64,
    commands_revision: u64,
    commands_loaded: bool,
    scope_known: bool,
    model_state_known: bool,
    presentation_state_floor: u64,
    catalogue_known: bool,
    runtime_scope_settled: bool,
    efforts_known: bool,
    // Idle compaction must not invent a stream when it finishes.
    compaction_working: bool,
    history_layers: Vec<HistoryLayer>,
    first_history_layer: usize,
    history_prefix_len: usize,
    compaction_snapshot: Option<HashMap<String, u64>>,
    error_revision: u64,
    extension_error_owner: Option<(u64, String, u64)>,
    reconciliation: Reconciliation,
    saved_preview: saved_preview::Admission,
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
            materialisation: BTreeMap::new(),
            startup_deadline: None,
            stop_deadline: None,
            admission: None,
            queue_submission: None,
            queue_version: 0,
            state_serial: 0,
            connected: false,
            scope_revision: 0,
            model_choices_revision: 0,
            commands_revision: 0,
            commands_loaded: false,
            scope_known: false,
            model_state_known: false,
            presentation_state_floor: 0,
            catalogue_known: false,
            runtime_scope_settled: false,
            efforts_known: false,
            compaction_working: false,
            history_layers: vec![],
            first_history_layer: 0,
            history_prefix_len: 0,
            compaction_snapshot: None,
            error_revision: 0,
            extension_error_owner: None,
            reconciliation: Reconciliation::default(),
            saved_preview: saved_preview::Admission::default(),
        }
    }
    fn reset_history(&mut self) {
        let prefix = self.history_prefix_len.min(self.state.transcript.len());
        self.state.transcript.drain(..prefix);
        self.history_prefix_len = 0;
        self.history_layers.clear();
        self.first_history_layer = 0;
        self.pending.retain(|_, p| p.command != "get_entries");
        self.latest.remove("get_entries");
        for row in &mut self.state.transcript {
            if row.kind == "compaction" {
                row.history_available = false;
                row.history_loading = false;
                row.touch();
            }
        }
    }
    fn set_history_loading(&mut self, loading: bool) {
        if let Some(marker) = self
            .state
            .transcript
            .iter_mut()
            .find(|r| r.kind == "compaction" && r.history_available)
        {
            marker.history_loading = loading;
            marker.touch();
        }
    }
    fn apply_history_prefix(&mut self) {
        if self.history_layers.is_empty() {
            if self.pending.values().any(|p| p.command == "get_entries") {
                self.set_history_loading(true);
            }
            return;
        }
        for row in &mut self.state.transcript {
            if row.kind == "compaction" {
                row.history_available = false;
                row.history_loading = false;
                row.touch();
            }
        }
        let mut prefix = history_prefix(&self.history_layers, self.first_history_layer);
        self.history_prefix_len = prefix.len();
        prefix.append(&mut self.state.transcript);
        self.state.transcript = prefix;
    }
    /// Activate only the current session's loadable boundary; Pi owns the tree.
    pub fn load_history(&mut self, marker_id: &str) -> Vec<Effect> {
        if !self.connected
            || !self
                .state
                .transcript
                .iter()
                .any(|r| r.id == marker_id && r.history_available && !r.history_loading)
        {
            return vec![];
        }
        if !self.history_layers.is_empty() {
            if self.first_history_layer > 0 {
                self.state.transcript.drain(..self.history_prefix_len);
                self.first_history_layer -= 1;
                self.apply_history_prefix();
            }
            vec![]
        } else {
            self.set_history_loading(true);
            vec![self.send("get_entries", json!({}), false)]
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
    /// O(1) invalidation token for catalogue, scope, current model and effort choices.
    pub fn model_choices_revision(&self) -> u64 {
        self.model_choices_revision
    }
    /// Changes only when the authoritative command catalogue changes or is reset.
    pub fn commands_revision(&self) -> u64 {
        self.commands_revision
    }
    pub fn settings_presentation(&self) -> Option<SettingsPresentation> {
        if !self.connected
            || !self.state.ready
            || !self.scope_known
            || !self.model_state_known
            || !self.catalogue_known
            || !self.runtime_scope_settled
            || !self.efforts_known
            || self.setting_pending()
            || self.state.stopping
            || self.state.compacting
            || self.state.models.is_empty()
            || self.state.thinking_levels.is_empty()
            || !valid_model(&self.state.model)
            || !self.state.thinking_levels.contains(&self.state.effort)
        {
            return None;
        }
        Some(SettingsPresentation {
            models: self
                .state
                .selectable_models()
                .into_iter()
                .cloned()
                .collect(),
            model: self.state.model.clone(),
            efforts: self.state.thinking_levels.clone(),
            effort: self.state.effort.clone(),
        })
    }
    pub fn commands_presentation(&self) -> Option<Vec<Value>> {
        (self.connected && self.state.ready && !self.state.stopping && self.commands_loaded)
            .then(|| self.state.commands.clone())
    }
    fn reset_commands(&mut self) {
        self.commands_loaded = false;
        self.commands_revision += replace_changed(&mut self.state.commands, vec![]) as u64;
    }
    pub fn error_token(&self) -> Option<ErrorToken> {
        self.state.error.as_ref().map(|_| ErrorToken {
            runtime_id: self.runtime_id.clone(),
            generation: self.generation,
            epoch: self.epoch,
            revision: self.error_revision,
        })
    }
    /// Dismiss only the captured incident; uncertain writes and provisional work remain owned.
    pub fn acknowledge_error(&mut self, token: &ErrorToken) -> bool {
        if self.error_token().as_ref() != Some(token) {
            return false;
        }
        self.state.error = None;
        self.extension_error_owner = None;
        self.update_status();
        true
    }
    /// Host write admission is not Pi acceptance. Extension handlers can await UI before replying.
    pub fn request_write_accepted(&mut self, generation: u64, id: &str) -> bool {
        if !self.connected || generation != self.generation {
            return false;
        }
        let Some(p) = self.pending.get_mut(id) else {
            return false;
        };
        if p.epoch != self.epoch || !p.identity.binding_eq(&self.state.identity) {
            return false;
        }
        p.write_accepted = true;
        if p.command == "prompt" && p.extension_command {
            self.admission = None;
        }
        true
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn setting_pending(&self) -> bool {
        self.pending
            .values()
            .any(|p| matches!(p.command.as_str(), "set_model" | "set_thinking_level"))
    }
    pub fn set_draft(&mut self, text: impl Into<String>) {
        self.state.draft = text.into();
    }
    pub fn restore_recovered_draft(&mut self, index: usize, expected: &str) -> bool {
        if !self.state.draft.is_empty()
            || expected.is_empty()
            || self.state.recovered_drafts.get(index).map(String::as_str) != Some(expected)
        {
            return false;
        }
        self.state.draft = self.state.recovered_drafts.remove(index);
        true
    }
    pub fn take_recovered_drafts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.state.recovered_drafts)
    }
    /// Optional requested saved identity, set before bootstrap; never itself a durability proof.
    pub fn bind_source(&mut self, id: impl Into<String>, path: impl Into<String>) {
        self.clear_extension_error(None);
        self.state.queue_feedback = None;
        self.state.extension_dialogs.clear();
        self.reset_history();
        self.compaction_snapshot = None;
        self.saved_preview = saved_preview::Admission::default();
        self.message_occurrences.clear();
        let identity = Identity {
            id: id.into(),
            path: path.into(),
            name: String::new(),
        };
        if !self.state.identity.binding_eq(&identity) {
            self.state
                .transcript
                .retain(|row| row.origin == EntryOrigin::Pi);
        }
        self.state.identity = identity;
        self.epoch += 1;
    }
    /// Begin a host settings read. Only the latest read may complete.
    pub fn begin_model_scope_read(&mut self) -> ModelScopeToken {
        self.scope_known = false;
        self.scope_revision += 1;
        self.model_scope_token()
    }
    pub fn model_scope_token(&self) -> ModelScopeToken {
        ModelScopeToken {
            generation: self.generation,
            epoch: self.epoch,
            revision: self.scope_revision,
        }
    }
    /// `None` is a failed settings read; non-array values are ignored like Vue.
    pub fn apply_model_scope(&mut self, token: &ModelScopeToken, patterns: Option<Value>) -> bool {
        if !self.connected || *token != self.model_scope_token() {
            return false;
        }
        self.scope_known = patterns.as_ref().is_some_and(Value::is_array);
        let patterns = match patterns {
            None => Vec::new(),
            Some(Value::Array(patterns)) => patterns
                .iter()
                .filter_map(|p| p.as_str().map(str::to_owned))
                .collect(),
            Some(_) => return false,
        };
        self.model_choices_revision +=
            replace_changed(&mut self.state.model_scope, patterns) as u64;
        true
    }
    fn reset_model_scope(&mut self) {
        self.scope_known = false;
        self.model_state_known = false;
        self.catalogue_known = false;
        self.runtime_scope_settled = false;
        self.efforts_known = false;
        self.scope_revision += 1;
        let changed = replace_changed(&mut self.state.models, Vec::new())
            | replace_changed(&mut self.state.model_scope, Vec::new())
            | replace_changed(&mut self.state.scoped_models, None);
        self.model_choices_revision += changed as u64;
    }
    fn next_id(&mut self) -> String {
        self.sequence += 1;
        format!(
            "tau:{}:g{}:{}",
            self.runtime_id, self.generation, self.sequence
        )
    }
    fn send(&mut self, command: &str, mut payload: Value, barrier: bool) -> Effect {
        // Retained values remain visible locally, but cannot seed another session mid-discovery.
        match command {
            "set_model" | "set_thinking_level" => {
                self.model_state_known = false;
                self.presentation_state_floor = self.sequence;
            }
            "get_state" => self.model_state_known = false,
            "get_available_models" => self.catalogue_known = false,
            "get_scoped_models" => self.runtime_scope_settled = false,
            "get_available_thinking_levels" => self.efforts_known = false,
            _ => {}
        }
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
                completed_assistant: false,
                draft: None,
                optimistic: None,
                accepted: false,
                write_accepted: false,
                previous_name: None,
                purpose: ReadPurpose::Ordinary,
                extension_command: false,
                command_incarnation: self.reconciliation.command_incarnation,
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
        self.clear_extension_error(None);
        self.state.queue_feedback = None;
        self.recover_pending();
        self.finalize_partial();
        self.reset_history();
        self.compaction_snapshot = None;
        self.saved_preview
            .adopt_start(self.generation, generation, self.epoch);
        self.generation = generation;
        self.reset_reconciliation_generation();
        self.reset_commands();
        self.compaction_working = false;
        self.epoch += 1;
        self.reset_model_scope();
        self.pending.clear();
        self.latest.clear();
        self.restart_saved_preview_provenance();
        self.message_occurrences = if self.saved_preview.projects_wire_rows() {
            self.saved_preview.occupied.clone()
        } else {
            BTreeMap::new()
        };
        self.connected = true;
        self.state_serial = 0;
        self.admission = None;
        self.state.ready = false;
        self.state.working = false;
        self.state.stopping = false;
        self.state.compacting = false;
        self.state.durable = false;
        self.state.status = Status::Starting;
        self.state.retry = None;
        self.state.history_loading = true;
        self.state.extension_dialogs.clear();
        self.lose_queue();
        self.active_assistant = None;
        self.startup_deadline = Some(self.now.saturating_add(REQUEST_TIMEOUT_MS));
        self.stop_deadline = None;
        self.update_status();
        vec![
            self.send_read("get_state", ReadPurpose::Bootstrap),
            self.send("get_messages", json!({}), false),
            self.send("get_available_models", json!({}), false),
            self.send("get_scoped_models", json!({}), false),
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
                if self.state.compacting {
                    self.compaction_working = true;
                }
                self.confirm_prompt();
                self.state.working = true;
                self.revision += 1;
                effects.push(self.send("get_state", json!({}), false));
            }
            "message_start" => {
                let m = &v["message"];
                let fallback = self.next_id();
                let key = occurrence_key(m, &fallback, &mut self.message_occurrences);
                self.saved_preview.track(&key, m, &self.message_occurrences);
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
                self.finish_saved_preview_track();
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
                    self.saved_preview.track(&key, m, &self.message_occurrences);
                    self.merge_rows(project(m, &key), false);
                    self.finish_saved_preview_track();
                    if meaningful_completed_assistant(m) {
                        self.capture_materialisation();
                    }
                    self.active_assistant = None;
                    self.revision += 1;
                    if !self
                        .pending
                        .values()
                        .any(|p| p.barrier && p.epoch == self.epoch)
                    {
                        let barrier = self.send("get_state", json!({}), true);
                        if let Effect::Send(request) = &barrier {
                            if let Some(pending) =
                                self.pending.get_mut(string(&request["id"]).as_str())
                            {
                                pending.completed_assistant =
                                    !matches!(m["stopReason"].as_str(), Some("error" | "aborted"));
                            }
                        }
                        effects.push(barrier);
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
                self.state.retry = None;
                self.state.compacting = false;
                self.state.working = false;
                self.state.stopping = false;
                self.stop_deadline = None;
                self.finalize_partial();
                self.revision += 1;
                self.watch_replacement();
                self.start_verification();
                effects.push(self.send_read("get_state", ReadPurpose::Settlement));
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
            "thinking_level_changed" => {
                self.model_choices_revision +=
                    replace_changed(&mut self.state.effort, string(&v["level"])) as u64;
            }
            "extension_error" => {
                self.fail("A Pi extension failed. Review the extension setup and try again.")
            }
            "compaction_start" => {
                if !self.state.compacting {
                    self.compaction_working = self.state.working;
                }
                self.state.compacting = true;
                self.state.working = true;
                // A mode acknowledgment must not resume a queue gesture across compaction.
                if let Some((draft, _)) = self.queue_submission.take() {
                    self.restore(draft);
                }
            }
            "auto_retry_start" => {
                self.state.retry = Some(RetryState {
                    attempt: Some(v["attempt"].as_u64().unwrap_or(1).max(1)),
                    max_attempts: v["maxAttempts"].as_u64(),
                    reason: RetryReason::from_provider_error(
                        v["errorMessage"].as_str().unwrap_or_default(),
                    ),
                });
                self.revision += 1;
                if self.state.compacting {
                    self.compaction_working = true;
                }
                self.state.working = true;
            }
            "auto_retry_end" => {
                self.state.retry = None;
                self.revision += 1;
            }
            "summarization_retry_scheduled" => {
                if self.state.compacting {
                    self.compaction_working = true;
                }
                self.state.working = true;
            }
            "compaction_end" => {
                if self.state.compacting {
                    self.state.working = self.compaction_working;
                }
                self.state.compacting = false;
                if v["result"].is_object() {
                    self.reset_history();
                    self.state
                        .transcript
                        .retain(|r| !r.source.starts_with("local:"));
                    self.compaction_snapshot = Some(
                        self.state
                            .transcript
                            .iter()
                            .map(|r| (r.id.clone(), r.revision))
                            .collect(),
                    );
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
        if p.purpose == ReadPurpose::Command
            && (!self.reconciliation.command
                || p.command_incarnation != self.reconciliation.command_incarnation)
        {
            return vec![];
        }
        if p.epoch != self.epoch || !p.identity.binding_eq(&self.state.identity) {
            // A completed predecessor's FIFO barrier remains useful after B is adopted,
            // but must never apply A's state to B.
            if p.barrier
                && v["success"] == true
                && v["data"].is_object()
                && (p.completed_assistant || self.has_materialisation(&p.identity))
            {
                let mut effects = vec![];
                self.register(&p.identity, &mut effects);
                if p.identity.binding_eq(&self.state.identity) {
                    self.verified();
                }
                return effects;
            }
            if p.barrier {
                let mut effects = vec![];
                self.handoff_materialisation(&p.identity, &mut effects);
                return effects;
            }
            if p.extension_command && p.command == "prompt" && v["success"] == true {
                return self.start_command_reconciliation();
            }
            return vec![];
        }
        if !p.barrier
            && p.purpose == ReadPurpose::Ordinary
            && p.command != "prompt"
            && self.latest.get(&p.command) != Some(&id)
        {
            return vec![];
        }
        if v["success"] != true {
            // A correlated rejection proves the optional capability is unavailable; transport
            // failures and timeouts do not establish a successful host-only discovery round.
            if p.command == "get_scoped_models" {
                self.runtime_scope_settled = true;
            }
            let rename = p.command == "set_session_name";
            self.reject(p, false);
            self.update_status();
            return if rename {
                vec![self.send("get_state", json!({}), false)]
            } else {
                vec![]
            };
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
                if p.barrier && (p.completed_assistant || self.has_materialisation(&p.identity)) {
                    self.register(&p.identity, &mut effects);
                    self.verified();
                }
                if p.serial < self.state_serial {
                    return effects;
                }
                self.state_serial = p.serial;
                if changed {
                    let previous = self.state.identity.clone();
                    if !self
                        .pending
                        .values()
                        .any(|pending| pending.barrier && pending.identity.binding_eq(&previous))
                    {
                        self.handoff_materialisation(&previous, &mut effects);
                    }
                    self.replacement_reconciliation(
                        p.purpose == ReadPurpose::Bootstrap,
                        &previous,
                        &current,
                    );
                    self.clear_extension_error(None);
                    // Preserve captured durability barriers and command completion ownership.
                    let retained: BTreeMap<_, _> = self
                        .pending
                        .iter()
                        .filter(|(_, pending)| {
                            pending.barrier || pending.extension_command || pending.accepted
                        })
                        .map(|(id, pending)| {
                            let mut pending = pending.clone();
                            if pending.extension_command {
                                pending.accepted = true;
                            }
                            (id.clone(), pending)
                        })
                        .collect();
                    self.pending.retain(|_, pending| {
                        !pending.barrier && !pending.extension_command && !pending.accepted
                    });
                    self.recover_pending();
                    self.pending = retained;
                    self.latest.clear();
                    self.epoch += 1;
                    self.reset_history();
                    self.compaction_snapshot = None;
                    self.state.transcript.clear();
                    self.message_occurrences.clear();
                    self.saved_preview = saved_preview::Admission::default();
                    self.active_assistant = None;
                    self.state.queue_feedback = None;
                    self.state.extension_dialogs.clear();
                    self.lose_queue();
                    self.state.durable = false;
                    self.state.identity = current.clone();
                    if matches!(
                        p.purpose,
                        ReadPurpose::Settlement | ReadPurpose::Verification
                    ) {
                        self.start_verification();
                    }
                    self.state.retry = None;
                    self.reset_commands();
                    self.reset_model_scope();
                    effects.push(Effect::IdentityChanged { previous, current });
                    effects.push(self.send("get_available_models", json!({}), false));
                    effects.push(self.send("get_scoped_models", json!({}), false));
                    effects.push(self.send("get_commands", json!({}), false));
                } else {
                    if !self.rename_pending() {
                        self.state.identity.name = current.name;
                        effects.push(Effect::SessionNameChanged {
                            identity: self.state.identity.clone(),
                        });
                    }
                }
                let choices_changed = replace_changed(&mut self.state.model, d["model"].clone())
                    | replace_changed(&mut self.state.effort, string(&d["thinkingLevel"]));
                self.model_choices_revision += choices_changed as u64;
                // A durability read is not a run-lifecycle signal; a later event may already have started work.
                if !p.barrier && p.revision == self.revision {
                    self.state.compacting = d["isCompacting"] == true;
                    self.compaction_working = d["isStreaming"] == true;
                    self.state.working = d["isStreaming"] == true || self.state.compacting;
                }
                self.state.ready = true;
                if p.purpose == ReadPurpose::Bootstrap {
                    self.reconciliation.bootstrap_source = None;
                }
                if p.barrier && !changed && self.state.durable {
                    self.verified();
                }
                if changed
                    || (!p.barrier
                        && (p.purpose != ReadPurpose::Probe
                            || self.reconciliation.verifying
                            || self.reconciliation.command))
                {
                    self.state.history_loading = true;
                    let purpose = if self.reconciliation.command {
                        ReadPurpose::Command
                    } else if changed && self.reconciliation.verifying {
                        ReadPurpose::Settlement
                    } else {
                        p.purpose
                    };
                    effects.push(self.send_read("get_messages", purpose));
                }
                // Old reads still own reconciliation, but not post-mutation presentation.
                self.model_state_known = p.serial > self.presentation_state_floor;
                effects.push(self.send("get_available_thinking_levels", json!({}), false));
            }
            "get_messages" => {
                if p.revision != self.revision && self.compaction_snapshot.is_none() {
                    self.state.history_loading = false;
                    if matches!(
                        p.purpose,
                        ReadPurpose::Settlement | ReadPurpose::Verification
                    ) {
                        self.reconciliation_failed(&p);
                        return vec![];
                    }
                    if !self.state.working {
                        return vec![self.send_read("get_messages", p.purpose)];
                    }
                    return vec![];
                }
                if let Some(messages) = d["messages"].as_array() {
                    if p.serial < self.reconciliation.messages_serial {
                        return vec![];
                    }
                    let preview_projection = self.saved_preview.projects_wire_rows();
                    let matched_sources = self.saved_preview.reconcile(messages);
                    self.saved_preview.authoritative = true;
                    self.reconciliation.messages_serial = p.serial;
                    self.reconciled_messages(&p, messages, &mut effects);
                    let old = self.state.transcript
                        [self.history_prefix_len.min(self.state.transcript.len())..]
                        .to_vec();
                    self.state.transcript.clear();
                    let prior_by_source: HashMap<_, _> = old
                        .iter()
                        .map(|row| ((row.source.as_str(), row.kind.as_str()), row))
                        .collect();
                    let optimistic: HashMap<_, _> = old
                        .iter()
                        .filter(|row| row.source.starts_with("optimistic:"))
                        .map(|row| ((row.kind.as_str(), row.text.as_str()), row))
                        .collect();
                    let mut occurrences = BTreeMap::new();
                    let mut positions = HashMap::new();
                    for (i, m) in messages.iter().enumerate() {
                        let key = occurrence_key(m, &format!("history-{i}"), &mut occurrences);
                        let mut rows = project(m, &key);
                        for r in &mut rows {
                            let source = if preview_projection {
                                matched_sources[i].as_ref().map(|prior| {
                                    r.source
                                        .strip_prefix(&key)
                                        .map(|suffix| format!("{prior}{suffix}"))
                                        .unwrap_or_else(|| r.source.clone())
                                })
                            } else {
                                Some(r.source.clone())
                            };
                            if let Some(prior) = source
                                .as_deref()
                                .and_then(|source| prior_by_source.get(&(source, r.kind.as_str())))
                            {
                                r.id = prior.id.clone();
                                r.revision = prior.revision + 1;
                                r.text_reset_revision = r.revision;
                            } else if let Some(prior) =
                                optimistic.get(&(r.kind.as_str(), r.text.as_str()))
                            {
                                r.id = prior.id.clone();
                                r.revision = prior.revision + 1;
                                r.text_reset_revision = r.revision;
                            }
                        }
                        for mut row in rows {
                            let identity = (row.source.clone(), row.kind.clone());
                            if let Some(&index) = positions.get(&identity) {
                                let prior: &Entry = &self.state.transcript[index];
                                row.id = prior.id.clone();
                                if row.kind == "tool" {
                                    if row.result.is_empty() {
                                        row.result = prior.result.clone();
                                        row.is_error = prior.is_error;
                                    }
                                    if row.arguments.is_null() {
                                        row.arguments = prior.arguments.clone();
                                    }
                                }
                                row.revision = prior.revision + 1;
                                row.text_reset_revision = row.revision;
                                self.state.transcript[index] = row;
                            } else {
                                if row.id.is_empty() {
                                    row.id = self.next_id();
                                }
                                positions.insert(identity, self.state.transcript.len());
                                self.state.transcript.push(row);
                            }
                        }
                    }
                    self.message_occurrences = occurrences;
                    // A continuing run may advance while the compaction read is in flight.
                    let reconciling_compaction = self.compaction_snapshot.is_some();
                    if let Some(snapshot) = self.compaction_snapshot.take() {
                        let live_rows: Vec<_> = old
                            .iter()
                            .filter(|r| snapshot.get(&r.id) != Some(&r.revision))
                            .collect();
                        for live in &live_rows {
                            if matches!(live.origin, EntryOrigin::RuntimeNotice { .. }) {
                                continue;
                            }
                            let identity = (live.source.clone(), live.kind.clone());
                            if let Some(&index) = positions.get(&identity) {
                                self.state.transcript[index] = (*live).clone();
                            } else {
                                // Equal prose is not provenance: repeated messages are valid.
                                positions.insert(identity, self.state.transcript.len());
                                self.state.transcript.push((*live).clone());
                            }
                        }
                        // Old anchors count the pre-compaction tail. Place notices before
                        // their next live Pi row (which may already be in the snapshot),
                        // or at the new tail end, then retain these translated anchors.
                        let mut pi_count = 0;
                        let mut pi_positions = HashMap::new();
                        for row in &self.state.transcript {
                            if row.origin == EntryOrigin::Pi {
                                pi_positions
                                    .insert((row.source.as_str(), row.kind.as_str()), pi_count);
                                pi_count += 1;
                            }
                        }
                        let mut anchor = pi_count;
                        let mut notices = vec![];
                        for live in live_rows.into_iter().rev() {
                            if matches!(live.origin, EntryOrigin::RuntimeNotice { .. }) {
                                let mut notice = live.clone();
                                notice.anchor = Some(anchor);
                                notices.push(notice);
                            } else if let Some(&position) =
                                pi_positions.get(&(live.source.as_str(), live.kind.as_str()))
                            {
                                anchor = position;
                            }
                        }
                        notices.reverse();
                        self.state.transcript = crate::transcript::merge_runtime_notices(
                            std::mem::take(&mut self.state.transcript),
                            notices,
                        );
                    }
                    if !reconciling_compaction {
                        let mut ids: HashSet<_> = self
                            .state
                            .transcript
                            .iter()
                            .map(|row| row.id.clone())
                            .collect();
                        let mut notices = vec![];
                        for row in old {
                            if !ids.insert(row.id.clone()) {
                                continue;
                            }
                            if matches!(row.origin, EntryOrigin::RuntimeNotice { .. }) {
                                notices.push(row);
                            } else if row.interrupted {
                                self.state.transcript.push(row);
                            }
                        }
                        self.state.transcript = crate::transcript::merge_runtime_notices(
                            std::mem::take(&mut self.state.transcript),
                            notices,
                        );
                    }
                    self.apply_history_prefix();
                    self.state.history_loading = false;
                } else {
                    self.reject(p, false);
                }
            }
            "get_entries" => {
                self.set_history_loading(false);
                self.history_layers = history_layers(
                    d["entries"]
                        .as_array()
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    &string(&d["leafId"]),
                );
                if self.history_layers.is_empty() {
                    for row in &mut self.state.transcript {
                        if row.kind == "compaction" {
                            row.history_available = false;
                            row.touch();
                        }
                    }
                } else {
                    self.first_history_layer = self.history_layers.len() - 1;
                    self.apply_history_prefix();
                }
            }
            "get_available_models" => {
                self.catalogue_known = d["models"].is_array();
                let models = array(&d["models"])
                    .into_iter()
                    .filter(valid_model)
                    .collect();
                self.model_choices_revision +=
                    replace_changed(&mut self.state.models, models) as u64;
            }
            "get_scoped_models" => {
                self.runtime_scope_settled = resolved_scope(d).is_some();
                self.model_choices_revision +=
                    replace_changed(&mut self.state.scoped_models, resolved_scope(d)) as u64;
            }
            "get_commands" => {
                self.commands_loaded = true;
                self.commands_revision +=
                    replace_changed(&mut self.state.commands, array(&d["commands"])) as u64;
            }
            "get_available_thinking_levels" => {
                self.efforts_known = d["levels"].is_array();
                self.model_choices_revision += replace_changed(
                    &mut self.state.thinking_levels,
                    strings(&d["levels"]).unwrap_or_default(),
                ) as u64;
            }
            "prompt" => {
                if let Some(rid) = &p.optimistic {
                    if let Some(r) = self.state.transcript.iter_mut().find(|r| &r.id == rid) {
                        r.pending = false;
                        r.touch();
                    }
                }
                if d["disposition"] == "handled"
                    || (p.extension_command && d["disposition"].is_null())
                {
                    self.admission = None;
                    if !p.accepted {
                        self.state.working = false;
                    }
                    effects.extend(self.start_command_reconciliation());
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
            "set_model" => {
                self.presentation_state_floor = self.sequence;
                effects.push(self.send("get_state", json!({}), false));
                effects.extend(self.refresh_models());
            }
            "set_thinking_level" => {
                self.presentation_state_floor = self.sequence;
                effects.push(self.send("get_state", json!({}), false));
            }
            "set_session_name" => effects.push(self.send("get_state", json!({}), false)),
            "set_steering_mode" | "set_follow_up_mode" => effects.extend(self.continue_queue()),
            "clear_queue" => {
                self.state.queue_feedback = None;
                if p.payload["queueVersion"].as_u64() == Some(self.queue_version) {
                    self.state.queue = QueueSnapshot::default();
                }
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
        let captured = self
            .materialisation
            .remove(&(identity.id.clone(), identity.path.clone()));
        let generation = captured
            .as_ref()
            .map_or(self.generation, |(_, generation)| *generation);
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
                generation,
                id: identity.id.clone(),
                path: identity.path.clone(),
                name: identity.name.clone(),
            });
        }
    }
    pub fn rename_pending(&self) -> bool {
        self.pending
            .values()
            .any(|p| p.command == "set_session_name")
    }
    pub fn rename(&mut self, name: &str) -> Vec<Effect> {
        let name = normalize_session_name(name);
        if !self.state.ready
            || !self.connected
            || self.rename_pending()
            || name.is_empty()
            || name == self.state.identity.name
        {
            return vec![];
        }
        let previous = std::mem::replace(&mut self.state.identity.name, name.clone());
        self.state.error = None;
        let effect = self.send("set_session_name", json!({"name": name}), false);
        self.state_serial = self.sequence;
        if let Effect::Send(request) = &effect {
            if let Some(pending) = request["id"]
                .as_str()
                .and_then(|id| self.pending.get_mut(id))
            {
                pending.previous_name = Some(previous);
            }
        }
        vec![effect]
    }
    pub fn submit(&mut self, text: impl Into<String>) -> Vec<Effect> {
        self.submit_inner(text.into(), None)
    }
    fn submit_inner(&mut self, draft: String, queue: Option<QueueKind>) -> Vec<Effect> {
        if !self.state.ready
            || self.state.compacting
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
            p.extension_command = command;
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
        if self.state.compacting {
            return vec![];
        }
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
        if self.state.compacting {
            if let Some((draft, _)) = self.queue_submission.take() {
                self.restore(draft);
            }
            return vec![];
        }
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
        if !self.state.working || self.state.stopping || self.state.compacting {
            return vec![];
        }
        self.state.stopping = true;
        self.stop_deadline = Some(self.now.saturating_add(STOP_TIMEOUT_MS));
        self.update_status();
        vec![self.send("abort", json!({}), false)]
    }
    fn settings_allowed(&self) -> bool {
        self.state.ready
            && !self.state.working
            && !self.state.stopping
            && !self.state.compacting
            && !self.setting_pending()
    }
    pub fn set_model(&mut self, provider: &str, model_id: &str) -> Vec<Effect> {
        if !self.settings_allowed() {
            return vec![];
        }
        vec![self.send(
            "set_model",
            json!({"provider":provider,"modelId":model_id}),
            false,
        )]
    }
    pub fn set_thinking(&mut self, level: &str) -> Vec<Effect> {
        if !self.settings_allowed() {
            return vec![];
        }
        vec![self.send("set_thinking_level", json!({"level":level}), false)]
    }
    pub fn refresh(&mut self) -> Vec<Effect> {
        if self.generation == 0 || !self.connected {
            return vec![];
        }
        self.state.error = None;
        let mut effects = vec![self.send("get_state", json!({}), false)];
        effects.extend(self.refresh_models());
        effects
    }
    /// Refresh catalogue and optional runtime scope after settings changes.
    pub fn refresh_models(&mut self) -> Vec<Effect> {
        if self.generation == 0 || !self.connected {
            return vec![];
        }
        self.scope_known = false;
        self.scope_revision += 1;
        vec![
            self.send("get_available_models", json!({}), false),
            self.send("get_scoped_models", json!({}), false),
        ]
    }
    pub fn queue_snapshot(&self) -> &QueueSnapshot {
        &self.state.queue
    }
    pub fn submission_pending(&self) -> bool {
        self.queue_submission.is_some() || self.pending.values().any(|p| p.command == "prompt")
    }
    pub fn queue_busy(&self) -> bool {
        self.submission_pending() || self.pending.values().any(|p| p.command == "clear_queue")
    }
    pub fn queue_feedback(&self) -> Option<&str> {
        self.state.queue_feedback.as_deref()
    }
    pub fn dismiss_queue_feedback(&mut self) {
        self.state.queue_feedback = None;
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
        self.state.queue_feedback = None;
        let effect = self.send("clear_queue", json!({}), false);
        if let Effect::Send(v) = &effect {
            if let Some(p) = self.pending.get_mut(v["id"].as_str().unwrap_or_default()) {
                p.payload["queueVersion"] = json!(self.queue_version);
            }
        }
        vec![effect]
    }
    /// Whether the current generic incident belongs to this live extension request.
    pub fn extension_delivery_error_owned(&self, id: &str) -> bool {
        self.state.error.is_some()
            && self
                .extension_error_owner
                .as_ref()
                .is_some_and(|(epoch, request, revision)| {
                    *epoch == self.epoch
                        && request == id
                        && *revision == self.error_revision
                        && self.state.extension_dialogs.iter().any(|d| d.id == id)
                })
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
        self.clear_extension_error(Some(id));
        self.state.extension_dialogs[index].submitting = true;
        self.update_status();
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
            if self.state.error.is_none() || self.extension_error_owner.is_some() {
                self.fail("The extension response could not be sent. Try again.");
                self.extension_error_owner = Some((self.epoch, id.into(), self.error_revision));
            }
            self.update_status();
            return vec![];
        }
        self.clear_extension_error(Some(id));
        self.state.extension_dialogs.remove(index);
        self.update_status();
        self.start_command_reconciliation()
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
            "notify" if v["message"].is_string() => {
                let source = format!("local:notify:{id}");
                if !self.state.transcript.iter().any(|r|
                    matches!(&r.origin, EntryOrigin::RuntimeNotice { request_id } if request_id == &id)) {
                    let rid = self.next_id();
                    let mut entry = Entry::new(rid, "notice", string(&v["message"]), source);
                    entry.notice_type =
                        crate::transcript::NoticeType::from_notify(&v["notifyType"]);
                    entry.origin = EntryOrigin::RuntimeNotice { request_id: id };
                    if self.saved_preview.authoritative {
                        entry.anchor = Some(self.state.transcript.iter()
                            .skip(self.history_prefix_len)
                            .filter(|row| row.origin == EntryOrigin::Pi).count());
                    }
                    self.state.transcript.push(entry);
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
            if let Some(previous) = &p.previous_name {
                self.state.identity.name = previous.clone();
            }
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
        if matches!(p.command.as_str(), "set_model" | "set_thinking_level") {
            // Reads issued before confirmation cannot resolve a possibly applied mutation.
            self.model_state_known = false;
            self.presentation_state_floor = self.sequence;
        }
        if self.reconciliation_failed(&p) {
            return;
        }
        if (timeout || p.accepted)
            && matches!(
                p.command.as_str(),
                "prompt"
                    | "abort"
                    | "set_session_name"
                    | "set_model"
                    | "set_thinking_level"
                    | "clear_queue"
            )
        {
            self.reconciliation.uncertain_write = true;
            self.reconciliation.uncertain_since = self.sequence;
        }
        // Optional capability: old Pi versions reject this correlated probe.
        if p.command == "get_scoped_models" {
            self.model_choices_revision +=
                replace_changed(&mut self.state.scoped_models, None) as u64;
            return;
        }
        if let Some(previous) = &p.previous_name {
            self.state.identity.name = previous.clone();
        }
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
        if p.command == "get_entries" {
            self.set_history_loading(false);
        }
        if p.command.starts_with("set_") {
            if let Some((draft, _)) = self.queue_submission.take() {
                self.restore(draft);
            }
        }
        if p.command == "clear_queue" {
            self.state.queue_feedback = Some(
                if timeout {
                    "Queue clear was not confirmed. Check the queue before retrying."
                } else {
                    "Could not clear the queue. Check the Pi version and retry."
                }
                .into(),
            );
            return;
        }
        self.fail(match p.command.as_str() {
            "prompt" if timeout => {
                "Message confirmation was lost. Check the transcript before resending."
            }
            "prompt" => "The message was not accepted. Review the draft and try again.",
            "abort" => "Stop was not confirmed. Pi may still be working. Try Stop again.",
            "get_entries" => {
                "Earlier messages could not be loaded. Try the compacted divider again."
            }
            "set_session_name" => {
                "The session could not be renamed. Choose another name and try again."
            }
            "set_model" | "set_thinking_level" => {
                "The setting was not confirmed. Refresh the session and try again."
            }
            _ => "The Pi request failed or timed out. Refresh the session and try again.",
        });
    }
    pub fn request_failed(&mut self, id: &str) -> Vec<Effect> {
        let mut effects = vec![];
        if let Some(p) = self.pending.remove(id) {
            if p.barrier && !p.identity.binding_eq(&self.state.identity) {
                self.handoff_materialisation(&p.identity, &mut effects);
            }
            if p.epoch == self.epoch
                && p.identity.binding_eq(&self.state.identity)
                && (p.barrier
                    || p.purpose != ReadPurpose::Ordinary
                    || p.command == "prompt"
                    || self
                        .latest
                        .get(&p.command)
                        .is_some_and(|latest| latest == id))
            {
                let rename = p.command == "set_session_name";
                self.reject(p, false);
                if rename {
                    effects.push(self.send("get_state", json!({}), false));
                }
            }
        }
        self.update_status();
        effects
    }
    /// Acknowledges physical backend stop, not admission to stop or remote exit proof.
    pub fn backend_stopped(&mut self, generation: u64) -> Vec<Effect> {
        self.transport_failed(generation)
    }
    pub fn transport_failed(&mut self, generation: u64) -> Vec<Effect> {
        if generation != self.generation {
            return vec![];
        }
        if self.state.working
            || self.reconciliation.command
            || self.pending.values().any(|p| {
                p.accepted || (p.command == "prompt" && p.extension_command && p.write_accepted)
            })
            || self
                .state
                .extension_dialogs
                .iter()
                .any(|dialog| dialog.submitting)
        {
            self.reconciliation.uncertain_write = true;
            self.reconciliation.uncertain_since = self.sequence;
        }
        self.connected = false;
        self.state.retry = None;
        self.admission = None;
        self.set_history_loading(false);
        self.compaction_snapshot = None;
        self.recover_pending();
        self.pending.clear();
        self.latest.clear();
        self.finalize_partial();
        self.lose_queue();
        self.clear_extension_error(None);
        self.state.queue_feedback = None;
        self.state.extension_dialogs.clear();
        self.state.ready = false;
        self.state.working = false;
        self.state.stopping = false;
        self.state.compacting = false;
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
        let mut refresh_name = false;
        let mut effects = vec![];
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| {
                p.deadline <= self.now
                    && !(p.command == "prompt"
                        && (p.accepted || (p.extension_command && p.write_accepted)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            if let Some(p) = self.pending.remove(&id) {
                if p.barrier && !p.identity.binding_eq(&self.state.identity) {
                    self.handoff_materialisation(&p.identity, &mut effects);
                }
                if p.epoch == self.epoch
                    && p.identity.binding_eq(&self.state.identity)
                    && (p.barrier
                        || p.purpose != ReadPurpose::Ordinary
                        || p.command == "prompt"
                        || self.latest.get(&p.command) == Some(&id))
                {
                    refresh_name |= p.command == "set_session_name";
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
        // Accepted writes (including cancellation) belong to the transport until Written.
        let expired_dialogs: Vec<_> = self
            .state
            .extension_dialogs
            .iter()
            .filter(|d| !d.submitting && d.deadline_ms.is_some_and(|n| n <= self.now))
            .map(|d| d.id.clone())
            .collect();
        for id in expired_dialogs {
            self.clear_extension_error(Some(&id));
            self.state.extension_dialogs.retain(|d| d.id != id);
        }
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
        effects.extend(self.tick_reconciliation());
        if refresh_name && self.connected {
            effects.push(self.send("get_state", json!({}), false));
        }
        effects
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
    fn clear_extension_error(&mut self, id: Option<&str>) {
        if let Some((epoch, request, revision)) = &self.extension_error_owner {
            if *epoch == self.epoch && id.is_none_or(|id| id == request) {
                if *revision == self.error_revision {
                    self.state.error = None;
                }
                self.extension_error_owner = None;
            }
        }
    }
    fn fail(&mut self, message: &str) {
        self.error_revision += 1;
        self.extension_error_owner = None;
        self.state.error = Some(message.into());
    }
    fn update_status(&mut self) {
        if !self.state.working {
            self.state.retry = None;
            self.compaction_working = false;
            self.state.compacting = false;
        }
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
#[path = "session_feedback_ownership_tests.rs"]
mod feedback_ownership_tests;

#[cfg(test)]
#[path = "session_workflow_tests.rs"]
mod workflow_tests;

#[cfg(test)]
#[path = "session_command_tests.rs"]
mod command_tests;

#[cfg(test)]
#[path = "session_history_tests.rs"]
mod history_tests;

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn compaction_presentation_preserves_stream_lifecycle_and_generation_fences() {
        let mut controller = Controller::new("test");
        controller.begin_generation(1);
        for streaming in [false, true] {
            if streaming {
                controller.handle(json!({"type":"agent_start"}));
            }
            controller.handle(json!({"type":"compaction_start"}));
            assert!(controller.state.compacting);
            assert!(controller.state.working);
            controller.handle(json!({"type":"compaction_end"}));
            assert!(!controller.state.compacting);
            assert_eq!(controller.state.working, streaming);
        }
        controller.handle(json!({"type":"compaction_start"}));
        controller.begin_generation(2);
        controller.ingest("test", 1, json!({"type":"compaction_start"}));
        assert!(!controller.state.compacting);
        assert!(!controller.state.working);
        controller.handle(json!({"type":"compaction_start"}));
        controller.transport_failed(2);
        assert!(!controller.state.compacting);
        assert!(!controller.state.working);
    }

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
