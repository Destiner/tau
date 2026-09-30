/*
 * The `tau.*`/`pi.*` and OpenTelemetry resource attribute catalog. Every
 * record family later stages emit must draw its attributes from here, so the
 * allowlist lives in one authoritative place instead of being repeated (and
 * drifting) at each call site. Mirrors src-tauri/src/telemetry/attributes.rs.
 */
import type { AttributeValue } from '@opentelemetry/api';
import {
  ATTR_DEPLOYMENT_ENVIRONMENT_NAME,
  ATTR_SERVICE_INSTANCE_ID,
  ATTR_SERVICE_NAME,
  ATTR_SERVICE_VERSION,
} from '@opentelemetry/semantic-conventions';
import {
  ATTR_HOST_ARCH,
  ATTR_OS_TYPE,
} from '@opentelemetry/semantic-conventions/incubating';

import { DEFAULT_MAX_ATTRIBUTE_LEN, utf8Length } from './privacy';

type AttributeKind = 'string' | 'int';

interface AttributeSpec {
  key: string;
  kind: AttributeKind;
  maxLen: number | null;
  metricSafe: boolean;
}

type AttributeError =
  | 'unknown-attribute'
  | 'unknown-value'
  | 'wrong-type'
  | 'out-of-range'
  | 'too-long';

type AttributeValidation =
  { valid: true } | { valid: false; error: AttributeError };

interface RecordFamily {
  name: string;
  attributes: readonly AttributeSpec[];
}

function stringAttribute(key: string, metricSafe: boolean): AttributeSpec {
  return { key, kind: 'string', maxLen: DEFAULT_MAX_ATTRIBUTE_LEN, metricSafe };
}

const RESOURCE_ATTRIBUTES: readonly AttributeSpec[] = [
  stringAttribute(ATTR_SERVICE_NAME, true),
  stringAttribute(ATTR_SERVICE_VERSION, true),
  stringAttribute(ATTR_SERVICE_INSTANCE_ID, true),
  stringAttribute(ATTR_DEPLOYMENT_ENVIRONMENT_NAME, true),
  stringAttribute(ATTR_OS_TYPE, true),
  stringAttribute(ATTR_HOST_ARCH, true),
];

const CONTEXT_ATTRIBUTES: readonly AttributeSpec[] = [
  stringAttribute('tau.project.id', false),
  stringAttribute('tau.session.id', false),
  stringAttribute('tau.controller.id', false),
  stringAttribute('tau.runtime.id', false),
  { key: 'pi.generation', kind: 'int', maxLen: null, metricSafe: false },
];

const UI_ACTION_NAMES = [
  'session.select',
  'session.new',
  'message.send',
  'session.stop',
  'session.rename',
  'model.select',
  'effort.select',
  'extension.dialog.submit',
  'extension.dialog.cancel',
] as const;
type UiActionName = (typeof UI_ACTION_NAMES)[number];

const UI_ACTION: RecordFamily = {
  name: 'ui.action',
  attributes: [stringAttribute('tau.action.name', true)],
};

const TAURI_INVOKE_COMMANDS = [
  'load_workspace',
  'set_admin_mode',
  'submit_issue_report',
  'import_project',
  'import_remote_project',
  'remove_project',
  'set_active_project',
  'set_project_collapsed',
  'reorder_projects',
  'set_active_session',
  'archive_session',
  'unarchive_session',
  'register_session',
  'probe_remote_project',
  'list_remote_directories',
  'prepare_file_preview',
  'release_file_preview',
  'read_model_scope',
  'read_remote_model_scope',
  'request_quit',
  'resolve_quit_request',
  'update_snapshot',
  'check_for_update',
  'get_dismissed_update_version',
  'set_dismissed_update_version',
  'download_update',
  'request_update_restart',
  'install_update',
  'restart_after_update',
  'start_pi',
  'start_pi_remote',
  'stop_pi',
  'read_pi_frontend_revision',
  'claim_pi_frontend',
] as const;
type TauriInvokeCommand = (typeof TAURI_INVOKE_COMMANDS)[number];

const TAURI_INVOKE_OUTCOMES = ['success', 'error'] as const;
type TauriInvokeOutcome = (typeof TAURI_INVOKE_OUTCOMES)[number];

const TAURI_INVOKE: RecordFamily = {
  name: 'tauri.invoke',
  attributes: [
    stringAttribute('tau.invoke.command', true),
    stringAttribute('tau.invoke.outcome', true),
  ],
};

const PI_RPC_METHODS = [
  'get_state',
  'get_available_models',
  'get_commands',
  'get_available_thinking_levels',
  'get_messages',
  'get_entries',
  'set_model',
  'set_thinking_level',
  'set_session_name',
  'prompt',
  'set_steering_mode',
  'set_follow_up_mode',
  'clear_queue',
  'abort',
  'extension_ui_response',
] as const;
type PiRpcMethod = (typeof PI_RPC_METHODS)[number];

const PI_RPC_OUTCOMES = [
  'success',
  'error',
  'timeout',
  'abandoned_process_exit',
  'abandoned_generation_change',
  'abandoned_stop',
  'abandoned_replacement',
  'abandoned_duplicate_request',
] as const;
type PiRpcOutcome = (typeof PI_RPC_OUTCOMES)[number];

const PI_RPC: RecordFamily = {
  name: 'pi.rpc',
  attributes: [
    stringAttribute('pi.rpc.method', true),
    stringAttribute('pi.rpc.request_id', false),
    stringAttribute('pi.rpc.outcome', true),
  ],
};

const PI_RPC_ANOMALY_KINDS = ['unmatched_or_duplicate'] as const;
type PiRpcAnomalyKind = (typeof PI_RPC_ANOMALY_KINDS)[number];

const PI_RPC_ANOMALY: RecordFamily = {
  name: 'pi.rpc.anomaly',
  attributes: [
    stringAttribute('pi.rpc.anomaly.kind', true),
    stringAttribute('pi.rpc.request_id', false),
  ],
};

const PI_STREAM: RecordFamily = {
  name: 'pi.stream',
  attributes: [
    {
      key: 'pi.stream.delta_count',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
    {
      key: 'pi.stream.character_count',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
  ],
};

const CONTROLLER_LIFECYCLE_STATES = [
  'idle',
  'connecting',
  'starting',
  'stopping',
  'syncing',
  'working',
  'ready',
] as const;
type ControllerLifecycleState = (typeof CONTROLLER_LIFECYCLE_STATES)[number];

const CONTROLLER_LIFECYCLE_CAUSES = [
  'controller_start',
  'controller_start_failed',
  'phantom_prompt_start',
  'phantom_prompt_resume',
  'process_exited',
  'agent_start',
  'agent_settled',
  'materialization_retry',
  'prompt_response',
  'get_state_failed',
  'get_messages_failed',
  'prompt_failed',
  'abort_failed',
  'abort_acknowledged',
  'get_state_response',
  'get_messages_response',
  'pending_prompt_dispatch',
  'pending_prompt_failed',
  'abort_probe_failed',
  'pending_prompt_cancelled',
  'process_stopped',
  'bridge_event_failed',
  'workspace_load_failed',
  'message_send',
  'message_send_failed',
  'stop_requested',
  'stop_failed',
] as const;
type ControllerLifecycleCause = (typeof CONTROLLER_LIFECYCLE_CAUSES)[number];

const CONTROLLER_LIFECYCLE: RecordFamily = {
  name: 'controller.lifecycle',
  attributes: [
    stringAttribute('tau.controller.state.before', true),
    stringAttribute('tau.controller.state.after', true),
    stringAttribute('tau.controller.transition.cause', true),
  ],
};

const PI_PROCESS_STOP_REASONS = [
  'explicit_stop',
  'replaced',
  'ownership_replaced',
] as const;
const PI_PROCESS_RESOLUTIONS = ['found', 'not_found'] as const;
const PI_PROCESS_EXIT_OUTCOMES = ['clean', 'unexpected'] as const;

const PI_OWNERSHIP_KINDS = ['initial', 'replacement'] as const;
const PI_OWNERSHIP_OUTCOMES = [
  'success',
  'cleanup_failed',
  'rejected',
] as const;
const PI_OWNERSHIP: RecordFamily = {
  name: 'pi.ownership',
  attributes: [
    stringAttribute('tau.ownership.kind', true),
    stringAttribute('tau.ownership.outcome', true),
    {
      key: 'tau.ownership.stale_process_count',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
  ],
};

const PI_PROCESS_LIFECYCLE: RecordFamily = {
  name: 'pi.process.lifecycle',
  attributes: [
    stringAttribute('tau.process.resolution', true),
    stringAttribute('tau.process.stop_reason', true),
    stringAttribute('tau.process.exit_outcome', true),
    {
      key: 'tau.process.exit_code',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
  ],
};

const APP_LIFECYCLE: RecordFamily = { name: 'app.lifecycle', attributes: [] };

const PI_READER_DROP_REASONS = [
  'oversized',
  'invalid_utf8',
  'malformed',
] as const;

const PI_READER_ERROR_KINDS = [
  'broken_pipe',
  'interrupted',
  'unexpected_eof',
  'other',
] as const;

const PI_EVENT_KINDS = ['started', 'rpc', 'stderr', 'error', 'exited'] as const;

const PI_READER: RecordFamily = {
  name: 'pi.reader',
  attributes: [
    stringAttribute('tau.reader.drop_reason', true),
    stringAttribute('tau.reader.error_kind', true),
    stringAttribute('tau.event.kind', true),
  ],
};

const TELEMETRY_HEALTH: RecordFamily = {
  name: 'telemetry.health',
  attributes: [
    {
      key: 'tau.telemetry.dropped_count',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
    {
      key: 'tau.telemetry.failed_write_count',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
  ],
};

const RUST_PANIC: RecordFamily = {
  name: 'rust.panic',
  attributes: [stringAttribute('tau.error.location', false)],
};

const FRONTEND_ERROR_SOURCES = [
  'window_error',
  'unhandled_rejection',
  'vue_error',
  'console_error',
] as const;
type FrontendErrorSource = (typeof FRONTEND_ERROR_SOURCES)[number];

const FRONTEND_ERROR_KINDS = [
  'Error',
  'TypeError',
  'RangeError',
  'ReferenceError',
  'SyntaxError',
  'EvalError',
  'URIError',
  'other',
  'none',
] as const;
type FrontendErrorKind = (typeof FRONTEND_ERROR_KINDS)[number];

const FRONTEND_ERROR: RecordFamily = {
  name: 'frontend.error',
  attributes: [
    stringAttribute('tau.error.source', true),
    stringAttribute('tau.error.kind', true),
    stringAttribute('tau.error.location', false),
  ],
};

const OPERATION_CHECKPOINT_FAMILIES = ['ui.action', 'pi.rpc'] as const;
type OperationCheckpointFamily = (typeof OPERATION_CHECKPOINT_FAMILIES)[number];

const OPERATION_CHECKPOINT_NAMES = [
  ...UI_ACTION_NAMES,
  ...PI_RPC_METHODS,
] as const;

/** A linked start/checkpoint log for an operation that might never finish:
 * recorded the moment its span starts (carrying that span's own `traceId`/
 * `spanId`), so the operation stays visible in the persisted timeline even
 * if the span itself never ends — a hang, a crash, or an abandoned request
 * all leave an open span an OTel exporter never gets to write. */
const OPERATION_CHECKPOINT: RecordFamily = {
  name: 'operation.checkpoint',
  attributes: [
    stringAttribute('tau.operation.family', true),
    stringAttribute('tau.operation.name', true),
    stringAttribute('pi.rpc.request_id', false),
  ],
};

const ACTION_MILESTONE_KINDS = [
  'ready',
  'persisted',
  'persistence_failed',
  'readable_memory',
  'readable_saved',
  'readable_rpc',
  'hydrated',
  'paint_opportunity',
  'paint_unavailable',
] as const;
type ActionMilestoneKind = (typeof ACTION_MILESTONE_KINDS)[number];

const ACTION_MILESTONE: RecordFamily = {
  name: 'action.milestone',
  attributes: [
    stringAttribute('tau.action.name', true),
    stringAttribute('tau.action.milestone', true),
    {
      key: 'tau.action.elapsed_ms',
      kind: 'int',
      maxLen: null,
      metricSafe: false,
    },
  ],
};

const HEARTBEAT_VISIBILITY_VALUES = ['visible', 'hidden'] as const;
type HeartbeatVisibility = (typeof HEARTBEAT_VISIBILITY_VALUES)[number];

const BOOLEAN_STRING_VALUES = ['true', 'false'] as const;

function intAttribute(key: string): AttributeSpec {
  return { key, kind: 'int', maxLen: null, metricSafe: true };
}

const FRONTEND_HEARTBEAT: RecordFamily = {
  name: 'frontend.heartbeat',
  attributes: [
    stringAttribute('tau.heartbeat.visibility', true),
    stringAttribute('tau.heartbeat.focused', true),
    intAttribute('tau.heartbeat.pending_rpc_count'),
    intAttribute('tau.heartbeat.controller_count'),
    intAttribute('tau.heartbeat.active_controller_count'),
    intAttribute('tau.heartbeat.runtime_count'),
    intAttribute('tau.heartbeat.queue_length'),
  ],
};

const DRAFT_LENGTH_BUCKETS = ['empty', 'short', 'medium', 'long'] as const;
type DraftLengthBucket = (typeof DRAFT_LENGTH_BUCKETS)[number];

const FRONTEND_STATE_SUMMARY: RecordFamily = {
  name: 'frontend.state_summary',
  attributes: [
    intAttribute('tau.state.controller_count'),
    intAttribute('tau.state.runtime_count'),
    intAttribute('tau.state.pending_rpc_count'),
    intAttribute('tau.state.notification_count'),
    intAttribute('tau.state.dialog_count'),
    intAttribute('tau.state.transcript.user_count'),
    intAttribute('tau.state.transcript.assistant_count'),
    intAttribute('tau.state.transcript.tool_count'),
    intAttribute('tau.state.transcript.thinking_count'),
    intAttribute('tau.state.transcript.error_count'),
    stringAttribute('tau.state.draft_bucket', true),
    intAttribute('tau.state.oldest_pending_rpc_age_ms'),
  ],
};

const FRONTEND_EVENT_LOOP_LAG: RecordFamily = {
  name: 'frontend.event_loop_lag',
  attributes: [
    stringAttribute('tau.heartbeat.visibility', true),
    stringAttribute('tau.heartbeat.focused', true),
  ],
};

const FRONTEND_LONG_TASK: RecordFamily = {
  name: 'frontend.long_task',
  attributes: [],
};

const FAMILIES: readonly RecordFamily[] = [
  UI_ACTION,
  TAURI_INVOKE,
  PI_RPC,
  PI_RPC_ANOMALY,
  PI_STREAM,
  CONTROLLER_LIFECYCLE,
  PI_OWNERSHIP,
  PI_PROCESS_LIFECYCLE,
  APP_LIFECYCLE,
  PI_READER,
  TELEMETRY_HEALTH,
  RUST_PANIC,
  FRONTEND_ERROR,
  OPERATION_CHECKPOINT,
  ACTION_MILESTONE,
  FRONTEND_HEARTBEAT,
  FRONTEND_STATE_SUMMARY,
  FRONTEND_EVENT_LOOP_LAG,
  FRONTEND_LONG_TASK,
];

function findFamily(name: string): RecordFamily | undefined {
  return FAMILIES.find((family) => family.name === name);
}

function allowedAttribute(
  family: string,
  key: string,
): AttributeSpec | undefined {
  const found = findFamily(family);
  if (!found) return undefined;
  return [...found.attributes, ...CONTEXT_ATTRIBUTES].find(
    (spec) => spec.key === key,
  );
}

function isMetricSafe(key: string): boolean {
  const catalog = [
    ...RESOURCE_ATTRIBUTES,
    ...CONTEXT_ATTRIBUTES,
    ...FAMILIES.flatMap((family) => family.attributes),
  ];
  return catalog.find((spec) => spec.key === key)?.metricSafe ?? false;
}

function categoricalValues(key: string): readonly string[] | undefined {
  switch (key) {
    case 'tau.action.name':
      return UI_ACTION_NAMES;
    case 'tau.action.milestone':
      return ACTION_MILESTONE_KINDS;
    case 'tau.invoke.command':
      return TAURI_INVOKE_COMMANDS;
    case 'tau.invoke.outcome':
      return TAURI_INVOKE_OUTCOMES;
    case 'pi.rpc.method':
      return PI_RPC_METHODS;
    case 'pi.rpc.outcome':
      return PI_RPC_OUTCOMES;
    case 'pi.rpc.anomaly.kind':
      return PI_RPC_ANOMALY_KINDS;
    case 'tau.ownership.kind':
      return PI_OWNERSHIP_KINDS;
    case 'tau.ownership.outcome':
      return PI_OWNERSHIP_OUTCOMES;
    case 'tau.process.stop_reason':
      return PI_PROCESS_STOP_REASONS;
    case 'tau.process.resolution':
      return PI_PROCESS_RESOLUTIONS;
    case 'tau.process.exit_outcome':
      return PI_PROCESS_EXIT_OUTCOMES;
    case 'tau.reader.drop_reason':
      return PI_READER_DROP_REASONS;
    case 'tau.reader.error_kind':
      return PI_READER_ERROR_KINDS;
    case 'tau.event.kind':
      return PI_EVENT_KINDS;
    case 'tau.error.source':
      return FRONTEND_ERROR_SOURCES;
    case 'tau.error.kind':
      return FRONTEND_ERROR_KINDS;
    case 'tau.controller.state.before':
    case 'tau.controller.state.after':
      return CONTROLLER_LIFECYCLE_STATES;
    case 'tau.controller.transition.cause':
      return CONTROLLER_LIFECYCLE_CAUSES;
    case 'tau.operation.family':
      return OPERATION_CHECKPOINT_FAMILIES;
    case 'tau.operation.name':
      return OPERATION_CHECKPOINT_NAMES;
    case 'tau.heartbeat.visibility':
      return HEARTBEAT_VISIBILITY_VALUES;
    case 'tau.heartbeat.focused':
      return BOOLEAN_STRING_VALUES;
    case 'tau.state.draft_bucket':
      return DRAFT_LENGTH_BUCKETS;
    default:
      return undefined;
  }
}

const MAX_COUNT_ATTRIBUTE = 1_000_000_000;
const MAX_AGE_ATTRIBUTE_MS = 24 * 60 * 60 * 1000;

function numericRange(key: string): readonly [number, number] | undefined {
  if (key.endsWith('_count') || key === 'tau.heartbeat.queue_length') {
    return [0, MAX_COUNT_ATTRIBUTE];
  }
  if (key === 'pi.generation') return [0, MAX_COUNT_ATTRIBUTE];
  if (key === 'tau.action.elapsed_ms') return [0, MAX_AGE_ATTRIBUTE_MS];
  if (key === 'tau.state.oldest_pending_rpc_age_ms') {
    return [0, MAX_AGE_ATTRIBUTE_MS];
  }
  return undefined;
}

function validateAttribute(
  family: string,
  key: string,
  value: AttributeValue,
): AttributeValidation {
  const spec = allowedAttribute(family, key);
  if (!spec) return { valid: false, error: 'unknown-attribute' };

  if (spec.kind === 'string') {
    if (typeof value !== 'string') return { valid: false, error: 'wrong-type' };
    if (spec.maxLen !== null && utf8Length(value) > spec.maxLen) {
      return { valid: false, error: 'too-long' };
    }
    const allowedValues = categoricalValues(key);
    if (allowedValues && !allowedValues.includes(value)) {
      return { valid: false, error: 'unknown-value' };
    }
    return { valid: true };
  }

  if (typeof value !== 'number' || !Number.isSafeInteger(value)) {
    return { valid: false, error: 'wrong-type' };
  }
  const range = numericRange(key);
  if (range && (value < range[0] || value > range[1])) {
    return { valid: false, error: 'out-of-range' };
  }
  return { valid: true };
}

export type {
  AttributeError,
  AttributeKind,
  AttributeSpec,
  ActionMilestoneKind,
  ControllerLifecycleCause,
  ControllerLifecycleState,
  DraftLengthBucket,
  FrontendErrorKind,
  FrontendErrorSource,
  HeartbeatVisibility,
  OperationCheckpointFamily,
  PiRpcAnomalyKind,
  PiRpcMethod,
  PiRpcOutcome,
  RecordFamily,
  TauriInvokeCommand,
  TauriInvokeOutcome,
  UiActionName,
};

export {
  allowedAttribute,
  ACTION_MILESTONE,
  ACTION_MILESTONE_KINDS,
  APP_LIFECYCLE,
  CONTEXT_ATTRIBUTES,
  CONTROLLER_LIFECYCLE,
  CONTROLLER_LIFECYCLE_CAUSES,
  CONTROLLER_LIFECYCLE_STATES,
  DRAFT_LENGTH_BUCKETS,
  FAMILIES,
  findFamily,
  FRONTEND_ERROR,
  FRONTEND_ERROR_KINDS,
  FRONTEND_ERROR_SOURCES,
  FRONTEND_EVENT_LOOP_LAG,
  FRONTEND_HEARTBEAT,
  FRONTEND_LONG_TASK,
  FRONTEND_STATE_SUMMARY,
  isMetricSafe,
  OPERATION_CHECKPOINT,
  OPERATION_CHECKPOINT_FAMILIES,
  OPERATION_CHECKPOINT_NAMES,
  PI_OWNERSHIP,
  PI_OWNERSHIP_KINDS,
  PI_OWNERSHIP_OUTCOMES,
  PI_PROCESS_EXIT_OUTCOMES,
  PI_PROCESS_LIFECYCLE,
  PI_PROCESS_RESOLUTIONS,
  PI_PROCESS_STOP_REASONS,
  PI_RPC,
  PI_RPC_ANOMALY,
  PI_RPC_ANOMALY_KINDS,
  PI_RPC_METHODS,
  PI_RPC_OUTCOMES,
  PI_STREAM,
  RESOURCE_ATTRIBUTES,
  TAURI_INVOKE,
  TAURI_INVOKE_COMMANDS,
  TAURI_INVOKE_OUTCOMES,
  TELEMETRY_HEALTH,
  UI_ACTION,
  UI_ACTION_NAMES,
  validateAttribute,
};
