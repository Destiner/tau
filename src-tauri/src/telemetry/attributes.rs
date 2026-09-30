//! Reviewed resource and signal attribute allowlist; keep validation centralized.
use opentelemetry::Value;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use super::privacy::DEFAULT_MAX_ATTRIBUTE_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeKind {
    Str,
    I64,
}

#[derive(Debug, Clone, Copy)]
pub struct AttributeSpec {
    pub key: &'static str,
    pub kind: AttributeKind,
    /// String length bound; non-string specs have no length bound.
    pub max_len: Option<usize>,
    /// Identifiers are excluded from metric dimensions to bound cardinality.
    pub metric_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeError {
    UnknownAttribute,
    UnknownValue,
    WrongType,
    OutOfRange,
    TooLong,
}

pub const RESOURCE_ATTRIBUTES: &[AttributeSpec] = &[
    AttributeSpec {
        key: SERVICE_NAME,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: SERVICE_VERSION,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: SERVICE_INSTANCE_ID,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: DEPLOYMENT_ENVIRONMENT_NAME,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: OS_TYPE,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: HOST_ARCH,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
];

/// Context identifiers belong on spans/logs, never metric dimensions.
pub const CONTEXT_ATTRIBUTES: &[AttributeSpec] = &[
    AttributeSpec {
        key: "tau.project.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.session.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.controller.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.runtime.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "pi.generation",
        kind: AttributeKind::I64,
        max_len: None,
        metric_safe: false,
    },
];

pub struct RecordFamily {
    pub name: &'static str,
    pub attributes: &'static [AttributeSpec],
}

pub const UI_ACTION_NAMES: &[&str] = &[
    "session.select",
    "session.new",
    "message.send",
    "session.stop",
    "session.rename",
    "model.select",
    "effort.select",
    "extension.dialog.submit",
    "extension.dialog.cancel",
];

pub const UI_ACTION: RecordFamily = RecordFamily {
    name: "ui.action",
    attributes: &[AttributeSpec {
        key: "tau.action.name",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    }],
};

/// The invoke wrapper excludes `send_pi` (RPC spans), telemetry ingest (avoid recursion), and
/// admin-mode reads (before gating).
pub const TAURI_INVOKE_COMMANDS: &[&str] = &[
    "load_workspace",
    "set_admin_mode",
    "get_dismissed_update_version",
    "set_dismissed_update_version",
    "submit_issue_report",
    "import_project",
    "import_remote_project",
    "remove_project",
    "set_active_project",
    "set_project_collapsed",
    "reorder_projects",
    "set_active_session",
    "archive_session",
    "register_session",
    "probe_remote_project",
    "list_remote_directories",
    "prepare_file_preview",
    "release_file_preview",
    "read_model_scope",
    "read_remote_model_scope",
    "request_quit",
    "resolve_quit_request",
    "update_snapshot",
    "check_for_update",
    "download_update",
    "request_update_restart",
    "install_update",
    "restart_after_update",
    "start_pi",
    "start_pi_remote",
    "stop_pi",
    "read_pi_frontend_revision",
    "claim_pi_frontend",
];

pub const TAURI_INVOKE_OUTCOMES: &[&str] = &["success", "error"];

pub const TAURI_INVOKE: RecordFamily = RecordFamily {
    name: "tauri.invoke",
    attributes: &[
        AttributeSpec {
            key: "tau.invoke.command",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.invoke.outcome",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

pub const PI_RPC_METHODS: &[&str] = &[
    "get_state",
    "get_available_models",
    "get_commands",
    "get_available_thinking_levels",
    "get_messages",
    "get_entries",
    "set_model",
    "set_thinking_level",
    "set_session_name",
    "prompt",
    "abort",
    "extension_ui_response",
];

pub const PI_RPC_OUTCOMES: &[&str] = &[
    "success",
    "error",
    "timeout",
    "abandoned_process_exit",
    "abandoned_generation_change",
    "abandoned_stop",
    "abandoned_replacement",
    "abandoned_duplicate_request",
];

pub const PI_RPC: RecordFamily = RecordFamily {
    name: "pi.rpc",
    attributes: &[
        AttributeSpec {
            key: "pi.rpc.method",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "pi.rpc.request_id",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: false,
        },
        AttributeSpec {
            key: "pi.rpc.outcome",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

pub const PI_RPC_ANOMALY_KINDS: &[&str] = &["unmatched_or_duplicate"];

pub const PI_RPC_ANOMALY: RecordFamily = RecordFamily {
    name: "pi.rpc.anomaly",
    attributes: &[
        AttributeSpec {
            key: "pi.rpc.anomaly.kind",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "pi.rpc.request_id",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: false,
        },
    ],
};

/// Only bounded streaming totals, never per-token or per-delta records.
pub const PI_STREAM: RecordFamily = RecordFamily {
    name: "pi.stream",
    attributes: &[
        AttributeSpec {
            key: "pi.stream.delta_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "pi.stream.character_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

pub const CONTROLLER_LIFECYCLE_STATES: &[&str] = &[
    "idle",
    "connecting",
    "starting",
    "stopping",
    "syncing",
    "working",
    "ready",
];

/// Categorical values for `tau.controller.transition.cause`: every named
/// mutation boundary the frontend's `setControllerLifecycle` is called
/// from.
pub const CONTROLLER_LIFECYCLE_CAUSES: &[&str] = &[
    "controller_start",
    "controller_start_failed",
    "phantom_prompt_start",
    "phantom_prompt_resume",
    "process_exited",
    "agent_start",
    "agent_settled",
    "prompt_response",
    "get_state_failed",
    "get_messages_failed",
    "prompt_failed",
    "abort_failed",
    "get_state_response",
    "get_messages_response",
    "pending_prompt_dispatch",
    "pending_prompt_failed",
    "abort_probe_failed",
    "pending_prompt_cancelled",
    "process_stopped",
    "bridge_event_failed",
    "workspace_load_failed",
    "message_send",
    "message_send_failed",
    "stop_requested",
    "stop_failed",
];

pub const CONTROLLER_LIFECYCLE: RecordFamily = RecordFamily {
    name: "controller.lifecycle",
    attributes: &[
        AttributeSpec {
            key: "tau.controller.state.before",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.controller.state.after",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.controller.transition.cause",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

/// Categorical values for `tau.process.stop_reason`: why Tau stopped a
/// runtime's process (never why it exited on its own; see `exit_code`).
pub const PI_PROCESS_STOP_REASONS: &[&str] = &["explicit_stop", "replaced", "ownership_replaced"];

pub const PI_PROCESS_RESOLUTIONS: &[&str] = &["found", "not_found"];
pub const PI_PROCESS_EXIT_OUTCOMES: &[&str] = &["clean", "unexpected"];

pub const PI_OWNERSHIP_KINDS: &[&str] = &["initial", "replacement"];
pub const PI_OWNERSHIP_OUTCOMES: &[&str] = &["success", "cleanup_failed", "rejected"];

pub const PI_OWNERSHIP: RecordFamily = RecordFamily {
    name: "pi.ownership",
    attributes: &[
        AttributeSpec {
            key: "tau.ownership.kind",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.ownership.outcome",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.ownership.stale_process_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

pub const PI_PROCESS_LIFECYCLE: RecordFamily = RecordFamily {
    name: "pi.process.lifecycle",
    attributes: &[
        AttributeSpec {
            key: "tau.process.resolution",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.process.stop_reason",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.process.exit_outcome",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.process.exit_code",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

/// `app.started`/`app.exited` lifecycle logs. They carry only the resource
/// and, where applicable, context attributes, so this family adds none.
pub const APP_LIFECYCLE: RecordFamily = RecordFamily {
    name: "app.lifecycle",
    attributes: &[],
};

/// Categorical values for `tau.reader.drop_reason`: why `pi.rs` discarded a
/// line from Pi's stdout without forwarding it.
pub const PI_READER_DROP_REASONS: &[&str] = &["oversized", "invalid_utf8", "malformed"];

/// Bounded reader I/O error categories, never OS error messages.
pub const PI_READER_ERROR_KINDS: &[&str] =
    &["broken_pipe", "interrupted", "unexpected_eof", "other"];

/// Categorical values for `tau.event.kind`: the `pi-event` kinds `pi.rs` can
/// fail to emit to the frontend.
pub const PI_EVENT_KINDS: &[&str] = &["started", "rpc", "stderr", "error", "exited"];

/// Native reader/forwarding diagnostics, distinct from process lifecycle.
pub const PI_READER: RecordFamily = RecordFamily {
    name: "pi.reader",
    attributes: &[
        AttributeSpec {
            key: "tau.reader.drop_reason",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.reader.error_kind",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.event.kind",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

/// Frontend reports only its queue drops; native failed writes are recorded separately.
pub const TELEMETRY_HEALTH: RecordFamily = RecordFamily {
    name: "telemetry.health",
    attributes: &[
        AttributeSpec {
            key: "tau.telemetry.dropped_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.telemetry.failed_write_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

/// A Rust panic. Carries only a sanitized `basename:line:column` source
/// location — never the panic message, which can contain arbitrary content.
pub const RUST_PANIC: RecordFamily = RecordFamily {
    name: "rust.panic",
    attributes: &[AttributeSpec {
        key: "tau.error.location",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    }],
};

/// Categorical values for `tau.error.source`: which frontend capture point
/// produced a `frontend.error` record.
pub const FRONTEND_ERROR_SOURCES: &[&str] = &[
    "window_error",
    "unhandled_rejection",
    "vue_error",
    "console_error",
];

/// Categorical values for `tau.error.kind`: a thrown/rejected value's
/// constructor name, bounded to JavaScript's built-in error types plus
/// `other`/`none`. Never the error's own message.
pub const FRONTEND_ERROR_KINDS: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "EvalError",
    "URIError",
    "other",
    "none",
];

/// Frontend failures carry bounded kind/source and sanitized location, never message or thrown
/// object.
pub const FRONTEND_ERROR: RecordFamily = RecordFamily {
    name: "frontend.error",
    attributes: &[
        AttributeSpec {
            key: "tau.error.source",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.error.kind",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.error.location",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: false,
        },
    ],
};

pub const OPERATION_CHECKPOINT_FAMILIES: &[&str] = &["ui.action", "pi.rpc"];

pub const OPERATION_CHECKPOINT_NAMES: &[&str] = &[
    "session.select",
    "session.new",
    "message.send",
    "session.stop",
    "session.rename",
    "model.select",
    "effort.select",
    "extension.dialog.submit",
    "extension.dialog.cancel",
    "get_state",
    "get_available_models",
    "get_commands",
    "get_available_thinking_levels",
    "get_messages",
    "get_entries",
    "set_model",
    "set_thinking_level",
    "set_session_name",
    "prompt",
    "abort",
    "extension_ui_response",
];

/// Link start/checkpoint logs to active spans so unfinished operations remain visible.
pub const OPERATION_CHECKPOINT: RecordFamily = RecordFamily {
    name: "operation.checkpoint",
    attributes: &[
        AttributeSpec {
            key: "tau.operation.family",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.operation.name",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "pi.rpc.request_id",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: false,
        },
    ],
};

pub const ACTION_MILESTONE_KINDS: &[&str] = &[
    "ready",
    "persisted",
    "persistence_failed",
    "readable_memory",
    "readable_saved",
    "readable_rpc",
    "hydrated",
    "paint_opportunity",
    "paint_unavailable",
];

pub const ACTION_MILESTONE: RecordFamily = RecordFamily {
    name: "action.milestone",
    attributes: &[
        AttributeSpec {
            key: "tau.action.name",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.action.milestone",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.action.elapsed_ms",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: false,
        },
    ],
};

pub const HEARTBEAT_VISIBILITY_VALUES: &[&str] = &["visible", "hidden"];

pub const BOOLEAN_STRING_VALUES: &[&str] = &["true", "false"];

pub const FRONTEND_HEARTBEAT: RecordFamily = RecordFamily {
    name: "frontend.heartbeat",
    attributes: &[
        AttributeSpec {
            key: "tau.heartbeat.visibility",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.focused",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.pending_rpc_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.controller_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.active_controller_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.runtime_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.queue_length",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

/// Categorical values for `tau.state.draft_bucket`: a length bucket only,
/// never the draft text itself.
pub const DRAFT_LENGTH_BUCKETS: &[&str] = &["empty", "short", "medium", "long"];

/// Workspace counts and draft length bucket only; never text or paths.
pub const FRONTEND_STATE_SUMMARY: RecordFamily = RecordFamily {
    name: "frontend.state_summary",
    attributes: &[
        AttributeSpec {
            key: "tau.state.controller_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.runtime_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.pending_rpc_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.notification_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.dialog_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.transcript.user_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.transcript.assistant_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.transcript.tool_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.transcript.thinking_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.transcript.error_count",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.draft_bucket",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.state.oldest_pending_rpc_age_ms",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

/// Frontend-only lag/long-task durations need raw metric IPC; other metrics derive from spans
/// or logs.
pub const FRONTEND_EVENT_LOOP_LAG: RecordFamily = RecordFamily {
    name: "frontend.event_loop_lag",
    attributes: &[
        AttributeSpec {
            key: "tau.heartbeat.visibility",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.heartbeat.focused",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

/// A single `PerformanceObserver` `longtask` entry's duration. No
/// attributes: a long task's own attribution is not part of the reviewed
/// catalog and is never read.
pub const FRONTEND_LONG_TASK: RecordFamily = RecordFamily {
    name: "frontend.long_task",
    attributes: &[],
};

pub const FAMILIES: &[&RecordFamily] = &[
    &UI_ACTION,
    &TAURI_INVOKE,
    &PI_RPC,
    &PI_RPC_ANOMALY,
    &PI_STREAM,
    &CONTROLLER_LIFECYCLE,
    &PI_OWNERSHIP,
    &PI_PROCESS_LIFECYCLE,
    &APP_LIFECYCLE,
    &PI_READER,
    &TELEMETRY_HEALTH,
    &RUST_PANIC,
    &FRONTEND_ERROR,
    &OPERATION_CHECKPOINT,
    &ACTION_MILESTONE,
    &FRONTEND_HEARTBEAT,
    &FRONTEND_STATE_SUMMARY,
    &FRONTEND_EVENT_LOOP_LAG,
    &FRONTEND_LONG_TASK,
];

pub fn find_family(name: &str) -> Option<&'static RecordFamily> {
    FAMILIES.iter().copied().find(|family| family.name == name)
}

pub fn allowed_attribute(family: &str, key: &str) -> Option<&'static AttributeSpec> {
    let family = find_family(family)?;
    family
        .attributes
        .iter()
        .chain(CONTEXT_ATTRIBUTES)
        .find(|spec| spec.key == key)
}

/// Whether `key` is safe to use as a metric dimension. Unknown keys are not
/// safe: only cataloged, explicitly-reviewed attributes may reach a metric.
/// checks metric dimensions against this before recording them.
#[allow(dead_code)]
pub fn is_metric_safe(key: &str) -> bool {
    RESOURCE_ATTRIBUTES
        .iter()
        .chain(CONTEXT_ATTRIBUTES)
        .chain(FAMILIES.iter().flat_map(|family| family.attributes))
        .find(|spec| spec.key == key)
        .is_some_and(|spec| spec.metric_safe)
}

/// The reviewed value set for a categorical string attribute, if `key` is
/// one. Centralizing the key-to-enum mapping here means a new categorical
/// attribute only needs an entry here, not a bespoke branch in `validate`.
fn categorical_values(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "tau.action.name" => Some(UI_ACTION_NAMES),
        "tau.action.milestone" => Some(ACTION_MILESTONE_KINDS),
        "tau.invoke.command" => Some(TAURI_INVOKE_COMMANDS),
        "tau.invoke.outcome" => Some(TAURI_INVOKE_OUTCOMES),
        "pi.rpc.method" => Some(PI_RPC_METHODS),
        "pi.rpc.outcome" => Some(PI_RPC_OUTCOMES),
        "pi.rpc.anomaly.kind" => Some(PI_RPC_ANOMALY_KINDS),
        "tau.ownership.kind" => Some(PI_OWNERSHIP_KINDS),
        "tau.ownership.outcome" => Some(PI_OWNERSHIP_OUTCOMES),
        "tau.process.stop_reason" => Some(PI_PROCESS_STOP_REASONS),
        "tau.process.resolution" => Some(PI_PROCESS_RESOLUTIONS),
        "tau.process.exit_outcome" => Some(PI_PROCESS_EXIT_OUTCOMES),
        "tau.reader.drop_reason" => Some(PI_READER_DROP_REASONS),
        "tau.reader.error_kind" => Some(PI_READER_ERROR_KINDS),
        "tau.event.kind" => Some(PI_EVENT_KINDS),
        "tau.error.source" => Some(FRONTEND_ERROR_SOURCES),
        "tau.error.kind" => Some(FRONTEND_ERROR_KINDS),
        "tau.controller.state.before" | "tau.controller.state.after" => {
            Some(CONTROLLER_LIFECYCLE_STATES)
        }
        "tau.controller.transition.cause" => Some(CONTROLLER_LIFECYCLE_CAUSES),
        "tau.operation.family" => Some(OPERATION_CHECKPOINT_FAMILIES),
        "tau.operation.name" => Some(OPERATION_CHECKPOINT_NAMES),
        "tau.heartbeat.visibility" => Some(HEARTBEAT_VISIBILITY_VALUES),
        "tau.heartbeat.focused" => Some(BOOLEAN_STRING_VALUES),
        "tau.state.draft_bucket" => Some(DRAFT_LENGTH_BUCKETS),
        _ => None,
    }
}

/// Validate family, type, length, and categorical membership to prevent arbitrary telemetry
/// values.
const MAX_COUNT_ATTRIBUTE: i64 = 1_000_000_000;
const MAX_AGE_ATTRIBUTE_MS: i64 = 24 * 60 * 60 * 1000;

fn numeric_range(key: &str) -> Option<(i64, i64)> {
    if key.ends_with("_count") || key == "tau.heartbeat.queue_length" {
        return Some((0, MAX_COUNT_ATTRIBUTE));
    }
    if key == "pi.generation" {
        return Some((0, MAX_COUNT_ATTRIBUTE));
    }
    if key == "tau.action.elapsed_ms" || key == "tau.state.oldest_pending_rpc_age_ms" {
        return Some((0, MAX_AGE_ATTRIBUTE_MS));
    }
    None
}

pub fn validate(family: &str, key: &str, value: &Value) -> Result<(), AttributeError> {
    let spec = allowed_attribute(family, key).ok_or(AttributeError::UnknownAttribute)?;
    match (spec.kind, value) {
        (AttributeKind::Str, Value::String(string)) => {
            let max_len = spec.max_len.expect("string specs declare a max length");
            if string.as_str().len() > max_len {
                return Err(AttributeError::TooLong);
            }
            if let Some(values) = categorical_values(key) {
                if !values.contains(&string.as_str()) {
                    return Err(AttributeError::UnknownValue);
                }
            }
        }
        (AttributeKind::I64, Value::I64(value)) => {
            if let Some((minimum, maximum)) = numeric_range(key) {
                if *value < minimum || *value > maximum {
                    return Err(AttributeError::OutOfRange);
                }
            }
        }
        _ => return Err(AttributeError::WrongType),
    }
    Ok(())
}
