//! The `tau.*`/`pi.*` and OpenTelemetry resource attribute catalog. Every
//! record family later stages emit must draw its attributes from here, so
//! the allowlist lives in one authoritative place instead of being repeated
//! (and drifting) at each call site.
use opentelemetry::Value;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use super::privacy::DEFAULT_MAX_ATTRIBUTE_LEN;

/// The OpenTelemetry value shapes this catalog allows. Kept narrower than
/// `opentelemetry::Value` on purpose: nothing in the initial catalog needs a
/// float or an array, and adding one should be a deliberate catalog change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeKind {
    Str,
    I64,
}

#[derive(Debug, Clone, Copy)]
pub struct AttributeSpec {
    pub key: &'static str,
    pub kind: AttributeKind,
    /// Maximum encoded length for string values. Always `Some` for
    /// `AttributeKind::Str`; always `None` otherwise, which
    /// `catalog_specs_are_well_formed` below enforces.
    pub max_len: Option<usize>,
    /// Whether this key may appear on a metric's dimensions. `false` marks
    /// session, project, controller, runtime, request, and trace
    /// identifiers, which the design principles forbid on metrics because
    /// they would create unbounded cardinality.
    pub metric_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeError {
    /// The key is not part of this family's allowlist (or the shared context
    /// attributes), so it cannot be recorded at all.
    UnknownAttribute,
    /// A categorical string is not one of the reviewed safe values.
    UnknownValue,
    /// The value's runtime type does not match the spec's declared kind.
    WrongType,
    /// A numeric value is outside its reviewed range.
    OutOfRange,
    /// A string value exceeds the spec's `max_len`.
    TooLong,
}

/// Resource attributes attached once per app run (Stage 1 generates the
/// actual `Resource`). Not family-scoped: every record family carries these.
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

/// Tau-specific context attributes. Attached to spans and logs when
/// meaningful; never to metric dimensions (all `metric_safe: false`), per the
/// design principles' cardinality rule.
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

/// One named event/span family and the attributes it may carry, beyond the
/// shared context attributes above.
pub struct RecordFamily {
    pub name: &'static str,
    pub attributes: &'static [AttributeSpec],
}

/// Categorical values for `ui.action`'s `tau.action.name`: the semantic user
/// actions Stage 3 instruments in `useTau.ts`.
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

/// Every ordinary Tauri command routed through the shared invoke wrapper.
/// `send_pi` (covered by `pi.rpc` spans), `ingest_telemetry` (which must
/// never trace itself), and `read_admin_mode` (which decides whether
/// telemetry may record at all, so its own span could only be dropped) are
/// deliberately absent.
pub const TAURI_INVOKE_COMMANDS: &[&str] = &[
    "load_workspace",
    "set_admin_mode",
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

/// Categorical values for `pi.rpc.method`: every request `type` Tau sends
/// through `send_pi` (see `src/lib/pi/runtime.ts`).
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

/// Categorical values for `pi.rpc.outcome`: how a Pi RPC span ended.
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

/// Per-Pi-run aggregate streaming counts (`src/lib/pi/runtime.ts`). Never one
/// record per delta or token: this family carries only bounded totals.
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

/// Categorical values for `tau.controller.state.before`/`.after`: the
/// coarse composite lifecycle state the frontend's
/// `classifyControllerLifecycle` derives from a controller's boolean flags.
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

/// Categorical values for `tau.process.resolution`: whether Tau found a
/// local `pi` executable to spawn.
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

/// Categorical values for `tau.reader.error_kind`: a bounded subset of
/// `std::io::ErrorKind` variants a Pi stdout/stderr reader can observe.
/// Never the OS's own error message, which can be arbitrary.
pub const PI_READER_ERROR_KINDS: &[&str] =
    &["broken_pipe", "interrupted", "unexpected_eof", "other"];

/// Categorical values for `tau.event.kind`: the `pi-event` kinds `pi.rs` can
/// fail to emit to the frontend.
pub const PI_EVENT_KINDS: &[&str] = &["started", "rpc", "stderr", "error", "exited"];

/// Reader failures, malformed/oversized lines, and failed `pi-event`
/// emission — all native-only diagnostics about `pi.rs`'s own reading and
/// event-forwarding machinery, distinct from `pi.process.lifecycle`'s
/// resolve/start/stop/exit events.
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

/// Telemetry-pipeline health: the frontend's bounded-queue drop count and
/// the native store's writer-failure count. The frontend only ever reports
/// its own counter; the native counter is recorded natively (see
/// `Telemetry::record_writer_health`) and never accepted from the frontend
/// (`ingest.rs`'s `required_attributes` only requires the dropped-count key).
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

/// `window.error`, `unhandledrejection`, Vue errors, and sanitized
/// `console.error` calls captured from the frontend. Never the thrown
/// value's message or a serialized object — only its bounded category and a
/// sanitized source location (`tau.error.location`, not metric-safe: many
/// distinct call sites would make it high-cardinality as a dimension).
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

/// Categorical values for `tau.operation.family`: which real span family a
/// linked checkpoint log stands in for.
pub const OPERATION_CHECKPOINT_FAMILIES: &[&str] = &["ui.action", "pi.rpc"];

/// Categorical values for `tau.operation.name`: the union of every action
/// and RPC method name a checkpoint can name.
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

/// A linked start/checkpoint log for an operation that might never finish:
/// recorded the moment its span starts, carrying that span's own
/// `traceId`/`spanId`, so the operation stays visible in the persisted
/// timeline even if the span itself never ends.
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

/// Categorical values shared by `tau.heartbeat.visibility`.
pub const HEARTBEAT_VISIBILITY_VALUES: &[&str] = &["visible", "hidden"];

/// Categorical values shared by every boolean-as-string attribute in the
/// catalog (`tau.heartbeat.focused`).
pub const BOOLEAN_STRING_VALUES: &[&str] = &["true", "false"];

/// A low-frequency liveness signal recorded whether or not anything else is
/// happening. A gap between heartbeats — or the absence of the next one —
/// is itself the diagnostic signal for a slow or stuck frontend; visibility
/// and focus are carried alongside it so a gap while hidden (background
/// timer throttling) is not mistaken for one while the window was actually
/// active.
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

/// A periodic, content-free snapshot of workspace shape: counts and a
/// length bucket only, never transcript text, draft text, or paths.
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

/// A raw frontend-measured duration with no native span/log counterpart —
/// event-loop lag and long-task measurements only exist on the frontend, so
/// unlike every other Stage 5 metric, these two cross IPC as a dedicated
/// `FrontendMetricRecord` (see `ingest.rs`) instead of being derived from an
/// already-ingested span or log.
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
    &FRONTEND_HEARTBEAT,
    &FRONTEND_STATE_SUMMARY,
    &FRONTEND_EVENT_LOOP_LAG,
    &FRONTEND_LONG_TASK,
];

pub fn find_family(name: &str) -> Option<&'static RecordFamily> {
    FAMILIES.iter().copied().find(|family| family.name == name)
}

/// Resolves `key` against `family`'s own attributes and the shared context
/// attributes, in that order.
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
/// Stage 5 checks metric dimensions against this before recording them.
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

/// Validates `value` for `key` within `family`: the key must be allowlisted,
/// its runtime type must match the spec, and string values must fit the
/// spec's maximum length. This is the enforcement point that keeps telemetry
/// callers from recording arbitrary attributes.
const MAX_COUNT_ATTRIBUTE: i64 = 1_000_000_000;
const MAX_AGE_ATTRIBUTE_MS: i64 = 24 * 60 * 60 * 1000;

fn numeric_range(key: &str) -> Option<(i64, i64)> {
    if key.ends_with("_count") || key == "tau.heartbeat.queue_length" {
        return Some((0, MAX_COUNT_ATTRIBUTE));
    }
    if key == "pi.generation" {
        return Some((0, MAX_COUNT_ATTRIBUTE));
    }
    if key == "tau.state.oldest_pending_rpc_age_ms" {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all_specs() -> impl Iterator<Item = &'static AttributeSpec> {
        RESOURCE_ATTRIBUTES
            .iter()
            .chain(CONTEXT_ATTRIBUTES)
            .chain(FAMILIES.iter().flat_map(|family| family.attributes))
    }

    #[test]
    fn catalog_specs_are_well_formed() {
        for spec in all_specs() {
            match spec.kind {
                AttributeKind::Str => assert!(
                    spec.max_len.is_some(),
                    "{} is a string attribute without a max length",
                    spec.key
                ),
                AttributeKind::I64 => assert!(
                    spec.max_len.is_none(),
                    "{} is a numeric attribute with a max length",
                    spec.key
                ),
            }
        }
    }

    #[test]
    fn context_identifiers_are_never_metric_safe() {
        for spec in CONTEXT_ATTRIBUTES {
            assert!(
                !spec.metric_safe,
                "{} must stay off metric dimensions",
                spec.key
            );
        }
    }

    #[test]
    fn family_attributes_are_reachable_by_name() {
        let spec = allowed_attribute("pi.rpc", "pi.rpc.method").expect("known attribute");
        assert_eq!(spec.kind, AttributeKind::Str);
    }

    #[test]
    fn context_attributes_are_reachable_from_every_family() {
        let spec = allowed_attribute("ui.action", "tau.session.id").expect("context attribute");
        assert!(!spec.metric_safe);
    }

    #[test]
    fn unknown_family_or_key_is_rejected() {
        assert!(allowed_attribute("does.not.exist", "tau.session.id").is_none());
        assert!(allowed_attribute("ui.action", "tau.does.not.exist").is_none());
    }

    #[test]
    fn validate_accepts_a_well_formed_attribute() {
        assert_eq!(
            validate(
                "pi.rpc",
                "pi.rpc.method",
                &Value::String("get_state".into())
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "pi.process.lifecycle",
                "tau.process.exit_code",
                &Value::I64(0)
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_rejects_an_unknown_attribute() {
        assert_eq!(
            validate(
                "pi.rpc",
                "pi.rpc.transcript",
                &Value::String("hello".into())
            ),
            Err(AttributeError::UnknownAttribute)
        );
    }

    #[test]
    fn validate_rejects_an_unreviewed_categorical_value() {
        assert_eq!(
            validate(
                "tauri.invoke",
                "tau.invoke.command",
                &Value::String("user content".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn validate_rejects_a_type_mismatch() {
        assert_eq!(
            validate("pi.rpc", "pi.rpc.method", &Value::I64(1)),
            Err(AttributeError::WrongType)
        );
    }

    #[test]
    fn validate_rejects_negative_or_absurd_collection_counts() {
        for value in [-1, MAX_COUNT_ATTRIBUTE + 1] {
            assert_eq!(
                validate(
                    "frontend.heartbeat",
                    "tau.heartbeat.pending_rpc_count",
                    &Value::I64(value)
                ),
                Err(AttributeError::OutOfRange)
            );
        }
        assert_eq!(
            validate("pi.rpc", "pi.generation", &Value::I64(-1)),
            Err(AttributeError::OutOfRange)
        );
    }

    #[test]
    fn validate_rejects_an_oversized_string() {
        let oversized = "x".repeat(DEFAULT_MAX_ATTRIBUTE_LEN + 1);
        assert_eq!(
            validate("pi.rpc", "pi.rpc.method", &Value::String(oversized.into())),
            Err(AttributeError::TooLong)
        );
    }

    #[test]
    fn metric_safety_matches_the_catalog() {
        assert!(is_metric_safe("tau.action.name"));
        assert!(!is_metric_safe("tau.session.id"));
        assert!(!is_metric_safe("unknown.key"));
    }

    #[test]
    fn validate_accepts_every_reviewed_action_name() {
        for name in UI_ACTION_NAMES {
            assert_eq!(
                validate(
                    "ui.action",
                    "tau.action.name",
                    &Value::String((*name).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_rejects_an_unreviewed_action_name() {
        assert_eq!(
            validate(
                "ui.action",
                "tau.action.name",
                &Value::String("not.a.real.action".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn validate_accepts_every_reviewed_invoke_command_and_outcome() {
        for command in TAURI_INVOKE_COMMANDS {
            assert_eq!(
                validate(
                    "tauri.invoke",
                    "tau.invoke.command",
                    &Value::String((*command).into())
                ),
                Ok(())
            );
        }
        for outcome in TAURI_INVOKE_OUTCOMES {
            assert_eq!(
                validate(
                    "tauri.invoke",
                    "tau.invoke.outcome",
                    &Value::String((*outcome).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_accepts_every_reviewed_rpc_method_and_outcome() {
        for method in PI_RPC_METHODS {
            assert_eq!(
                validate("pi.rpc", "pi.rpc.method", &Value::String((*method).into())),
                Ok(())
            );
        }
        for outcome in PI_RPC_OUTCOMES {
            assert_eq!(
                validate(
                    "pi.rpc",
                    "pi.rpc.outcome",
                    &Value::String((*outcome).into())
                ),
                Ok(())
            );
        }
        for kind in PI_RPC_ANOMALY_KINDS {
            assert_eq!(
                validate(
                    "pi.rpc.anomaly",
                    "pi.rpc.anomaly.kind",
                    &Value::String((*kind).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_rejects_an_unreviewed_rpc_outcome() {
        assert_eq!(
            validate(
                "pi.rpc",
                "pi.rpc.outcome",
                &Value::String("not-a-real-outcome".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn validate_accepts_stream_aggregate_counts() {
        assert_eq!(
            validate("pi.stream", "pi.stream.delta_count", &Value::I64(3)),
            Ok(())
        );
        assert_eq!(
            validate("pi.stream", "pi.stream.character_count", &Value::I64(42)),
            Ok(())
        );
    }

    #[test]
    fn validate_accepts_reviewed_ownership_attributes() {
        for kind in PI_OWNERSHIP_KINDS {
            assert_eq!(
                validate(
                    "pi.ownership",
                    "tau.ownership.kind",
                    &Value::String((*kind).into())
                ),
                Ok(())
            );
        }
        for outcome in PI_OWNERSHIP_OUTCOMES {
            assert_eq!(
                validate(
                    "pi.ownership",
                    "tau.ownership.outcome",
                    &Value::String((*outcome).into())
                ),
                Ok(())
            );
        }
        assert_eq!(
            validate(
                "pi.ownership",
                "tau.ownership.stale_process_count",
                &Value::I64(2)
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "pi.ownership",
                "tau.ownership.kind",
                &Value::String("owner-secret".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn validate_accepts_every_reviewed_process_stop_reason_and_resolution() {
        for reason in PI_PROCESS_STOP_REASONS {
            assert_eq!(
                validate(
                    "pi.process.lifecycle",
                    "tau.process.stop_reason",
                    &Value::String((*reason).into())
                ),
                Ok(())
            );
        }
        for resolution in PI_PROCESS_RESOLUTIONS {
            assert_eq!(
                validate(
                    "pi.process.lifecycle",
                    "tau.process.resolution",
                    &Value::String((*resolution).into())
                ),
                Ok(())
            );
        }
        for outcome in PI_PROCESS_EXIT_OUTCOMES {
            assert_eq!(
                validate(
                    "pi.process.lifecycle",
                    "tau.process.exit_outcome",
                    &Value::String((*outcome).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn pi_rpc_span_can_carry_runtime_and_generation_context() {
        assert_eq!(
            validate(
                "pi.rpc",
                "tau.runtime.id",
                &Value::String("runtime-1".into())
            ),
            Ok(())
        );
        assert_eq!(validate("pi.rpc", "pi.generation", &Value::I64(2)), Ok(()));
    }

    /// No forbidden-content canary is a reviewed categorical value for any
    /// attribute this catalog generalizes through `categorical_values`, so
    /// every one of them must be rejected as an unreviewed value rather than
    /// accidentally matching a real enum entry.
    #[test]
    fn no_forbidden_content_canary_is_ever_a_reviewed_categorical_value() {
        use crate::telemetry::privacy::FORBIDDEN_CONTENT_CANARIES;

        let categorical_attributes = [
            ("ui.action", "tau.action.name"),
            ("tauri.invoke", "tau.invoke.command"),
            ("tauri.invoke", "tau.invoke.outcome"),
            ("pi.rpc", "pi.rpc.method"),
            ("pi.rpc", "pi.rpc.outcome"),
            ("pi.rpc.anomaly", "pi.rpc.anomaly.kind"),
            ("pi.process.lifecycle", "tau.process.stop_reason"),
            ("pi.process.lifecycle", "tau.process.resolution"),
            ("pi.process.lifecycle", "tau.process.exit_outcome"),
            ("pi.reader", "tau.reader.drop_reason"),
            ("pi.reader", "tau.reader.error_kind"),
            ("pi.reader", "tau.event.kind"),
            ("frontend.error", "tau.error.source"),
            ("frontend.error", "tau.error.kind"),
            ("controller.lifecycle", "tau.controller.state.before"),
            ("controller.lifecycle", "tau.controller.state.after"),
            ("controller.lifecycle", "tau.controller.transition.cause"),
            ("operation.checkpoint", "tau.operation.family"),
            ("operation.checkpoint", "tau.operation.name"),
            ("frontend.heartbeat", "tau.heartbeat.visibility"),
            ("frontend.heartbeat", "tau.heartbeat.focused"),
            ("frontend.state_summary", "tau.state.draft_bucket"),
        ];
        for (family, key) in categorical_attributes {
            for (_, canary) in FORBIDDEN_CONTENT_CANARIES {
                assert_eq!(
                    validate(family, key, &Value::String((*canary).into())),
                    Err(AttributeError::UnknownValue),
                    "{family}/{key} accepted the {canary} canary",
                );
            }
        }
    }

    #[test]
    fn validate_accepts_every_reviewed_reader_and_frontend_error_value() {
        for reason in PI_READER_DROP_REASONS {
            assert_eq!(
                validate(
                    "pi.reader",
                    "tau.reader.drop_reason",
                    &Value::String((*reason).into())
                ),
                Ok(())
            );
        }
        for kind in PI_READER_ERROR_KINDS {
            assert_eq!(
                validate(
                    "pi.reader",
                    "tau.reader.error_kind",
                    &Value::String((*kind).into())
                ),
                Ok(())
            );
        }
        for kind in PI_EVENT_KINDS {
            assert_eq!(
                validate(
                    "pi.reader",
                    "tau.event.kind",
                    &Value::String((*kind).into())
                ),
                Ok(())
            );
        }
        for source in FRONTEND_ERROR_SOURCES {
            assert_eq!(
                validate(
                    "frontend.error",
                    "tau.error.source",
                    &Value::String((*source).into())
                ),
                Ok(())
            );
        }
        for kind in FRONTEND_ERROR_KINDS {
            assert_eq!(
                validate(
                    "frontend.error",
                    "tau.error.kind",
                    &Value::String((*kind).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_rejects_an_unreviewed_reader_drop_reason() {
        assert_eq!(
            validate(
                "pi.reader",
                "tau.reader.drop_reason",
                &Value::String("not-a-real-reason".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn telemetry_health_and_rust_panic_accept_their_declared_attributes() {
        assert_eq!(
            validate(
                "telemetry.health",
                "tau.telemetry.dropped_count",
                &Value::I64(3)
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "telemetry.health",
                "tau.telemetry.failed_write_count",
                &Value::I64(1)
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "rust.panic",
                "tau.error.location",
                &Value::String("pi.rs:42:5".into())
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_accepts_every_reviewed_controller_lifecycle_value() {
        for state in CONTROLLER_LIFECYCLE_STATES {
            assert_eq!(
                validate(
                    "controller.lifecycle",
                    "tau.controller.state.before",
                    &Value::String((*state).into())
                ),
                Ok(())
            );
            assert_eq!(
                validate(
                    "controller.lifecycle",
                    "tau.controller.state.after",
                    &Value::String((*state).into())
                ),
                Ok(())
            );
        }
        for cause in CONTROLLER_LIFECYCLE_CAUSES {
            assert_eq!(
                validate(
                    "controller.lifecycle",
                    "tau.controller.transition.cause",
                    &Value::String((*cause).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_rejects_an_unreviewed_controller_lifecycle_state() {
        assert_eq!(
            validate(
                "controller.lifecycle",
                "tau.controller.state.before",
                &Value::String("not-a-real-state".into())
            ),
            Err(AttributeError::UnknownValue)
        );
    }

    #[test]
    fn validate_accepts_every_reviewed_operation_checkpoint_value() {
        for family in OPERATION_CHECKPOINT_FAMILIES {
            assert_eq!(
                validate(
                    "operation.checkpoint",
                    "tau.operation.family",
                    &Value::String((*family).into())
                ),
                Ok(())
            );
        }
        for name in OPERATION_CHECKPOINT_NAMES {
            assert_eq!(
                validate(
                    "operation.checkpoint",
                    "tau.operation.name",
                    &Value::String((*name).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn validate_accepts_every_reviewed_heartbeat_and_state_summary_value() {
        for visibility in HEARTBEAT_VISIBILITY_VALUES {
            assert_eq!(
                validate(
                    "frontend.heartbeat",
                    "tau.heartbeat.visibility",
                    &Value::String((*visibility).into())
                ),
                Ok(())
            );
        }
        for focused in BOOLEAN_STRING_VALUES {
            assert_eq!(
                validate(
                    "frontend.heartbeat",
                    "tau.heartbeat.focused",
                    &Value::String((*focused).into())
                ),
                Ok(())
            );
        }
        for bucket in DRAFT_LENGTH_BUCKETS {
            assert_eq!(
                validate(
                    "frontend.state_summary",
                    "tau.state.draft_bucket",
                    &Value::String((*bucket).into())
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn frontend_metric_families_accept_their_declared_attributes() {
        assert_eq!(
            validate(
                "frontend.event_loop_lag",
                "tau.heartbeat.visibility",
                &Value::String("visible".into())
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "frontend.event_loop_lag",
                "tau.heartbeat.focused",
                &Value::String("true".into())
            ),
            Ok(())
        );
        // `frontend.long_task` declares no attributes of its own: even a
        // context attribute like `tau.session.id` is still allowed (shared
        // context, not family-specific), but nothing family-specific is.
        assert_eq!(
            validate(
                "frontend.long_task",
                "tau.error.kind",
                &Value::String("Error".into())
            ),
            Err(AttributeError::UnknownAttribute)
        );
    }
}
