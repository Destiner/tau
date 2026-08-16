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

type AttributeError = 'unknown-attribute' | 'wrong-type' | 'too-long';

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

const UI_ACTION: RecordFamily = {
  name: 'ui.action',
  attributes: [stringAttribute('tau.action.name', true)],
};

const TAURI_INVOKE: RecordFamily = {
  name: 'tauri.invoke',
  attributes: [stringAttribute('tau.invoke.command', true)],
};

const PI_RPC: RecordFamily = {
  name: 'pi.rpc',
  attributes: [
    stringAttribute('pi.rpc.method', true),
    stringAttribute('pi.rpc.request_id', false),
    stringAttribute('pi.rpc.outcome', true),
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

const PI_PROCESS_LIFECYCLE: RecordFamily = {
  name: 'pi.process.lifecycle',
  attributes: [
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
    return { valid: true };
  }

  if (typeof value !== 'number' || !Number.isInteger(value)) {
    return { valid: false, error: 'wrong-type' };
  }
  return { valid: true };
}

export type { AttributeError, AttributeKind, AttributeSpec, RecordFamily };

export {
  allowedAttribute,
  APP_LIFECYCLE,
  CONTEXT_ATTRIBUTES,
  CONTROLLER_LIFECYCLE,
  FAMILIES,
  findFamily,
  isMetricSafe,
  PI_PROCESS_LIFECYCLE,
  PI_RPC,
  RESOURCE_ATTRIBUTES,
  TAURI_INVOKE,
  UI_ACTION,
  validateAttribute,
};
