export type Component = 'button' | 'input' | 'textarea';
export type Theme = 'light' | 'dark';
export type Props = {
  label: string;
  value: string;
  variant: string;
  size: string;
  placeholder: string;
  disabled: boolean;
  readonly: boolean;
  error: boolean;
  width: number;
  rows: number;
  monospaced: boolean;
};
export const defaultProps: Props = {
  label: 'Continue',
  value: '',
  variant: '',
  size: 'sm',
  placeholder: '',
  disabled: false,
  readonly: false,
  error: false,
  width: 360,
  rows: 4,
  monospaced: false,
};
export type Observation = {
  value: string;
  selection: [number, number] | null;
  focused: boolean;
  disabled: boolean;
  readonly: boolean;
  clicks: number;
  changes: number;
};
export type Step =
  | { action: 'focus' | 'blur' | 'click' | 'hover' | 'mouseDown' | 'mouseUp' }
  | { action: 'pointer'; phase: 'move' | 'down' | 'up'; x: number; y: number }
  | { action: 'key'; key: string }
  | { action: 'type'; text: string }
  | { action: 'select'; start: number; end: number }
  | { action: 'props'; props: Partial<Props> }
  | { action: 'checkpoint'; name: string; expected: Partial<Observation> };
export type Scenario = {
  id: string;
  component: Component;
  props: Partial<Props>;
  steps: Step[];
};
export type Suite = {
  schemaVersion: 1;
  theme: Theme;
  viewport: { width: 640; height: 320 };
  scale: 2;
  scenarios: Scenario[];
};
export type Bounds = { x: number; y: number; width: number; height: number };
export type Capture = {
  scenario: string;
  checkpoint: string;
  observation: Observation;
  bounds: Bounds;
  screenshot: string;
};

const propKeys = Object.keys(defaultProps);
const observationKeys = [
  'value',
  'selection',
  'focused',
  'disabled',
  'readonly',
  'clicks',
  'changes',
];
const actions = [
  'focus',
  'blur',
  'click',
  'hover',
  'mouseDown',
  'mouseUp',
  'pointer',
  'key',
  'type',
  'select',
  'props',
  'checkpoint',
];
function object(value: unknown, context: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new Error(`${context}: expected object`);
  return value as Record<string, unknown>;
}
function keys(
  value: Record<string, unknown>,
  allowed: string[],
  context: string,
): void {
  for (const key of Object.keys(value))
    if (!allowed.includes(key))
      throw new Error(`${context}: unknown key ${key}`);
}
function integer(value: unknown): boolean {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}
export function validateProps(value: unknown, context: string): void {
  const p = object(value, context);
  keys(p, propKeys, context);
  for (const [key, v] of Object.entries(p)) {
    if (
      typeof v !== typeof defaultProps[key as keyof Props] ||
      (typeof v === 'number' && !Number.isFinite(v))
    )
      throw new Error(`${context}: invalid ${key}`);
  }
  if (p.width !== undefined && (p.width as number) <= 0)
    throw new Error(`${context}: width must be positive`);
  if (p.rows !== undefined && (!integer(p.rows) || (p.rows as number) < 1))
    throw new Error(`${context}: rows must be a positive integer`);
}
export function validateSelection(
  text: string,
  start: number,
  end: number,
): void {
  const boundaries = new Set([0]);
  let offset = 0;
  for (const char of text) {
    offset += char.length;
    boundaries.add(offset);
  }
  if (start > end || !boundaries.has(start) || !boundaries.has(end))
    throw new Error('selection must use in-range UTF-16 scalar boundaries');
}
export function validateObservation(
  value: unknown,
  context: string,
  partial = false,
): asserts value is Observation {
  const o = object(value, context);
  keys(o, observationKeys, context);
  if (!partial && observationKeys.some((key) => !(key in o)))
    throw new Error(`${context}: incomplete observation`);
  for (const [key, v] of Object.entries(o)) {
    const valid =
      key === 'selection'
        ? v === null ||
          (Array.isArray(v) &&
            v.length === 2 &&
            v.every(integer) &&
            v[0] <= v[1])
        : key === 'value'
          ? typeof v === 'string'
          : key === 'clicks' || key === 'changes'
            ? integer(v)
            : typeof v === 'boolean';
    if (!valid) throw new Error(`${context}: invalid ${key}`);
  }
}
export function validateSuite(value: unknown): asserts value is Suite {
  const suite = object(value, 'suite');
  keys(
    suite,
    ['schemaVersion', 'theme', 'viewport', 'scale', 'scenarios'],
    'suite',
  );
  if (
    suite.schemaVersion !== 1 ||
    !['light', 'dark'].includes(String(suite.theme))
  )
    throw new Error('unsupported suite version/theme');
  const viewport = object(suite.viewport, 'viewport');
  keys(viewport, ['width', 'height'], 'viewport');
  if (viewport.width !== 640 || viewport.height !== 320)
    throw new Error('viewport must be 640x320');
  if (suite.scale !== 2) throw new Error('scale must be 2');
  if (!Array.isArray(suite.scenarios) || !suite.scenarios.length)
    throw new Error('empty scenarios');
  const ids = new Set<string>();
  for (const raw of suite.scenarios) {
    const s = object(raw, 'scenario');
    keys(s, ['id', 'component', 'props', 'steps'], 'scenario');
    if (typeof s.id !== 'string' || !/^[a-z0-9-]+$/.test(s.id) || ids.has(s.id))
      throw new Error(`duplicate/invalid scenario id: ${s.id}`);
    ids.add(s.id);
    if (!['button', 'input', 'textarea'].includes(String(s.component)))
      throw new Error(`${s.id}: unknown component`);
    validateProps(s.props, s.id);
    if (!Array.isArray(s.steps) || !s.steps.length)
      throw new Error(`${s.id}: empty steps`);
    const checkpoints = new Set<string>();
    for (const rawStep of s.steps) {
      const step = object(rawStep, s.id);
      const action = step.action;
      if (!actions.includes(String(action)))
        throw new Error(`${s.id}: unknown action ${action}`);
      const fields: Record<string, string[]> = {
        pointer: ['phase', 'x', 'y'],
        key: ['key'],
        type: ['text'],
        select: ['start', 'end'],
        props: ['props'],
        checkpoint: ['name', 'expected'],
      };
      keys(step, ['action', ...(fields[String(action)] || [])], s.id);
      if (
        ['key', 'type'].includes(String(action)) &&
        typeof step[action === 'key' ? 'key' : 'text'] !== 'string'
      )
        throw new Error(`${s.id}: invalid ${action}`);
      if (
        action === 'select' &&
        (!integer(step.start) ||
          !integer(step.end) ||
          (step.start as number) > (step.end as number))
      )
        throw new Error(`${s.id}: invalid selection`);
      if (
        action === 'pointer' &&
        (!['move', 'down', 'up'].includes(String(step.phase)) ||
          typeof step.x !== 'number' ||
          !Number.isFinite(step.x) ||
          step.x < 0 ||
          step.x >= 640 ||
          typeof step.y !== 'number' ||
          !Number.isFinite(step.y) ||
          step.y < 0 ||
          step.y >= 320)
      )
        throw new Error(`${s.id}: invalid pointer`);
      if (action === 'props') validateProps(step.props, s.id);
      if (action === 'checkpoint') {
        if (
          typeof step.name !== 'string' ||
          !/^[a-z0-9-]+$/.test(step.name) ||
          checkpoints.has(step.name)
        )
          throw new Error(`${s.id}: duplicate/invalid checkpoint`);
        checkpoints.add(step.name);
        validateObservation(step.expected, s.id, true);
        if (!Object.keys(step.expected as object).length)
          throw new Error(`${s.id}: empty expected observation`);
      }
    }
    if (!checkpoints.size) throw new Error(`${s.id}: no checkpoints`);
  }
}
export function assertCaptures(
  suite: Suite,
  captures: Capture[],
  label: string,
): Map<string, Capture> {
  const wanted = new Set(
    suite.scenarios.flatMap((s) =>
      s.steps
        .filter((x) => x.action === 'checkpoint')
        .map(
          (x) =>
            `${s.id}/${(x as Extract<Step, { action: 'checkpoint' }>).name}`,
        ),
    ),
  );
  const found = new Map<string, Capture>();
  if (!Array.isArray(captures))
    throw new Error(`${label}: expected capture array`);
  for (const c of captures) {
    const id = `${c.scenario}/${c.checkpoint}`;
    if (!wanted.has(id) || found.has(id))
      throw new Error(`${label}: unknown or duplicate capture ${id}`);
    validateObservation(c.observation, `${label}/${id}`);
    const b = object(c.bounds, `${label}/${id}/bounds`);
    keys(b, ['x', 'y', 'width', 'height'], label);
    if (
      !['x', 'y', 'width', 'height'].every(
        (k) => typeof b[k] === 'number' && Number.isFinite(b[k]),
      ) ||
      (b.width as number) <= 0 ||
      (b.height as number) <= 0 ||
      (b.x as number) < 0 ||
      (b.y as number) < 0 ||
      (b.x as number) + (b.width as number) > suite.viewport.width ||
      (b.y as number) + (b.height as number) > suite.viewport.height
    )
      throw new Error(`${label}/${id}: invalid bounds`);
    if (
      typeof c.screenshot !== 'string' ||
      !/^[a-z0-9-]+--[a-z0-9-]+\.png$/.test(c.screenshot) ||
      c.screenshot !== `${c.scenario}--${c.checkpoint}.png`
    )
      throw new Error(`${label}/${id}: invalid screenshot`);
    found.set(id, c);
  }
  for (const id of wanted)
    if (!found.has(id)) throw new Error(`${label}: missing capture ${id}`);
  return found;
}
export function stateDifferences(
  expected: Partial<Observation>,
  actual: Observation,
): string[] {
  return Object.entries(expected)
    .filter(
      ([k, v]) =>
        JSON.stringify(v) !== JSON.stringify(actual[k as keyof Observation]),
    )
    .map(
      ([k, v]) =>
        `${k}: expected ${JSON.stringify(v)}, got ${JSON.stringify(actual[k as keyof Observation])}`,
    );
}
