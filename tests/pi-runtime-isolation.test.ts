import { beforeEach, expect, it, vi } from 'vitest';

import { state } from '../src/composables/state';
import { handleBridgeEvent } from '../src/lib/pi/runtime';

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

it('rejects a stale-generation delta without changing the working session', async () => {
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
  const before = {
    messages: structuredClone(controller.messages),
    working: controller.working,
    streaming: controller.streaming,
    ready: controller.ready,
    generation: controller.generation,
    unread: controller.unread,
  };

  await handleBridgeEvent({
    runtimeId: controller.runtimeId,
    generation: controller.generation - 1,
    kind: 'rpc',
    line: JSON.stringify({
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: 'Stale output' },
    }),
  });

  expect({
    messages: controller.messages,
    working: controller.working,
    streaming: controller.streaming,
    ready: controller.ready,
    generation: controller.generation,
    unread: controller.unread,
  }).toEqual(before);
});
