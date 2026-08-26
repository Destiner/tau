import { describe, expect, it } from 'vitest';

import phantomCommandRegistration from './phantom-command-registration';
import savedSessionCommandReplacement from './saved-session-command-replacement';
import {
  rawBridgeError,
  rawExitMessage,
  rawStderr,
  savedSessionBootstrapProcessExit,
} from './saved-session-process-failures';
import savedSessionStreamThenStaleGeneration, {
  savedSessionBootstrap,
  savedSessionConversation,
  savedSessionStaleGeneration,
} from './saved-session-stream-then-stale-generation';
import savedSessionUnacknowledgedAbort from './saved-session-unacknowledged-abort';

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

function gatedOutputScenario(): PiScenarioEngine {
  return new PiScenarioEngine(
    definePiScenario({
      metadata: {
        name: 'gated-output',
        purpose: 'Exercise explicit gate transitions.',
        qualityRule: 'Deterministic diagnostics',
        schemaVersion: 1,
      },
      runtimes: [{ key: 'main', generation: 1 }],
      steps: [
        { kind: 'gate', name: 'first', required: true },
        { kind: 'event', runtime: 'main', event: { type: 'agent_start' } },
        { kind: 'gate', name: 'second', required: true },
        { kind: 'event', runtime: 'main', event: { type: 'agent_settled' } },
      ],
    }),
  );
}

describe('PiScenarioEngine', () => {
  it('offers an exact bootstrap checkpoint without consuming prompt steps', () => {
    const engine = new PiScenarioEngine(savedSessionBootstrap);

    engine.bindRuntime('main', 'runtime-dynamic-47');
    takeRequiredOutput(engine);
    for (const [id, type] of [
      ['models', 'get_available_models'],
      ['commands', 'get_commands'],
      ['state', 'get_state'],
      ['efforts', 'get_available_thinking_levels'],
      ['messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }

    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.isComplete()).toBe(true);
    expect(() => engine.verifyComplete()).not.toThrow();
    expect(engine.timeline()).toHaveLength(12);
  });

  it('resolves production-shaped process outputs and rebinds a restarted runtime', () => {
    const engine = new PiScenarioEngine(savedSessionBootstrapProcessExit);
    expect(engine.bindRuntime('failed-bootstrap', 'runtime-dynamic-47')).toBe(
      2,
    );
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { kind: 'started' },
    });
    for (const type of [
      'get_available_models',
      'get_commands',
      'get_state',
    ] as const) {
      consumeRequest(engine, `failed-${type}`, type);
    }

    expect(engine.takeOutput()).toBeUndefined();
    engine.releaseGate('before-bootstrap-process-failure');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { kind: 'stderr', message: rawStderr },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { kind: 'error', message: rawBridgeError },
    });
    expect(engine.takeOutput()).toBeUndefined();
    engine.releaseGate('after-bootstrap-bridge-error');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { kind: 'exited', code: 47, message: rawExitMessage },
    });
    expect(engine.takeOutput()).toBeUndefined();
    engine.releaseGate('after-bootstrap-process-exit');
    expect(engine.takeOutput()).toBeUndefined();

    expect(engine.bindRuntime('recovered-main', 'runtime-dynamic-47')).toBe(3);
    expect(takeRequiredOutput(engine)).toMatchObject({
      runtime: { key: 'recovered-main', generation: 3 },
      value: { kind: 'started' },
    });
    for (const [id, type] of [
      ['models', 'get_available_models'],
      ['commands', 'get_commands'],
      ['state', 'get_state'],
      ['efforts', 'get_available_thinking_levels'],
      ['messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }

    expect(engine.timeline()).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          kind: 'output',
          output: 'runtime-event failed-bootstrap@current stderr',
        }),
        expect.objectContaining({
          kind: 'output',
          output: 'runtime-event failed-bootstrap@current exited',
        }),
      ]),
    );
    expect(JSON.stringify(engine.timeline())).not.toContain('RAW_');
    expect(() => engine.verifyComplete()).not.toThrow();
  });

  it('reports real transcript work after a phantom command identity sync', () => {
    const engine = new PiScenarioEngine(phantomCommandRegistration);
    engine.bindRuntime('main', 'runtime-dynamic-47');
    takeRequiredOutput(engine);
    for (const [id, type] of [
      ['models', 'get_available_models'],
      ['commands', 'get_commands'],
      ['state', 'get_state'],
      ['efforts', 'get_available_thinking_levels'],
      ['messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }

    engine.bindRuntime('phantom', 'runtime-dynamic-82');
    takeRequiredOutput(engine);
    for (const [id, type] of [
      ['phantom-models', 'get_available_models'],
      ['phantom-commands', 'get_commands'],
      ['phantom-state', 'get_state'],
      ['phantom-efforts', 'get_available_thinking_levels'],
      ['phantom-messages', 'get_messages'],
    ] as const) {
      engine.consumeRequest('runtime-dynamic-82', { id, type });
      takeRequiredOutput(engine);
    }
    engine.consumeRequest('runtime-dynamic-82', {
      id: 'command',
      type: 'prompt',
      message: '/mcp',
    });
    takeRequiredOutput(engine);
    engine.consumeRequest('runtime-dynamic-82', {
      id: 'command-sync',
      type: 'get_state',
    });

    expect(engine.takeOutput()).toBeUndefined();
    engine.releaseGate('before-streaming-command-sync');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        id: 'command-sync',
        data: {
          sessionId: 'session-mcp',
          sessionName: 'MCP workflow',
          isStreaming: true,
        },
      },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'Real agent work started.' },
      },
    });
    expect(() => engine.verifyComplete()).not.toThrow();
  });

  it('holds a command replacement identity until its immediate probe is gated', () => {
    const engine = new PiScenarioEngine(savedSessionCommandReplacement);
    engine.bindRuntime('main', 'runtime-dynamic-47');
    takeRequiredOutput(engine);

    for (const [id, type] of [
      ['models', 'get_available_models'],
      ['commands', 'get_commands'],
      ['state', 'get_state'],
      ['efforts', 'get_available_thinking_levels'],
      ['messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }

    consumeRequest(engine, 'command', 'prompt', { message: '/mock 42' });
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { id: 'command', command: 'prompt' },
    });
    consumeRequest(engine, 'identity', 'get_state');
    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.gates()).toEqual([
      {
        name: 'before-command-replacement-identity',
        required: true,
        reached: true,
        released: false,
      },
    ]);

    engine.releaseGate('before-command-replacement-identity');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        id: 'identity',
        command: 'get_state',
        data: {
          sessionId: 'session-plan-42',
          sessionFile: '/fixture/tau-project/session-plan-42.jsonl',
          sessionName: '42 • plan',
        },
      },
    });
    for (const [id, type] of [
      ['replacement-models', 'get_available_models'],
      ['replacement-commands', 'get_commands'],
      ['replacement-efforts', 'get_available_thinking_levels'],
      ['replacement-messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'Replacement session started.' },
      },
    });

    expect(() => engine.verifyComplete()).not.toThrow();
  });

  it('holds an unacknowledged abort until its timeout probe reports idle', () => {
    const engine = new PiScenarioEngine(savedSessionUnacknowledgedAbort);
    engine.bindRuntime('main', 'runtime-dynamic-47');
    takeRequiredOutput(engine);

    for (const [id, type] of [
      ['models', 'get_available_models'],
      ['commands', 'get_commands'],
      ['state', 'get_state'],
      ['efforts', 'get_available_thinking_levels'],
      ['messages', 'get_messages'],
    ] as const) {
      consumeRequest(engine, id, type);
      takeRequiredOutput(engine);
    }

    consumeRequest(engine, 'prompt', 'prompt', {
      message: 'Stop this fixture',
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { type: 'agent_start' },
    });
    consumeRequest(engine, 'run-state', 'get_state');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { data: { isStreaming: true } },
    });
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'Partial reply.' },
      },
    });

    consumeRequest(engine, 'abort', 'abort');
    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.gates()[0]).toMatchObject({
      name: 'abort-request-consumed',
      reached: true,
      released: false,
    });
    engine.releaseGate('abort-request-consumed');
    expect(engine.takeOutput()).toBeUndefined();

    consumeRequest(engine, 'abort-probe', 'get_state');
    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.gates()[1]).toMatchObject({
      name: 'before-abort-timeout-probe-response',
      reached: true,
      released: false,
    });
    engine.releaseGate('before-abort-timeout-probe-response');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { id: 'abort-probe', data: { isStreaming: false } },
    });
    consumeRequest(engine, 'abort-efforts', 'get_available_thinking_levels');
    takeRequiredOutput(engine);
    consumeRequest(engine, 'abort-messages', 'get_messages');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: {
        data: {
          messages: [
            { role: 'user', content: 'Stop this fixture' },
            { role: 'assistant', content: [{ text: 'Partial reply.' }] },
          ],
        },
      },
    });

    const serialized = JSON.stringify(savedSessionUnacknowledgedAbort.steps);
    expect(serialized).not.toContain('agent_settled');
    expect(serialized).not.toContain('"command":"prompt"');
    expect(
      savedSessionUnacknowledgedAbort.steps.filter(
        (step) => step.kind === 'request' && step.match.type === 'abort',
      ),
    ).toHaveLength(1);
    expect(() => engine.verifyComplete()).not.toThrow();
  });

  it('keeps the bootstrap and complete conversation checkpoints unchanged by the stale interleaving', () => {
    const sharedConversationSteps =
      savedSessionStreamThenStaleGeneration.steps.filter(
        (step) =>
          step.kind !== 'gate' &&
          !(
            step.kind === 'event' &&
            'generation' in step &&
            step.generation === 1
          ),
      );

    expect(savedSessionBootstrap.steps.at(-1)).toMatchObject({
      kind: 'response',
      command: 'get_messages',
      request: 'bootstrap-messages',
    });
    expect(savedSessionConversation.steps).toEqual(sharedConversationSteps);
    expect(savedSessionConversation.steps.at(-1)).toMatchObject({
      kind: 'response',
      command: 'get_messages',
      request: 'settled-messages',
    });
    expect(JSON.stringify(savedSessionConversation)).not.toContain(
      'STALE_GENERATION_SENTINEL',
    );
  });

  it('keeps the browser stale-generation regression minimal and unsettled', () => {
    const afterStaleGateIndex =
      savedSessionStreamThenStaleGeneration.steps.findIndex(
        (step) =>
          step.kind === 'gate' && step.name === 'after-stale-generation-output',
      );

    expect(savedSessionStaleGeneration.metadata.name).toBe(
      'saved-session-stale-generation',
    );
    expect(savedSessionStaleGeneration.steps).toEqual(
      savedSessionStreamThenStaleGeneration.steps.slice(0, afterStaleGateIndex),
    );
    expect(
      savedSessionStaleGeneration.steps.filter((step) => step.kind === 'gate'),
    ).toEqual([
      {
        kind: 'gate',
        name: 'before-stale-generation-output',
        required: true,
      },
    ]);
    const serializedSteps = JSON.stringify(savedSessionStaleGeneration.steps);
    expect(serializedSteps).not.toContain('agent_settled');
    expect(serializedSteps).not.toContain('"command":"prompt"');
  });

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
    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.gates()).toEqual([
      {
        name: 'before-stale-generation-output',
        required: true,
        reached: true,
        released: false,
      },
      {
        name: 'after-stale-generation-output',
        required: true,
        reached: false,
        released: false,
      },
    ]);
    engine.releaseGate('before-stale-generation-output');
    expect(takeRequiredOutput(engine)).toMatchObject({
      kind: 'event',
      runtime: { key: 'main', id: 'runtime-dynamic-47', generation: 1 },
      value: {
        type: 'message_update',
        assistantMessageEvent: { delta: 'STALE_GENERATION_SENTINEL' },
      },
    });
    expect(engine.takeOutput()).toBeUndefined();
    expect(engine.gates()).toEqual([
      {
        name: 'before-stale-generation-output',
        required: true,
        reached: true,
        released: true,
      },
      {
        name: 'after-stale-generation-output',
        required: true,
        reached: true,
        released: false,
      },
    ]);
    engine.releaseGate('after-stale-generation-output');
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
    expect(engine.takeOutput()).toBeUndefined();
    expect(() => engine.verifyComplete()).not.toThrow();
    expect(engine.timeline().map((entry) => entry.sequence)).toEqual(
      Array.from({ length: 31 }, (_, index) => index + 1),
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

  it('waits for gates and preserves deterministic release ordering', async () => {
    const engine = gatedOutputScenario();
    engine.bindRuntime('main', 'runtime-dynamic-47');
    let firstReached = false;
    const waiting = engine.waitForGateReached('first').then(() => {
      firstReached = true;
    });

    expect(engine.takeOutput()).toBeUndefined();
    await waiting;
    expect(firstReached).toBe(true);
    engine.releaseGate('first');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { type: 'agent_start' },
    });
    expect(engine.takeOutput()).toBeUndefined();
    await engine.waitForGateReached('second');
    engine.releaseGate('second');
    expect(takeRequiredOutput(engine)).toMatchObject({
      value: { type: 'agent_settled' },
    });

    expect(engine.timeline().map(({ kind }) => kind)).toEqual([
      'runtime-bound',
      'gate-reached',
      'gate-released',
      'output',
      'gate-reached',
      'gate-released',
      'output',
    ]);
    engine.verifyComplete();
  });

  it('fails clearly for unknown gates and invalid release transitions', () => {
    const beforeReach = gatedOutputScenario();
    beforeReach.bindRuntime('main', 'runtime-dynamic-47');
    expect(() => beforeReach.releaseGate('missing')).toThrowError(
      /Unknown scenario gate "missing"/,
    );
    expect(() => beforeReach.releaseGate('first')).toThrowError(
      /Gate "first" was released before it was reached/,
    );

    const duplicate = gatedOutputScenario();
    duplicate.bindRuntime('main', 'runtime-dynamic-47');
    duplicate.takeOutput();
    duplicate.releaseGate('first');
    expect(() => duplicate.releaseGate('first')).toThrowError(
      /Gate "first" was released more than once/,
    );
    expect(() => duplicate.waitForGateReached('missing')).toThrowError(
      /Unknown scenario gate "missing"/,
    );
  });

  it('rejects duplicate gate definitions and reports every unfinished required gate', () => {
    expect(
      () =>
        new PiScenarioEngine(
          definePiScenario({
            metadata: {
              name: 'duplicate-gates',
              purpose: 'Reject ambiguous gate names.',
              qualityRule: 'Diagnostics',
              schemaVersion: 1,
            },
            runtimes: [{ key: 'main', generation: 1 }],
            steps: [
              { kind: 'gate', name: 'same', required: true },
              { kind: 'gate', name: 'same', required: true },
            ],
          }),
        ),
    ).toThrowError(/gate "same" must have a unique non-empty name/);

    const neverReached = gatedOutputScenario();
    neverReached.bindRuntime('main', 'runtime-dynamic-47');
    expect(() => neverReached.verifyComplete()).toThrowError(
      /Unfinished required gates:[\s\S]*first: never reached, unreleased[\s\S]*second: never reached, unreleased[\s\S]*Gate states:/,
    );

    neverReached.takeOutput();
    expect(() => neverReached.verifyComplete()).toThrowError(
      /first: reached, unreleased[\s\S]*second: never reached, unreleased[\s\S]*Timeline:[\s\S]*gate-reached/,
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
