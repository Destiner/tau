import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import { state, type SessionController } from '../../../composables/state';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock('../../telemetry', () => ({
  invokeTraced: vi.fn(async () => undefined),
  recordControllerTransition: vi.fn(),
  recordRpcResponseAnomaly: vi.fn(),
  recordStreamAggregate: vi.fn(),
  startRpcSpan: vi.fn(() => ({ context: undefined, end: vi.fn() })),
}));

const mockInvoke = vi.mocked(invoke);

function feedbackMessage(controller: SessionController): string {
  return (
    [...controller.feedback]
      .reverse()
      .find((incident) => !incident.acknowledged)?.message ?? ''
  );
}

function makeController(
  overrides: Partial<SessionController> = {},
): SessionController {
  return sessionControllerFixture({
    sessionPath: '/tmp/project/session.jsonl',
    generation: 1,
    ready: true,
    ...overrides,
  });
}

beforeEach(async () => {
  mockInvoke.mockClear();
  state.controllers.splice(0);
  state.workspaceFeedback = [];
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.activeProjectPath = '';
  state.activeSessionId = '';
  state.activeSessionPath = '';
  state.activeControllerKey = '';
  state.workspace = null;
  state.remoteRetry = undefined;
  state.remoteDialogOpen = false;
  state.remoteDialogMode = 'add';
  state.remoteConnectionError = '';
  state.remoteConnecting = false;
  const telemetry = await import('../../telemetry');
  vi.mocked(telemetry.invokeTraced).mockReset();
  vi.mocked(telemetry.invokeTraced).mockResolvedValue(undefined);
});

describe('extension failures', () => {
  it('uses reviewed status copy for extension_error without retaining raw details', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController();
    const rawDetails = 'private extension failure details';

    await handleRpc(controller, {
      type: 'extension_error',
      extensionPath: rawDetails,
      event: rawDetails,
      error: rawDetails,
    });

    expect(feedbackMessage(controller)).toBe(
      'A Pi extension failed. Review the extension setup and try again.',
    );
    expect(controller.messages).toEqual([]);
    expect(JSON.stringify(controller)).not.toContain(rawDetails);
  });
});

describe('ordinary user transcript events', () => {
  it('reconciles transformed Pi input into the composer optimistic row', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController({
      submittedPrompt: {
        requestId: 'prompt-1',
        generation: 1,
        message: 'Original input',
        draft: 'Original input',
        accepted: true,
        optimisticId: 'optimistic-user-1',
      },
      messages: [
        {
          id: 'optimistic-user-1',
          kind: 'user',
          text: 'Original input',
          pending: true,
        },
        {
          id: 'extension-notify:1',
          kind: 'notice',
          text: 'Transforming input',
          anchor: 1,
        },
        {
          id: 'local-error-1',
          kind: 'error',
          text: 'A local warning',
          anchor: 1,
        },
      ],
    });

    await handleRpc(controller, { type: 'agent_start' });
    await handleRpc(controller, {
      type: 'message_start',
      message: { role: 'user', content: 'Transformed input' },
    });

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-user-1',
        kind: 'user',
        text: 'Transformed input',
      },
      {
        id: 'extension-notify:1',
        kind: 'notice',
        text: 'Transforming input',
        anchor: 1,
      },
      {
        id: 'local-error-1',
        kind: 'error',
        text: 'A local warning',
        anchor: 1,
      },
    ]);
  });

  it('appends every repeated extension or steering event and ignores empty content', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController();
    const repeated = {
      type: 'message_start',
      message: { role: 'user', content: 'Injected by extension' },
    };

    await handleRpc(controller, repeated);
    await handleRpc(controller, repeated);
    await handleRpc(controller, {
      type: 'message_start',
      message: { role: 'user', content: [{ type: 'image', data: 'hidden' }] },
    });

    expect(controller.messages).toEqual([
      {
        id: 'stream-user-0',
        kind: 'user',
        text: 'Injected by extension',
      },
      {
        id: 'stream-user-1',
        kind: 'user',
        text: 'Injected by extension',
      },
    ]);
  });

  it('isolates user events by runtime and generation', async () => {
    const { handleBridgeEvent } = await import('./index');
    const selected = makeController();
    const other = makeController({
      key: 'controller-2',
      runtimeId: 'runtime-2',
      sessionId: 'session-2',
    });
    state.controllers.push(selected, other);
    const line = JSON.stringify({
      type: 'message_start',
      message: { role: 'user', content: 'Only the matching runtime' },
    });

    await handleBridgeEvent({
      runtimeId: other.runtimeId,
      generation: other.generation,
      kind: 'rpc',
      line,
    });
    await handleBridgeEvent({
      runtimeId: selected.runtimeId,
      generation: selected.generation - 1,
      kind: 'rpc',
      line,
    });

    expect(selected.messages).toEqual([]);
    expect(other.messages).toMatchObject([
      { kind: 'user', text: 'Only the matching runtime' },
    ]);
  });
});

describe('skill transcript events', () => {
  it('optimistically keeps a known skill and its prompt together', async () => {
    const { appendOptimisticPrompt } = await import('./index');
    const controller = makeController({
      commands: [
        {
          name: 'skill:pr-review',
          source: 'skill',
          description: 'Review a PR',
        },
      ],
    });

    appendOptimisticPrompt(
      controller,
      '/skill:pr-review Focus on correctness',
      'optimistic-1',
    );

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-1',
        kind: 'skill',
        text: '',
        pending: true,
        skillName: 'pr-review',
        skillPrompt: 'Focus on correctness',
      },
    ]);
  });

  it('upgrades a raw optimistic command when command metadata arrived late', async () => {
    const { appendOptimisticPrompt, handleRpc } = await import('./index');
    const controller = makeController();
    appendOptimisticPrompt(
      controller,
      '/skill:pr-review Focus on correctness',
      'optimistic-user-1',
    );

    await handleRpc(controller, {
      type: 'message_start',
      message: {
        role: 'user',
        content: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
Full review instructions
</skill>

Focus on correctness`,
      },
    });

    expect(controller.messages).toMatchObject([
      {
        id: 'optimistic-user-1',
        kind: 'skill',
        text: 'Full review instructions',
        skillName: 'pr-review',
        skillPrompt: 'Focus on correctness',
      },
    ]);
  });

  it('fills optimistic skill details from the user message event', async () => {
    const { appendOptimisticPrompt, handleRpc } = await import('./index');
    const controller = makeController({
      commands: [{ name: 'skill:pr-review', source: 'skill' }],
    });
    appendOptimisticPrompt(controller, '/skill:pr-review', 'optimistic-1');

    await handleRpc(controller, {
      type: 'message_start',
      message: {
        role: 'user',
        content: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
Full review instructions
</skill>`,
      },
    });

    expect(controller.messages).toEqual([
      {
        id: 'optimistic-1',
        kind: 'skill',
        text: 'Full review instructions',
        skillName: 'pr-review',
      },
    ]);
  });
});

describe('live tool transcript events', () => {
  it('keeps complete arguments and results before settlement', async () => {
    const { handleRpc } = await import('./index');
    const controller = makeController({ streaming: true, working: true });
    const argumentTail = 'LIVE_ARGUMENT_TAIL_SENTINEL';
    const resultTail = 'LIVE_RESULT_TAIL_SENTINEL';
    const args = {
      command: `printf %s ${'argument '.repeat(520)}${argumentTail}`,
    };
    const result = `${'result '.repeat(650)}${resultTail}`;

    await handleRpc(controller, {
      type: 'tool_execution_start',
      toolCallId: 'long-live-call',
      toolName: 'bash',
      args,
    });
    await handleRpc(controller, {
      type: 'tool_execution_end',
      toolCallId: 'long-live-call',
      result: { content: [{ type: 'text', text: result }] },
      isError: false,
    });

    expect(controller.messages[0]).toMatchObject({
      toolRunning: false,
      toolErrored: false,
      toolArguments: JSON.stringify(args, null, 2),
      toolResult: result,
    });
    expect(
      controller.messages[0]?.toolArguments?.indexOf(argumentTail),
    ).toBeGreaterThan(4_000);
    expect(
      controller.messages[0]?.toolResult?.indexOf(resultTail),
    ).toBeGreaterThan(4_000);
  });
});
