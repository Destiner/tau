type PiReadMethod =
  | 'get_state'
  | 'get_available_models'
  | 'get_commands'
  | 'get_available_thinking_levels'
  | 'get_messages';

type PiRequestMatcher =
  | { type: PiReadMethod | 'abort' }
  | { type: 'set_model'; provider: string; modelId: string }
  | { type: 'set_thinking_level'; level: string }
  | { type: 'set_session_name'; name: string }
  | { type: 'prompt'; message: string }
  | {
      type: 'extension_ui_response';
      variant: 'value';
      value: string;
    }
  | {
      type: 'extension_ui_response';
      variant: 'confirmed';
      confirmed: boolean;
    }
  | {
      type: 'extension_ui_response';
      variant: 'cancelled';
      cancelled: true;
    };

type PiRpcMethod = PiRequestMatcher['type'];

interface PiScenarioMetadata {
  name: string;
  purpose: string;
  qualityRule: string;
  schemaVersion: 1;
  origin?: string;
}

interface PiScenarioRuntime {
  key: string;
  generation: number;
}

interface ExpectedPiRequest {
  kind: 'request';
  runtime: string;
  capture: string;
  match: PiRequestMatcher;
}

interface PiModel {
  provider: string;
  id: string;
  name: string;
  reasoning: boolean;
}

interface PiCommand {
  name: string;
  description?: string;
  source?: string;
}

interface PiState {
  sessionId: string;
  sessionFile: string;
  sessionName: string;
  model: {
    provider: string;
    id: string;
    name: string;
  };
  thinkingLevel: string;
  isStreaming: boolean;
}

type PiMessage =
  | { role: 'user'; content: string }
  | {
      role: 'assistant';
      content: readonly { type: 'text'; text: string }[];
    };

type ScriptedPiResponse =
  | {
      kind: 'response';
      request: string;
      command: 'get_available_models';
      data: { models: readonly PiModel[] };
    }
  | {
      kind: 'response';
      request: string;
      command: 'get_commands';
      data: { commands: readonly PiCommand[] };
    }
  | {
      kind: 'response';
      request: string;
      command: 'get_state';
      data: PiState;
    }
  | {
      kind: 'response';
      request: string;
      command: 'get_available_thinking_levels';
      data: { levels: readonly string[] };
    }
  | {
      kind: 'response';
      request: string;
      command: 'get_messages';
      data: { messages: readonly PiMessage[] };
    }
  | {
      kind: 'response';
      request: string;
      command: 'prompt';
    };

type PiRpcEvent =
  | { type: 'agent_start' }
  | {
      type: 'message_update';
      assistantMessageEvent: { type: 'text_delta'; delta: string };
    }
  | { type: 'agent_settled' }
  | {
      type: 'extension_ui_request';
      id: string;
      method: 'select' | 'confirm' | 'input' | 'editor';
      title: string;
      message?: string;
      options?: readonly string[];
      placeholder?: string;
      prefill?: string;
      /** How long the request stands before Pi stops waiting for an answer. */
      timeout?: number;
    };

interface ScriptedPiEvent {
  kind: 'event';
  runtime: string;
  generation?: number;
  event: PiRpcEvent;
}

type ScriptedRuntimeEvent =
  | {
      kind: 'runtime-event';
      runtime: string;
      generation?: number;
      event: 'started';
    }
  | {
      kind: 'runtime-event';
      runtime: string;
      generation?: number;
      event: 'stderr' | 'error';
      message: string;
    }
  | {
      kind: 'runtime-event';
      runtime: string;
      generation?: number;
      event: 'exited';
      code?: number;
      message?: string;
    };

type ScriptedPiOutput =
  ScriptedPiResponse | ScriptedPiEvent | ScriptedRuntimeEvent;

interface PiScenarioGate {
  kind: 'gate';
  name: string;
  required: true;
}

type PiScenarioStep = ExpectedPiRequest | ScriptedPiOutput | PiScenarioGate;

interface PiScenario {
  metadata: PiScenarioMetadata;
  runtimes: readonly PiScenarioRuntime[];
  steps: readonly PiScenarioStep[];
}

interface ResolvedRuntime {
  key: string;
  id: string;
  generation: number;
}

type PiResponseData =
  | PiState
  | { models: readonly PiModel[] }
  | { commands: readonly PiCommand[] }
  | { levels: readonly string[] }
  | { messages: readonly PiMessage[] };

interface ResolvedPiResponse {
  kind: 'response';
  runtime: ResolvedRuntime;
  value: {
    type: 'response';
    id: string;
    command: ScriptedPiResponse['command'];
    success: true;
    data?: PiResponseData;
  };
}

interface ResolvedPiEvent {
  kind: 'event';
  runtime: ResolvedRuntime;
  value: PiRpcEvent;
}

interface ResolvedRuntimeEvent {
  kind: 'runtime-event';
  runtime: ResolvedRuntime;
  value:
    | { kind: 'started' }
    | { kind: 'stderr' | 'error'; message: string }
    | { kind: 'exited'; code?: number; message?: string };
}

type ResolvedPiOutput =
  ResolvedPiResponse | ResolvedPiEvent | ResolvedRuntimeEvent;

interface PiScenarioGateState {
  name: string;
  required: true;
  reached: boolean;
  released: boolean;
}

type PiScenarioTimelineEntry =
  | {
      sequence: number;
      kind: 'runtime-bound';
      runtime: string;
      generation: number;
    }
  | {
      sequence: number;
      kind: 'request';
      runtime: string;
      generation: number;
      capture: string;
      request: PiRequestMatcher;
    }
  | {
      sequence: number;
      kind: 'output';
      runtime: string;
      generation: number;
      output: string;
    }
  | {
      sequence: number;
      kind: 'gate-reached' | 'gate-released';
      gate: string;
    };

type TimelineEntryWithoutSequence = PiScenarioTimelineEntry extends infer Entry
  ? Entry extends { sequence: number }
    ? Omit<Entry, 'sequence'>
    : never
  : never;

interface CapturedRequest {
  id: string;
  method: PiRpcMethod;
  runtime: string;
}

interface RuntimeBinding {
  id: string;
  definition: PiScenarioRuntime;
}

interface MutableGateState extends PiScenarioGateState {
  waiters: Array<() => void>;
}

const RPC_METHODS = new Set<PiRpcMethod>([
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
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function stableValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(stableValue);
  if (!isRecord(value)) return value;
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, stableValue(value[key])]),
  );
}

function stableJson(value: unknown): string {
  return JSON.stringify(stableValue(value));
}

function isPiRpcMethod(value: unknown): value is PiRpcMethod {
  return typeof value === 'string' && RPC_METHODS.has(value as PiRpcMethod);
}

function requiredString(
  request: Record<string, unknown>,
  field: string,
): string {
  const value = request[field];
  if (typeof value !== 'string') {
    throw new Error(`Pi request field ${field} must be a string.`);
  }
  return value;
}

function matcherFromRequest(
  request: Record<string, unknown>,
): PiRequestMatcher {
  const type = request.type;
  if (!isPiRpcMethod(type)) {
    throw new Error(`Unknown Pi RPC method ${stableJson(type)}.`);
  }

  if (
    type === 'get_state' ||
    type === 'get_available_models' ||
    type === 'get_commands' ||
    type === 'get_available_thinking_levels' ||
    type === 'get_messages' ||
    type === 'abort'
  ) {
    return { type };
  }
  if (type === 'set_model') {
    return {
      type,
      provider: requiredString(request, 'provider'),
      modelId: requiredString(request, 'modelId'),
    };
  }
  if (type === 'set_thinking_level') {
    return { type, level: requiredString(request, 'level') };
  }
  if (type === 'set_session_name') {
    return { type, name: requiredString(request, 'name') };
  }
  if (type === 'prompt') {
    return { type, message: requiredString(request, 'message') };
  }

  const variants = ['value', 'confirmed', 'cancelled'].filter(
    (field) => request[field] !== undefined,
  );
  if (variants.length !== 1) {
    throw new Error(
      'extension_ui_response must contain exactly one response variant.',
    );
  }
  if (variants[0] === 'value') {
    return {
      type,
      variant: 'value',
      value: requiredString(request, 'value'),
    };
  }
  if (variants[0] === 'confirmed') {
    if (typeof request.confirmed !== 'boolean') {
      throw new Error('Pi request field confirmed must be a boolean.');
    }
    return { type, variant: 'confirmed', confirmed: request.confirmed };
  }
  if (request.cancelled !== true) {
    throw new Error('Pi request field cancelled must be true.');
  }
  return { type, variant: 'cancelled', cancelled: true };
}

function meaningfulDiff(
  expected: PiRequestMatcher,
  actual: PiRequestMatcher,
): string[] {
  const expectedRecord = expected as Record<string, unknown>;
  const actualRecord = actual as Record<string, unknown>;
  const fields = [
    ...new Set([...Object.keys(expectedRecord), ...Object.keys(actualRecord)]),
  ]
    .filter((field) => expectedRecord[field] !== actualRecord[field])
    .sort();
  return fields.map(
    (field) =>
      `${field}: expected ${stableJson(expectedRecord[field])}, received ${stableJson(actualRecord[field])}`,
  );
}

class PiScenarioEngine {
  readonly #scenario: PiScenario;
  readonly #runtimeDefinitions: Map<string, PiScenarioRuntime>;
  readonly #bindingsByKey = new Map<string, RuntimeBinding>();
  readonly #runtimeKeyById = new Map<string, string>();
  readonly #captures = new Map<string, CapturedRequest>();
  readonly #consumedRequestIds = new Map<string, Map<string, string>>();
  readonly #timeline: PiScenarioTimelineEntry[] = [];
  readonly #gates = new Map<string, MutableGateState>();
  #stepIndex = 0;

  constructor(scenario: PiScenario) {
    this.#scenario = scenario;
    this.#runtimeDefinitions = new Map(
      scenario.runtimes.map((runtime) => [runtime.key, runtime]),
    );
    this.#validateScenario();
  }

  bindRuntime(runtimeKey: string, runtimeId: string): number {
    const definition = this.#runtimeDefinitions.get(runtimeKey);
    if (!definition) throw new Error(`Unknown scenario runtime ${runtimeKey}.`);
    if (!runtimeId) throw new Error('Runtime id must not be empty.');
    const existingKey = this.#runtimeKeyById.get(runtimeId);
    if (existingKey && existingKey !== runtimeKey) {
      throw new Error(
        `Runtime id ${runtimeId} is already active as ${existingKey}.`,
      );
    }
    const existingBinding = this.#bindingsByKey.get(runtimeKey);
    if (existingBinding && existingBinding.id !== runtimeId) {
      throw new Error(`Scenario runtime ${runtimeKey} is already bound.`);
    }
    if (!existingBinding) {
      this.#bindingsByKey.set(runtimeKey, { id: runtimeId, definition });
      this.#runtimeKeyById.set(runtimeId, runtimeKey);
      this.#record({
        kind: 'runtime-bound',
        runtime: runtimeKey,
        generation: definition.generation,
      });
    }
    return definition.generation;
  }

  consumeRequest(runtimeId: string, value: unknown): void {
    const runtimeKey = this.#runtimeKeyById.get(runtimeId);
    const next = this.#scenario.steps[this.#stepIndex];
    let actual: PiRequestMatcher;
    let requestId: string;
    try {
      if (!isRecord(value)) throw new Error('Pi request must be an object.');
      requestId = typeof value.id === 'string' ? value.id : '';
      if (!requestId) throw new Error('Pi request id must not be empty.');
      actual = matcherFromRequest(value);
    } catch (error) {
      throw this.#failure(
        error instanceof Error ? error.message : 'Invalid Pi request.',
      );
    }

    if (!runtimeKey || !next || next.kind !== 'request') {
      throw this.#unexpectedRequest(runtimeId, runtimeKey, actual, next);
    }
    const consumedCapture = this.#consumedRequestIds
      .get(runtimeKey)
      ?.get(requestId);
    if (consumedCapture) {
      throw this.#failure(
        `Pi request id ${stableJson(requestId)} for runtime ${runtimeKey} was already consumed as ${consumedCapture}; it cannot also satisfy ${next.capture}.`,
      );
    }
    const diff = meaningfulDiff(next.match, actual);
    if (next.runtime !== runtimeKey || diff.length > 0) {
      throw this.#unexpectedRequest(runtimeId, runtimeKey, actual, next, diff);
    }
    const binding = this.#bindingsByKey.get(runtimeKey);
    if (!binding) throw this.#failure(`Runtime ${runtimeKey} is not bound.`);
    this.#captures.set(next.capture, {
      id: requestId,
      method: actual.type,
      runtime: runtimeKey,
    });
    const consumedIds =
      this.#consumedRequestIds.get(runtimeKey) ?? new Map<string, string>();
    consumedIds.set(requestId, next.capture);
    this.#consumedRequestIds.set(runtimeKey, consumedIds);
    this.#stepIndex += 1;
    this.#record({
      kind: 'request',
      runtime: runtimeKey,
      generation: binding.definition.generation,
      capture: next.capture,
      request: actual,
    });
  }

  takeOutput(): ResolvedPiOutput | undefined {
    let next = this.#scenario.steps[this.#stepIndex];
    if (!next || next.kind === 'request') return undefined;
    if (next.kind === 'gate') {
      const gate = this.#requiredGate(next.name);
      if (!gate.reached) {
        gate.reached = true;
        this.#record({ kind: 'gate-reached', gate: gate.name });
        for (const resolve of gate.waiters.splice(0)) resolve();
      }
      if (!gate.released) return undefined;
      this.#stepIndex += 1;
      next = this.#scenario.steps[this.#stepIndex];
      if (!next || next.kind === 'request' || next.kind === 'gate') {
        return this.takeOutput();
      }
    }

    if (next.kind !== 'response' && !this.#bindingsByKey.has(next.runtime)) {
      return undefined;
    }

    const resolved = this.#resolveOutput(next);
    this.#stepIndex += 1;
    this.#record({
      kind: 'output',
      runtime: resolved.runtime.key,
      generation: resolved.runtime.generation,
      output: this.#outputLabel(next),
    });
    if (resolved.kind === 'runtime-event' && resolved.value.kind === 'exited') {
      this.#runtimeKeyById.delete(resolved.runtime.id);
    }
    return resolved;
  }

  waitForGateReached(name: string): Promise<void> {
    const gate = this.#requiredGate(name);
    if (gate.reached) return Promise.resolve();
    return new Promise((resolve) => gate.waiters.push(resolve));
  }

  releaseGate(name: string): void {
    const gate = this.#requiredGate(name);
    if (!gate.reached) {
      throw this.#failure(
        `Gate ${stableJson(name)} was released before it was reached.`,
      );
    }
    if (gate.released) {
      throw this.#failure(
        `Gate ${stableJson(name)} was released more than once.`,
      );
    }
    gate.released = true;
    this.#record({ kind: 'gate-released', gate: name });
  }

  gates(): readonly PiScenarioGateState[] {
    return [...this.#gates.values()].map((gate) => ({
      name: gate.name,
      required: gate.required,
      reached: gate.reached,
      released: gate.released,
    }));
  }

  timeline(): readonly PiScenarioTimelineEntry[] {
    return this.#timeline.map((entry) => structuredClone(entry));
  }

  isComplete(): boolean {
    return (
      this.#stepIndex === this.#scenario.steps.length &&
      this.gates().every(
        (gate) => !gate.required || (gate.reached && gate.released),
      )
    );
  }

  verifyComplete(): void {
    const unfinishedGates = this.gates().filter(
      (gate) => gate.required && (!gate.reached || !gate.released),
    );
    if (this.isComplete()) return;
    const remaining = this.#scenario.steps.slice(this.#stepIndex);
    const requests = remaining
      .filter((step): step is ExpectedPiRequest => step.kind === 'request')
      .map(
        (step) => `${step.runtime}.${step.capture} ${stableJson(step.match)}`,
      );
    const outputs = remaining
      .filter(
        (step): step is ScriptedPiOutput =>
          step.kind !== 'request' && step.kind !== 'gate',
      )
      .map((step) => this.#outputLabel(step));
    const details = [
      unfinishedGates.length > 0
        ? `Unfinished required gates:\n${unfinishedGates
            .map(
              (gate) =>
                `- ${gate.name}: ${gate.reached ? 'reached' : 'never reached'}, ${gate.released ? 'released' : 'unreleased'}`,
            )
            .join('\n')}\nGate states: ${stableJson(this.gates())}`
        : '',
      requests.length > 0
        ? `Unconsumed expectations:\n${requests.map((item) => `- ${item}`).join('\n')}`
        : '',
      outputs.length > 0
        ? `Unfinished outputs:\n${outputs.map((item) => `- ${item}`).join('\n')}`
        : '',
    ]
      .filter(Boolean)
      .join('\n');
    throw this.#failure(`Scenario is incomplete.\n${details}`);
  }

  #validateScenario(): void {
    if (this.#runtimeDefinitions.size !== this.#scenario.runtimes.length) {
      throw new Error('Scenario runtime keys must be unique.');
    }
    for (const runtime of this.#scenario.runtimes) {
      if (
        !runtime.key ||
        !Number.isInteger(runtime.generation) ||
        runtime.generation <= 0
      ) {
        throw new Error(
          'Scenario runtimes need a key and positive generation.',
        );
      }
    }

    const captures = new Map<string, ExpectedPiRequest>();
    for (const step of this.#scenario.steps) {
      if (step.kind === 'gate') {
        if (!step.name || this.#gates.has(step.name)) {
          throw new Error(
            `Scenario gate ${stableJson(step.name)} must have a unique non-empty name.`,
          );
        }
        this.#gates.set(step.name, {
          name: step.name,
          required: step.required,
          reached: false,
          released: false,
          waiters: [],
        });
        continue;
      }
      if (step.kind === 'request') {
        if (!this.#runtimeDefinitions.has(step.runtime)) {
          throw new Error(
            `Request ${step.capture} uses unknown runtime ${step.runtime}.`,
          );
        }
        if (!step.capture || captures.has(step.capture)) {
          throw new Error(`Request capture ${step.capture} must be unique.`);
        }
        captures.set(step.capture, step);
        continue;
      }
      if (step.kind === 'response') {
        const request = captures.get(step.request);
        if (!request) {
          throw new Error(
            `Response references unavailable request ${step.request}.`,
          );
        }
        if (request.match.type !== step.command) {
          throw new Error(
            `Response for ${step.request} uses ${step.command}, expected ${request.match.type}.`,
          );
        }
        continue;
      }
      if (!this.#runtimeDefinitions.has(step.runtime)) {
        throw new Error(`Output uses unknown runtime ${step.runtime}.`);
      }
    }
  }

  #requiredGate(name: string): MutableGateState {
    const gate = this.#gates.get(name);
    if (!gate)
      throw this.#failure(`Unknown scenario gate ${stableJson(name)}.`);
    return gate;
  }

  #resolveOutput(output: ScriptedPiOutput): ResolvedPiOutput {
    if (output.kind === 'response') {
      const captured = this.#captures.get(output.request);
      if (!captured) {
        throw this.#failure(
          `Response request ${output.request} is not captured.`,
        );
      }
      const runtime = this.#resolvedRuntime(captured.runtime);
      return {
        kind: 'response',
        runtime,
        value: {
          type: 'response',
          id: captured.id,
          command: output.command,
          success: true,
          ...('data' in output ? { data: output.data } : {}),
        },
      };
    }

    const generation =
      output.generation ??
      this.#runtimeDefinitions.get(output.runtime)?.generation;
    const runtime = this.#resolvedRuntime(output.runtime, generation);
    if (output.kind === 'event') {
      return { kind: 'event', runtime, value: output.event };
    }
    if (output.event === 'stderr' || output.event === 'error') {
      return {
        kind: 'runtime-event',
        runtime,
        value: { kind: output.event, message: output.message },
      };
    }
    if (output.event === 'exited') {
      return {
        kind: 'runtime-event',
        runtime,
        value: {
          kind: output.event,
          ...('code' in output && output.code !== undefined
            ? { code: output.code }
            : {}),
          ...('message' in output && output.message !== undefined
            ? { message: output.message }
            : {}),
        },
      };
    }
    return { kind: 'runtime-event', runtime, value: { kind: output.event } };
  }

  #resolvedRuntime(runtimeKey: string, generation?: number): ResolvedRuntime {
    const binding = this.#bindingsByKey.get(runtimeKey);
    if (!binding) throw this.#failure(`Runtime ${runtimeKey} is not bound.`);
    return {
      key: runtimeKey,
      id: binding.id,
      generation: generation ?? binding.definition.generation,
    };
  }

  #unexpectedRequest(
    runtimeId: string,
    runtimeKey: string | undefined,
    actual: PiRequestMatcher,
    expected: PiScenarioStep | undefined,
    diff: readonly string[] = [],
  ): Error {
    const expectedValue =
      expected?.kind === 'request'
        ? { runtime: expected.runtime, request: expected.match }
        : expected?.kind === 'gate'
          ? { next: `gate ${expected.name}` }
          : expected
            ? { next: this.#outputLabel(expected) }
            : { next: 'end of scenario' };
    const actualValue = {
      runtime: runtimeKey ?? `unbound:${runtimeId}`,
      request: actual,
    };
    const runtimeDiff =
      expected?.kind === 'request' && expected.runtime !== runtimeKey
        ? [
            `runtime: expected ${expected.runtime}, received ${runtimeKey ?? runtimeId}`,
          ]
        : [];
    const differences = [...runtimeDiff, ...diff];
    return this.#failure(
      [
        'Unexpected Pi request.',
        `Expected: ${stableJson(expectedValue)}`,
        `Actual: ${stableJson(actualValue)}`,
        differences.length > 0
          ? `Diff:\n${differences.map((item) => `- ${item}`).join('\n')}`
          : '',
      ]
        .filter(Boolean)
        .join('\n'),
    );
  }

  #outputLabel(output: ScriptedPiOutput): string {
    if (output.kind === 'response') {
      return `response ${output.command} -> $${output.request}`;
    }
    if (output.kind === 'event') {
      return `event ${output.runtime}@${output.generation ?? 'current'} ${output.event.type}`;
    }
    return `runtime-event ${output.runtime}@${output.generation ?? 'current'} ${output.event}`;
  }

  #record(entry: TimelineEntryWithoutSequence): void {
    this.#timeline.push({
      sequence: this.#timeline.length + 1,
      ...entry,
    } as PiScenarioTimelineEntry);
  }

  #failure(message: string): Error {
    const timeline = this.#timeline
      .map(
        (entry) =>
          `${String(entry.sequence).padStart(3, '0')} ${stableJson(entry)}`,
      )
      .join('\n');
    return new Error(
      `${this.#scenario.metadata.name}: ${message}\nTimeline:${timeline ? `\n${timeline}` : ' (empty)'}`,
    );
  }
}

function definePiScenario<const Scenario extends PiScenario>(
  scenario: Scenario,
): Scenario {
  return scenario;
}

export {
  PiScenarioEngine,
  definePiScenario,
  type PiMessage,
  type PiRequestMatcher,
  type PiRpcEvent,
  type PiScenario,
  type PiScenarioGateState,
  type PiScenarioMetadata,
  type PiScenarioStep,
  type PiScenarioTimelineEntry,
  type PiState,
  type ResolvedPiOutput,
};
