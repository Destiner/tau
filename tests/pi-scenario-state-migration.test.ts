import { beforeEach, expect, it, vi } from 'vitest';

import { state } from '../src/composables/state';
import { handleBridgeEvent } from '../src/lib/pi/runtime';

import { PiScenarioEngine } from './support/pi-scenario';
import { savedSessionStaleGeneration } from './support/pi-scenario/saved-session-stream-then-stale-generation';
import sessionControllerFixture from './support/session-controller';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../src/lib/telemetry', () => ({
  invokeTraced: vi.fn(),
  recordControllerTransition: vi.fn(),
  recordRpcResponseAnomaly: vi.fn(),
  recordStreamAggregate: vi.fn(),
  startRpcSpan: vi.fn(() => ({
    context: { traceId: 'test', spanId: 'test', sampled: false },
    end: vi.fn(),
  })),
}));

beforeEach(() => {
  state.controllers.splice(0);
  state.activeControllerKey = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.workspace = null;
});

it('rejects a stale-generation scenario delta without changing the working session', async () => {
  const controller = sessionControllerFixture({
    generation: 2,
    ready: true,
    working: true,
    streaming: true,
    messages: [
      { id: 'user', kind: 'user', text: 'Explain the fixture' },
      { id: 'reply', kind: 'assistant', text: 'Deterministic reply.' },
    ],
  });
  state.controllers.push(controller);
  state.activeControllerKey = controller.key;
  state.activeSessionId = controller.sessionId;
  state.activeSessionPath = controller.sessionPath;
  const engine = new PiScenarioEngine(savedSessionStaleGeneration);
  expect(savedSessionStaleGeneration.metadata).toMatchObject({
    name: 'saved-session-stale-generation',
    schemaVersion: 1,
  });
  engine.bindRuntime('main', controller.runtimeId);
  const before = {
    messages: structuredClone(controller.messages),
    working: controller.working,
    streaming: controller.streaming,
    ready: controller.ready,
    generation: controller.generation,
    unread: controller.unread,
  };

  for (const [index, step] of savedSessionStaleGeneration.steps.entries()) {
    if (step.kind === 'request') {
      engine.consumeRequest(controller.runtimeId, {
        id: `scenario-${index}`,
        ...step.match,
      });
      continue;
    }
    if (step.kind === 'gate') {
      expect(engine.takeOutput()).toBeUndefined();
      await engine.waitForGateReached(step.name);
      engine.releaseGate(step.name);
      continue;
    }
    const output = engine.takeOutput();
    expect(output).toBeDefined();
    if (output?.kind === 'event' && output.runtime.generation === 1) {
      await handleBridgeEvent({
        runtimeId: output.runtime.id,
        generation: output.runtime.generation,
        kind: 'rpc',
        line: JSON.stringify(output.value),
      });
    }
  }

  expect({
    messages: controller.messages,
    working: controller.working,
    streaming: controller.streaming,
    ready: controller.ready,
    generation: controller.generation,
    unread: controller.unread,
  }).toEqual(before);
  expect(engine.gates()).toEqual([
    {
      name: 'before-stale-generation-output',
      required: true,
      reached: true,
      released: true,
    },
  ]);
  expect(engine.timeline().slice(-2)).toEqual([
    expect.objectContaining({
      kind: 'gate-released',
      gate: 'before-stale-generation-output',
    }),
    expect.objectContaining({
      kind: 'output',
      generation: 1,
      output: 'event main@1 message_update',
    }),
  ]);
  expect(engine.isComplete()).toBe(true);
  expect(() => engine.verifyComplete()).not.toThrow();
});
