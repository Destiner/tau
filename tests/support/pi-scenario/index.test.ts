import { describe, expect, it } from 'vitest';

import savedSessionStreamThenStaleGeneration from './saved-session-stream-then-stale-generation';

import {
  PiScenarioEngine,
  definePiScenario,
  type ResolvedPiOutput,
} from './index';

function takeRequiredOutput(engine: PiScenarioEngine): ResolvedPiOutput {
  const output = engine.takeOutput();
  if (!output) throw new Error('Expected a scripted output');
  return output;
}

function consumeRequest(
  engine: PiScenarioEngine,
  id: string,
  type: string,
  fields: Record<string, unknown> = {},
): void {
  engine.consumeRequest('runtime-dynamic-47', { id, type, ...fields });
}

describe('PiScenarioEngine', () => {
  it('runs the complete bootstrap, prompt, stream, settlement, and stale event exchange in memory', () => {
    const engine = new PiScenarioEngine(savedSessionStreamThenStaleGeneration);

    expect(engine.bindRuntime('main', 'runtime-dynamic-47')).toBe(2);
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'runtime-event',
      runtime: { key: 'main', id: 'runtime-dynamic-47', generation: 2 },
      value: { kind: 'started' },
    });

    consumeRequest(engine, 'models-901', 'get_available_models');
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'response',
      value: {
        id: 'models-901',
        command: 'get_available_models',
        success: true,
      },
    });
    consumeRequest(engine, 'commands-332', 'get_commands');
    takeRequiredOutput(engine);
    consumeRequest(engine, 'state-843', 'get_state');
    takeRequiredOutput(engine);
    consumeRequest(engine, 'efforts-177', 'get_available_thinking_levels');
    takeRequiredOutput(engine);
    consumeRequest(engine, 'messages-625', 'get_messages');
    takeRequiredOutput(engine);

    consumeRequest(engine, 'prompt-518', 'prompt', {
      message: 'Explain the fixture',
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      value: { type: 'agent_start' },
    });
    consumeRequest(engine, 'run-state-204', 'get_state');
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'response',
      value: { id: 'run-state-204', data: { isStreaming: true } },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'Deterministic ' },
      },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'reply.' },
      },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'response',
      value: { id: 'prompt-518', command: 'prompt' },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      value: { type: 'agent_settled' },
    });

    consumeRequest(engine, 'settled-state-736', 'get_state');
    takeRequiredOutput(engine);
    consumeRequest(
      engine,
      'settled-efforts-429',
      'get_available_thinking_levels',
    );
    takeRequiredOutput(engine);
    consumeRequest(engine, 'settled-messages-110', 'get_messages');
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'response',
      value: {
        id: 'settled-messages-110',
        data: {
          messages: [
            { role: 'user', content: 'Explain the fixture' },
            { role: 'assistant' },
          ],
        },
      },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      runtime: { key: 'main', id: 'runtime-dynamic-47', generation: 1 },
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'STALE_GENERATION_SENTINEL' },
      },
    });

    expect(engine.takeOutput()).toBeUndefined();
    expect(() => engine.verifyComplete()).not.toThrow();
    expect(engine.timeline().map((entry) => entry.sequence)).toEqual(
      Array.from({ length: 27 }, (_, index) => index + 1),
    );
  });

  it('captures dynamic request ids and correlates a delayed response to the right request', () => {
    const engine = new PiScenarioEngine(
      definePiScenario({
        metadata: {
          name: 'correlation',
          purpose: 'Prove response ids are captured rather than scripted.',
          qualityRule: 'State correctness',
          schemaVersion: 1,
        },
        runtimes: [{ key: 'main', generation: 4 }],
        steps: [
          {
            kind: 'request',
            runtime: 'main',
            capture: 'prompt',
            match: { type: 'prompt', message: 'Hello' },
          },
          { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
          {
            kind: 'request',
            runtime: 'main',
            capture: 'state',
            match: { type: 'get_state' },
          },
          { kind: 'response', request: 'prompt', command: 'prompt' },
        ],
      }),
    );
    engine.bindRuntime('main', 'runtime-dynamic-47');

    consumeRequest(engine, 'generated-prompt-id', 'prompt', {
      message: 'Hello',
      ignoredTraceField: 'changes-per-run',
    });
    takeRequiredOutput(engine);
    consumeRequest(engine, 'generated-state-id', 'get_state');

    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'response',
      runtime: { generation: 4 },
      value: {
        id: 'generated-prompt-id',
        command: 'prompt',
        success: true,
      },
    });
    engine.verifyComplete();
  });

  it('rejects a reused request id within one runtime but allows it across runtimes', () => {
    const engine = new PiScenarioEngine(
      definePiScenario({
        metadata: {
          name: 'request-id-reuse',
          purpose: 'Reject duplicate request consumption within a runtime.',
          qualityRule: 'Diagnostics',
          schemaVersion: 1,
        },
        runtimes: [
          { key: 'main', generation: 2 },
          { key: 'background', generation: 1 },
        ],
        steps: [
          {
            kind: 'request',
            runtime: 'main',
            capture: 'main-models',
            match: { type: 'get_available_models' },
          },
          {
            kind: 'request',
            runtime: 'background',
            capture: 'background-models',
            match: { type: 'get_available_models' },
          },
          {
            kind: 'request',
            runtime: 'main',
            capture: 'main-state',
            match: { type: 'get_state' },
          },
        ],
      }),
    );
    engine.bindRuntime('main', 'runtime-dynamic-47');
    engine.bindRuntime('background', 'runtime-dynamic-82');

    engine.consumeRequest('runtime-dynamic-47', {
      id: 'shared-request-id',
      type: 'get_available_models',
    });
    engine.consumeRequest('runtime-dynamic-82', {
      id: 'shared-request-id',
      type: 'get_available_models',
    });

    expect(() =>
      engine.consumeRequest('runtime-dynamic-47', {
        id: 'shared-request-id',
        type: 'get_state',
      }),
    ).toThrowError(
      /request-id-reuse: Pi request id "shared-request-id" for runtime main was already consumed as main-models; it cannot also satisfy main-state\.[\s\S]*Timeline:[\s\S]*"capture":"main-models"[\s\S]*"capture":"background-models"/,
    );
  });

  it('rejects an unexpected request immediately with meaningful diagnostics', () => {
    const engine = new PiScenarioEngine(
      definePiScenario({
        metadata: {
          name: 'unexpected-request',
          purpose: 'Exercise request mismatch diagnostics.',
          qualityRule: 'Diagnostics',
          schemaVersion: 1,
        },
        runtimes: [{ key: 'main', generation: 2 }],
        steps: [
          {
            kind: 'request',
            runtime: 'main',
            capture: 'prompt',
            match: { type: 'prompt', message: 'Expected prompt' },
          },
        ],
      }),
    );
    engine.bindRuntime('main', 'runtime-dynamic-47');

    expect(() =>
      consumeRequest(engine, 'unpredictable-id', 'prompt', {
        message: 'Wrong prompt',
      }),
    ).toThrowError(
      /Unexpected Pi request[\s\S]*Expected prompt[\s\S]*Wrong prompt[\s\S]*message: expected "Expected prompt", received "Wrong prompt"[\s\S]*Timeline:/,
    );
  });

  it('reports both unconsumed expectations and unfinished outputs', () => {
    const engine = new PiScenarioEngine(
      definePiScenario({
        metadata: {
          name: 'unfinished',
          purpose: 'Exercise final verification.',
          qualityRule: 'Diagnostics',
          schemaVersion: 1,
        },
        runtimes: [{ key: 'main', generation: 2 }],
        steps: [
          {
            kind: 'request',
            runtime: 'main',
            capture: 'models',
            match: { type: 'get_available_models' },
          },
          {
            kind: 'response',
            request: 'models',
            command: 'get_available_models',
            data: { models: [] },
          },
          {
            kind: 'request',
            runtime: 'main',
            capture: 'state',
            match: { type: 'get_state' },
          },
        ],
      }),
    );
    engine.bindRuntime('main', 'runtime-dynamic-47');
    consumeRequest(engine, 'models-id', 'get_available_models');

    expect(() => engine.verifyComplete()).toThrowError(
      /Unconsumed expectations:[\s\S]*main.state[\s\S]*Unfinished outputs:[\s\S]*response get_available_models -> \$models[\s\S]*Timeline:/,
    );
  });
});
