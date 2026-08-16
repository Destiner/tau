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

/** The OpenTelemetry value shapes this catalog allows. Kept narrower than
 * `AttributeValue`: nothing in the initial catalog needs a float, boolean, or
 * array, and adding one should be a deliberate catalog change. */
type AttributeKind = 'string' | 'int';

interface AttributeSpec {
  key: string;
  kind: AttributeKind;
  /** Maximum length for string values. Always set for `'string'` kinds;
   * always `null` otherwise, which a catalog test enforces. */
  maxLen: number | null;
  /** Whether this key may appear on a metric's dimensions. `false` marks
   * session, project, controller, runtime, request, and trace identifiers,
   * which the design principles forbid on metrics because they would create
   * unbounded cardinality. */
  metricSafe: boolean;
}

type AttributeError =
  'unknown-attribute' | 'unknown-value' | 'wrong-type' | 'too-long';

type AttributeValidation =
  { valid: true } | { valid: false; error: AttributeError };

interface RecordFamily {
  name: string;
  attributes: readonly AttributeSpec[];
}

function stringAttribute(key: string, metricSafe: boolean): AttributeSpec {
  return { key, kind: 'string', maxLen: DEFAULT_MAX_ATTRIBUTE_LEN, metricSafe };
}

/** Resource attributes attached once per app run (Stage 1 generates the
 * actual OTel resource). Not family-scoped: every record family carries
 * these. */
const RESOURCE_ATTRIBUTES: readonly AttributeSpec[] = [
  stringAttribute(ATTR_SERVICE_NAME, true),
  stringAttribute(ATTR_SERVICE_VERSION, true),
  stringAttribute(ATTR_SERVICE_INSTANCE_ID, true),
  stringAttribute(ATTR_DEPLOYMENT_ENVIRONMENT_NAME, true),
  stringAttribute(ATTR_OS_TYPE, true),
  stringAttribute(ATTR_HOST_ARCH, true),
];

/** Tau-specific context attributes. Attached to spans and logs when
 * meaningful; never to metric dimensions, per the design principles'
 * cardinality rule. */
const CONTEXT_ATTRIBUTES: readonly AttributeSpec[] = [
  stringAttribute('tau.project.id', false),
  stringAttribute('tau.session.id', false),
  stringAttribute('tau.controller.id', false),
  stringAttribute('tau.runtime.id', false),
  { key: 'pi.generation', kind: 'int', maxLen: null, metricSafe: false },
];

/** Categorical values for `ui.action`'s `tau.action.name`: the semantic user
 * actions Stage 3 instruments in `useTau.ts`. */
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

/** Every ordinary Tauri command routed through the shared invoke wrapper.
 * `send_pi` (covered by `pi.rpc` spans) and `ingest_telemetry` (which must
 * never trace itself) are deliberately absent. */
const TAURI_INVOKE_COMMANDS = [
  'load_workspace',
  'import_project',
  'import_remote_project',
  'remove_project',
  'set_active_project',
  'set_project_collapsed',
  'reorder_projects',
  'set_active_session',
  'archive_session',
  'register_session',
  'probe_remote_project',
  'list_remote_directories',
  'read_model_scope',
  'read_remote_model_scope',
  'start_pi',
  'start_pi_remote',
  'stop_pi',
] as const;
type TauriInvokeCommand = (typeof TAURI_INVOKE_COMMANDS)[number];

const TAURI_INVOKE: RecordFamily = {
  name: 'tauri.invoke',
  attributes: [stringAttribute('tau.invoke.command', true)],
};

/** Categorical values for `pi.rpc.method`: every request `type` Tau sends
 * through `send_pi` (see `src/lib/pi/runtime.ts`). */
const PI_RPC_METHODS = [
  'get_state',
  'get_available_models',
  'get_commands',
  'get_available_thinking_levels',
  'get_messages',
  'set_model',
  'set_thinking_level',
  'set_session_name',
  'prompt',
  'abort',
  'extension_ui_response',
] as const;
type PiRpcMethod = (typeof PI_RPC_METHODS)[number];

/** Categorical values for `pi.rpc.outcome`: how a Pi RPC span ended. */
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

/** Per-Pi-run aggregate streaming counts (`src/lib/pi/runtime.ts`). Never
 * one record per delta or token: this family carries only bounded totals. */
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

const CONTROLLER_LIFECYCLE: RecordFamily = {
  name: 'controller.lifecycle',
  attributes: [
    stringAttribute('tau.controller.state.before', true),
    stringAttribute('tau.controller.state.after', true),
    stringAttribute('tau.controller.transition.cause', true),
  ],
};

const PI_PROCESS_STOP_REASONS = ['explicit_stop', 'replaced'] as const;
const PI_PROCESS_RESOLUTIONS = ['found', 'not_found'] as const;

const PI_PROCESS_LIFECYCLE: RecordFamily = {
  name: 'pi.process.lifecycle',
  attributes: [
    stringAttribute('tau.process.resolution', true),
    stringAttribute('tau.process.stop_reason', true),
    {
      key: 'tau.process.exit_code',
      kind: 'int',
      maxLen: null,
      metricSafe: true,
    },
  ],
};

/** `app.started`/`app.exited` lifecycle logs. They carry only the resource
 * and, where applicable, context attributes, so this family adds none. */
const APP_LIFECYCLE: RecordFamily = { name: 'app.lifecycle', attributes: [] };

const FAMILIES: readonly RecordFamily[] = [
  UI_ACTION,
  TAURI_INVOKE,
  PI_RPC,
  PI_STREAM,
  CONTROLLER_LIFECYCLE,
  PI_PROCESS_LIFECYCLE,
  APP_LIFECYCLE,
];

function findFamily(name: string): RecordFamily | undefined {
  return FAMILIES.find((family) => family.name === name);
}

/** Resolves `key` against `family`'s own attributes and the shared context
 * attributes, in that order. */
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

/** Whether `key` is safe to use as a metric dimension. Unknown keys are not
 * safe: only cataloged, explicitly-reviewed attributes may reach a metric. */
function isMetricSafe(key: string): boolean {
  const catalog = [
    ...RESOURCE_ATTRIBUTES,
    ...CONTEXT_ATTRIBUTES,
    ...FAMILIES.flatMap((family) => family.attributes),
  ];
  return catalog.find((spec) => spec.key === key)?.metricSafe ?? false;
}

/** The reviewed value set for a categorical string attribute, if `key` is
 * one. Centralizing the key-to-enum mapping here means a new categorical
 * attribute only needs an entry here, not a bespoke branch in
 * `validateAttribute`. */
function categoricalValues(key: string): readonly string[] | undefined {
  switch (key) {
    case 'tau.action.name':
      return UI_ACTION_NAMES;
    case 'tau.invoke.command':
      return TAURI_INVOKE_COMMANDS;
    case 'pi.rpc.method':
      return PI_RPC_METHODS;
    case 'pi.rpc.outcome':
      return PI_RPC_OUTCOMES;
    case 'tau.process.stop_reason':
      return PI_PROCESS_STOP_REASONS;
    case 'tau.process.resolution':
      return PI_PROCESS_RESOLUTIONS;
    default:
      return undefined;
  }
}

/** Validates `value` for `key` within `family`: the key must be allowlisted,
 * its runtime type must match the spec, and string values must fit the
 * spec's maximum length. This is the enforcement point that keeps telemetry
 * callers from recording arbitrary attributes. */
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
  return { valid: true };
}

export type {
  AttributeError,
  AttributeKind,
  AttributeSpec,
  PiRpcMethod,
  PiRpcOutcome,
  RecordFamily,
  TauriInvokeCommand,
  UiActionName,
};

export {
  allowedAttribute,
  APP_LIFECYCLE,
  CONTEXT_ATTRIBUTES,
  CONTROLLER_LIFECYCLE,
  FAMILIES,
  findFamily,
  isMetricSafe,
  PI_PROCESS_LIFECYCLE,
  PI_PROCESS_RESOLUTIONS,
  PI_PROCESS_STOP_REASONS,
  PI_RPC,
  PI_RPC_METHODS,
  PI_RPC_OUTCOMES,
  PI_STREAM,
  RESOURCE_ATTRIBUTES,
  TAURI_INVOKE,
  TAURI_INVOKE_COMMANDS,
  UI_ACTION,
  UI_ACTION_NAMES,
  validateAttribute,
};
