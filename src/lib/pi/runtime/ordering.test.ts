import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import sessionControllerFixture from '../../../../tests/support/session-controller';
import {
  nextRequestId,
  state,
  type SessionController,
} from '../../../composables/state';

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

const user = (text: string): Record<string, unknown> => ({
  role: 'user',
  content: text,
});
const assistant = (
  text: string,
  kind = 'assistant',
): Record<string, unknown> => ({
  role: 'assistant',
  content: [
    kind === 'thinking'
      ? { type: 'thinking', thinking: text }
      : { type: 'text', text },
  ],
});
const rows = (c: SessionController): string[][] =>
  c.messages.map((e) => [e.kind, e.text]);
async function hydrate(
  c: SessionController,
  messages: unknown[],
): Promise<void> {
  const { rpc, handleResponse } = await import('./index');
  const id = nextRequestId('ordering-messages');
  await rpc(c, { id, type: 'get_messages' });
  await handleResponse(c, {
    id,
    command: 'get_messages',
    success: true,
    data: { messages },
  });
}
async function start(
  c: SessionController,
  role = 'assistant',
  text = '',
): Promise<void> {
  const { handleRpc } = await import('./index');
  await handleRpc(c, {
    type: 'message_start',
    message: role === 'user' ? user(text) : assistant(''),
  });
}
async function delta(
  c: SessionController,
  text: string,
  kind = 'assistant',
  contentIndex = 0,
): Promise<void> {
  const { handleRpc } = await import('./index');
  await handleRpc(c, {
    type: 'message_update',
    assistantMessageEvent: {
      type: kind === 'thinking' ? 'thinking_delta' : 'text_delta',
      delta: text,
      contentIndex,
    },
  });
}
async function notify(
  c: SessionController,
  id: string,
  severity = 'warning',
): Promise<void> {
  const { handleRpc } = await import('./index');
  await handleRpc(c, {
    type: 'extension_ui_request',
    method: 'notify',
    id,
    message: 'feedback',
    notifyType: severity,
  });
}
function controller(
  overrides: Partial<SessionController> = {},
): SessionController {
  return sessionControllerFixture({
    generation: 1,
    ready: true,
    messagesLoaded: true,
    sessionPath: '/tmp/project/session.jsonl',
    ...overrides,
  });
}
beforeEach(() => {
  vi.mocked(invoke).mockClear();
  state.controllers.splice(0);
  state.ephemeralSessions.splice(0);
  state.extensionDialogs.splice(0);
  state.workspace = null;
  state.activeControllerKey = '';
});

describe('local feedback source ordering', () => {
  it.each(
    ['assistant', 'thinking'].flatMap((kind) =>
      ['info', 'warning', 'error'].map((severity) => [kind, severity]),
    ),
  )(
    'preserves %s splits for %s through repeated hydration and later turns',
    async (kind, severity) => {
      const c = controller();
      await hydrate(c, [user('question')]);
      await start(c);
      await delta(c, 'before', kind);
      await notify(c, 'one', severity);
      await notify(c, 'two', severity);
      await notify(c, 'one', severity);
      await delta(c, 'middle', kind);
      await notify(c, 'three', severity);
      await delta(c, 'after', kind);
      const initial = rows(c);
      const ids = c.messages.map((e) => e.id);
      const history = [user('question'), assistant('beforemiddleafter', kind)];
      for (let i = 0; i < 3; i++) {
        await hydrate(c, history);
        expect(rows(c)).toEqual(initial);
        expect(c.messages.map((e) => e.id)).toEqual(ids);
      }
      await start(c);
      await delta(c, 'distinct', kind);
      await hydrate(c, [...history, assistant('distinct', kind)]);
      expect(rows(c)).toEqual([...initial, [kind, 'distinct']]);
      await start(c, 'user', 'next');
      await start(c);
      await delta(c, 'reply');
      await hydrate(c, [
        ...history,
        assistant('distinct', kind),
        user('next'),
        assistant('reply'),
      ]);
      expect(rows(c)).toEqual([
        ...initial,
        [kind, 'distinct'],
        ['user', 'next'],
        ['assistant', 'reply'],
      ]);
      expect(new Set(c.messages.map((e) => e.id)).size).toBe(c.messages.length);
    },
  );

  it('does not join distinct messages or content blocks with identical text', async () => {
    const c = controller();
    await start(c);
    await delta(c, 'same');
    await notify(c, 'a');
    await delta(c, 'same');
    await start(c);
    await delta(c, 'same');
    await notify(c, 'b');
    await delta(c, 'same');
    await delta(c, 'same', 'assistant', 1);
    await notify(c, 'c');
    await delta(c, 'same', 'assistant', 1);
    const initial = rows(c);
    const ids = c.messages.map((e) => e.id);
    const history = [
      assistant('samesame'),
      {
        role: 'assistant',
        content: [
          { type: 'text', text: 'samesame' },
          { type: 'text', text: 'samesame' },
        ],
      },
    ];
    await hydrate(c, history);
    await hydrate(c, history);
    expect(rows(c)).toEqual(initial);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
  });

  it('preserves live tool and following message identities across ordinary refreshes', async () => {
    const c = controller();
    const { handleRpc } = await import('./index');
    await start(c);
    await delta(c, 'checking');
    const call = { type: 'toolCall', id: 'call', name: 'read', arguments: {} };
    const first = {
      role: 'assistant',
      content: [{ type: 'text', text: 'checking' }, call],
    };
    await handleRpc(c, { type: 'message_end', message: first });
    await handleRpc(c, {
      type: 'tool_execution_start',
      toolCallId: 'call',
      toolName: 'read',
      args: {},
    });
    await notify(c, 'after-tool');
    await handleRpc(c, {
      type: 'tool_execution_end',
      toolCallId: 'call',
      result: { content: [{ type: 'text', text: 'result' }] },
    });
    await start(c);
    await delta(c, 'before');
    await notify(c, 'inside-reply');
    await delta(c, 'after');
    const initial = rows(c);
    const ids = c.messages.map((entry) => entry.id);
    const history = [
      first,
      {
        role: 'toolResult',
        toolCallId: 'call',
        content: [{ type: 'text', text: 'result' }],
      },
      assistant('beforeafter'),
    ];
    for (let i = 0; i < 3; i++) {
      await hydrate(c, history);
      expect(rows(c)).toEqual(initial);
      expect(c.messages.map((entry) => entry.id)).toEqual(ids);
    }
  });

  it.each(['notice', 'error'])(
    'keeps %s feedback after its live tool when history supplies earlier text blocks',
    async (kind) => {
      const c = controller();
      const { handleRpc } = await import('./index');
      await start(c);
      await handleRpc(c, {
        type: 'tool_execution_start',
        toolCallId: 'call',
        toolName: 'read',
        args: {},
      });
      if (kind === 'notice') await notify(c, 'tool-boundary');
      else
        await handleRpc(c, {
          type: 'compaction_end',
          errorMessage: 'feedback',
        });
      const toolId = c.messages[0]!.id;
      const noticeId = c.messages[1]!.id;
      const feedbackText = c.messages[1]!.text;
      const history = [
        {
          role: 'assistant',
          content: [
            { type: 'text', text: 'finalized explanation' },
            { type: 'toolCall', id: 'call', name: 'read', arguments: {} },
          ],
        },
        assistant('later'),
      ];
      for (let i = 0; i < 3; i++) {
        await hydrate(c, history);
        expect(rows(c)).toEqual([
          ['assistant', 'finalized explanation'],
          ['tool', '{}'],
          [kind, feedbackText],
          ['assistant', 'later'],
        ]);
        expect(c.messages[1]!.id).toBe(toolId);
        expect(c.messages[2]!.id).toBe(noticeId);
      }
    },
  );

  it.each([1, 3])(
    'captures tail-relative anchors with %i revealed layers',
    async (count) => {
      const c = controller();
      await hydrate(c, [user('current'), assistant('reply')]);
      const { applyHistoryTail } = await import('./history');
      c.historyLayers = Array.from({ length: count }, (_, i) => ({
        markerId: `marker-${i}`,
        rows: [{ id: `prefix-${i}`, kind: 'user' as const, text: 'old' }],
      }));
      applyHistoryTail(c, c.messages);
      await notify(c, 'tail');
      await hydrate(c, [
        user('current'),
        assistant('reply'),
        user('later'),
        assistant('later reply'),
      ]);
      expect(rows(c).slice(c.historyPrefixLength)).toEqual([
        ['user', 'current'],
        ['assistant', 'reply'],
        ['notice', 'feedback'],
        ['user', 'later'],
        ['assistant', 'later reply'],
      ]);
    },
  );

  it('splits authoritative output arriving only in history at a captured text offset', async () => {
    const c = controller();
    await start(c);
    await delta(c, 'before');
    await notify(c, 'history-only');
    const firstId = c.messages[0]!.id;
    const history = [assistant('beforefinal suffix')];
    await hydrate(c, history);
    const ids = c.messages.map((e) => e.id);
    expect(rows(c)).toEqual([
      ['assistant', 'before'],
      ['notice', 'feedback'],
      ['assistant', 'final suffix'],
    ]);
    expect(ids[0]).toBe(firstId);
    await hydrate(c, history);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
  });

  it.each(['rewritten', 'x'])(
    'uses authoritative refined content (%s) and falls back to the end of its owning block',
    async (refined) => {
      const c = controller();
      await start(c);
      await delta(c, 'before');
      await notify(c, 'refined');
      await delta(c, 'after');
      await start(c);
      await delta(c, 'later');
      const history = [assistant(refined), assistant('later')];
      await hydrate(c, history);
      await hydrate(c, history);
      expect(rows(c)).toEqual([
        ['assistant', refined],
        ['notice', 'feedback'],
        ['assistant', 'later'],
      ]);
    },
  );

  it('keeps feedback in its owning message when refinement removes a block', async () => {
    const c = controller();
    await start(c);
    await delta(c, 'thought', 'thinking', 0);
    await delta(c, 'before', 'assistant', 1);
    await notify(c, 'removed-block');
    await delta(c, 'after', 'assistant', 1);
    await start(c);
    await delta(c, 'later');
    const history = [assistant('rewritten owner'), assistant('later')];
    await hydrate(c, history);
    await hydrate(c, history);
    expect(rows(c)).toEqual([
      ['assistant', 'rewritten owner'],
      ['notice', 'feedback'],
      ['assistant', 'later'],
    ]);
  });

  it('retains proven splits while replacing only the authoritative suffix', async () => {
    const c = controller();
    await start(c);
    await delta(c, 'before');
    await notify(c, 'refined-suffix');
    await delta(c, 'stale suffix');
    const ids = c.messages.map((e) => e.id);
    await hydrate(c, [assistant('beforefinal suffix')]);
    expect(rows(c)).toEqual([
      ['assistant', 'before'],
      ['notice', 'feedback'],
      ['assistant', 'final suffix'],
    ]);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
  });

  it('preserves a local compaction failure inside a stream through later turns', async () => {
    const { handleRpc } = await import('./index');
    const c = controller();
    await start(c);
    await delta(c, 'before');
    await handleRpc(c, {
      type: 'compaction_end',
      errorMessage: 'fixture failure',
    });
    await delta(c, 'after');
    const initial = rows(c);
    const ids = c.messages.map((e) => e.id);
    await hydrate(c, [assistant('beforeafter')]);
    expect(rows(c)).toEqual(initial);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
    await handleRpc(c, { type: 'agent_start' });
    await hydrate(c, [
      assistant('beforeafter'),
      user('later'),
      assistant('later reply'),
    ]);
    expect(rows(c)).toEqual([
      ...initial,
      ['user', 'later'],
      ['assistant', 'later reply'],
    ]);
  });

  it('continues a retained assistant message after compaction hydration without creating another row', async () => {
    const { handleRpc, handleResponse } = await import('./index');
    const c = controller({ streaming: true });
    await hydrate(c, [user('Compact this fixture')]);
    await delta(c, 'Ready to compact.');
    const source = c.messages.at(-1)!.source;
    const id = c.messages.at(-1)!.id;
    await handleRpc(c, { type: 'compaction_start' });
    await handleRpc(c, { type: 'compaction_end', result: {} });
    const request = vi
      .mocked(invoke)
      .mock.calls.filter(([name]) => name === 'send_pi')
      .map(
        ([, args]) =>
          (args as { request: { id: string; type: string } }).request,
      )
      .filter((r) => r.type === 'get_messages')
      .at(-1)!;
    await handleResponse(c, {
      id: request.id,
      command: 'get_messages',
      success: true,
      data: {
        messages: [
          { role: 'compactionSummary' },
          user('Compact this fixture'),
          assistant('Ready to compact.'),
        ],
      },
    });
    await delta(c, ' Still working.');
    expect(rows(c)).toEqual([
      ['compaction', ''],
      ['user', 'Compact this fixture'],
      ['assistant', 'Ready to compact. Still working.'],
    ]);
    expect(c.messages.at(-1)).toMatchObject({ id, source });
  });

  it('retains fresh compaction splits through the next ordinary refresh', async () => {
    const { handleRpc, handleResponse } = await import('./index');
    const c = controller({ streaming: true });
    await hydrate(c, [user('old'), assistant('old reply')]);
    await notify(c, 'old');
    await handleRpc(c, { type: 'compaction_end', result: {} });
    const request = vi
      .mocked(invoke)
      .mock.calls.filter(([name]) => name === 'send_pi')
      .map(
        ([, args]) =>
          (args as { request: { id: string; type: string } }).request,
      )
      .filter((r) => r.type === 'get_messages')
      .at(-1)!;
    await start(c);
    await delta(c, 'before');
    await notify(c, 'fresh');
    await delta(c, 'after');
    const history = [
      { role: 'compactionSummary' },
      user('old'),
      assistant('beforeafter'),
    ];
    await handleResponse(c, {
      id: request.id,
      command: 'get_messages',
      success: true,
      data: { messages: history },
    });
    const initial = rows(c);
    const ids = c.messages.map((e) => e.id);
    expect(initial).toEqual([
      ['compaction', ''],
      ['user', 'old'],
      ['assistant', 'before'],
      ['notice', 'feedback'],
      ['assistant', 'after'],
    ]);
    await hydrate(c, history);
    await hydrate(c, history);
    expect(rows(c)).toEqual(initial);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
  });

  it.each(['beforeafter', 'beforefinal', 'rewritten'])(
    'keeps one remote interruption across reconnect with authoritative content: %s',
    async (persisted) => {
      const { handleBridgeEvent, handleResponse, rpc } =
        await import('./index');
      const c = controller({
        streaming: true,
        working: true,
        draft: 'keep draft',
      });
      state.controllers.push(c);
      state.workspace = {
        activeProjectPath: c.projectPath,
        piPath: null,
        projects: [
          {
            path: c.projectPath,
            name: 'Remote',
            workingDirectory: '/remote/project',
            connectionString: 'ssh fixture@example',
            collapsed: false,
            selected: true,
            sessions: [],
          },
        ],
      };
      await hydrate(c, [user('question')]);
      const { applyHistoryTail } = await import('./history');
      c.historyLayers = [
        {
          markerId: 'old-marker',
          rows: [{ id: 'old', kind: 'user', text: 'old' }],
        },
      ];
      applyHistoryTail(c, c.messages);
      await start(c);
      await delta(c, 'before');
      await notify(c, 'earlier');
      await delta(c, 'after');
      await handleBridgeEvent({
        runtimeId: c.runtimeId,
        generation: c.generation,
        kind: 'exited',
        code: 255,
      });
      const original = rows(c).slice(c.historyPrefixLength);
      const expected =
        persisted === 'rewritten'
          ? [
              ['user', 'question'],
              ['assistant', 'rewritten'],
              ['notice', 'feedback'],
              original[4]!,
            ]
          : original.map((row) =>
              row[1] === 'after' ? ['assistant', persisted.slice(6)] : row,
            );
      const ids = c.messages.slice(c.historyPrefixLength).map((e) => e.id);
      c.generation = 2;
      c.recoveringRuntime = true;
      const id = nextRequestId('start-messages');
      c.startMessagesRequestId = id;
      await rpc(c, { id, type: 'get_messages' });
      const history = [user('question'), assistant(persisted)];
      await handleResponse(c, {
        id,
        command: 'get_messages',
        success: true,
        data: { messages: history },
      });
      expect(rows(c).slice(c.historyPrefixLength)).toEqual(expected);
      if (persisted !== 'rewritten')
        expect(
          c.messages.slice(c.historyPrefixLength).map((e) => e.id),
        ).toEqual(ids);
      await hydrate(c, [...history, user('later'), assistant('later reply')]);
      expect(rows(c).slice(c.historyPrefixLength)).toEqual([
        ...expected,
        ['user', 'later'],
        ['assistant', 'later reply'],
      ]);
      expect(c.draft).toBe('keep draft');
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(
            ([name, args]) =>
              name === 'send_pi' &&
              (args as { request: { type: string } }).request.type === 'prompt',
          ),
      ).toEqual([]);
    },
  );

  it.each(['unresolved', 'success', 'error'] as const)(
    'keeps an interrupted remote tool final after reconnect: %s',
    async (result) => {
      const { handleBridgeEvent, handleRpc, handleResponse, rpc } =
        await import('./index');
      const c = controller({ streaming: true, working: true });
      state.controllers.push(c);
      state.workspace = {
        activeProjectPath: c.projectPath,
        piPath: null,
        projects: [
          {
            path: c.projectPath,
            name: 'Remote',
            workingDirectory: '/remote/project',
            connectionString: 'ssh fixture@example',
            collapsed: false,
            selected: true,
            sessions: [],
          },
        ],
      };
      const call = {
        role: 'assistant',
        content: [
          {
            type: 'toolCall',
            id: 'interrupted-call',
            name: 'bash',
            arguments: { command: 'command' },
          },
        ],
      };
      await start(c);
      await handleRpc(c, { type: 'message_end', message: call });
      await handleRpc(c, {
        type: 'tool_execution_start',
        toolCallId: 'interrupted-call',
        toolName: 'bash',
        args: { command: 'command' },
      });
      const tool = (): (typeof c.messages)[number] =>
        c.messages.find((entry) => entry.kind === 'tool')!;
      const toolId = tool().id;
      expect(tool().toolRunning).toBe(true);
      await handleBridgeEvent({
        runtimeId: c.runtimeId,
        generation: c.generation,
        kind: 'exited',
        code: 255,
      });
      expect(tool().toolRunning).toBe(false);
      const history: unknown[] = [call];
      if (result !== 'unresolved') {
        history.push({
          role: 'toolResult',
          toolCallId: 'interrupted-call',
          content: [{ type: 'text', text: 'persisted result' }],
          isError: result === 'error',
        });
      }
      c.generation++;
      c.recoveringRuntime = true;
      const id = nextRequestId('start-messages');
      c.startMessagesRequestId = id;
      await rpc(c, { id, type: 'get_messages' });
      await handleResponse(c, {
        id,
        command: 'get_messages',
        success: true,
        data: { messages: history },
      });
      const assertFinal = (): void => {
        expect(
          c.messages.filter((entry) => entry.kind === 'tool'),
        ).toHaveLength(1);
        expect(tool()).toMatchObject({ id: toolId, toolRunning: false });
        if (result !== 'unresolved') {
          expect(tool()).toMatchObject({
            toolResult: 'persisted result',
            toolErrored: result === 'error',
          });
        }
      };
      assertFinal();
      await hydrate(c, history);
      assertFinal();
      await hydrate(c, [...history, user('later'), assistant('later reply')]);
      assertFinal();
    },
  );

  it('does not reconcile missing reconnect text against a different message with equal text', async () => {
    const { hydrateTranscript, mergeLocalEntries } =
      await import('../transcript');
    const { mergeReconnectedStream } = await import('./history');
    const c = controller();
    await start(c);
    await delta(c, 'same');
    await notify(c, 'first-gap');
    await delta(c, ' suffix');
    await start(c);
    await delta(c, 'same');
    const initial = rows(c);
    const ids = c.messages.map((entry) => entry.id);
    const snapshot = hydrateTranscript(
      [assistant('same'), assistant('same')],
      c.messages,
    );
    const reconciled = mergeLocalEntries(
      mergeReconnectedStream(snapshot, c.messages),
      [],
      c.messages,
    );
    expect(reconciled.map((entry) => [entry.kind, entry.text])).toEqual(
      initial,
    );
    expect(reconciled.map((entry) => entry.id)).toEqual(ids);
  });

  it('keeps unrecorded reconnect output but adopts authoritative tool completion', async () => {
    const { hydrateTranscript, mergeLocalEntries } =
      await import('../transcript');
    const { mergeReconnectedStream, mergeLiveStreamSuffix } =
      await import('./history');
    const c = controller();
    await start(c);
    await delta(c, 'before');
    await notify(c, 'partial');
    await delta(c, 'after');
    const snapshot = hydrateTranscript([assistant('before')], c.messages);
    expect(
      mergeLocalEntries(
        mergeReconnectedStream(snapshot, c.messages),
        [],
        c.messages,
      ).map((e) => [e.kind, e.text]),
    ).toEqual(rows(c));
    const previous = [
      {
        id: 'stream-tool-99',
        kind: 'tool' as const,
        text: 'command',
        toolCallId: 'call',
        toolRunning: true,
      },
    ];
    const hydrated = hydrateTranscript(
      [
        {
          role: 'assistant',
          content: [
            {
              type: 'toolCall',
              id: 'call',
              name: 'bash',
              arguments: { command: 'command' },
            },
          ],
        },
        {
          role: 'toolResult',
          toolCallId: 'call',
          content: [{ type: 'text', text: 'done' }],
          isError: false,
        },
      ],
      previous,
    );
    const expected = [
      expect.objectContaining({
        id: 'stream-tool-99',
        toolRunning: false,
        toolResult: 'done',
      }),
    ];
    expect(mergeReconnectedStream(hydrated, previous)).toEqual(expected);
    expect(mergeLiveStreamSuffix(hydrated, previous, 99)).toEqual(expected);
  });

  it.each(['generation', 'identity'])(
    'ignores stale %s hydration without mutating feedback provenance',
    async (stale) => {
      const { rpc, handleResponse } = await import('./index');
      const c = controller();
      await start(c);
      await delta(c, 'before');
      await notify(c, 'stale');
      await delta(c, 'after');
      const id = nextRequestId('stale-ordering');
      await rpc(c, { id, type: 'get_messages' });
      const original = structuredClone(c.messages);
      if (stale === 'generation') c.generation++;
      else {
        c.sessionId = 'replacement';
        c.sessionPath = '/tmp/replacement.jsonl';
      }
      await handleResponse(c, {
        id,
        command: 'get_messages',
        success: true,
        data: { messages: [assistant('wrong history')] },
      });
      expect(c.messages).toEqual(original);
    },
  );

  it('cold-restores Pi-owned failures before later successful turns', async () => {
    const c = controller({ messagesLoaded: false });
    const history = [
      user('failed question'),
      {
        role: 'assistant',
        content: [],
        stopReason: 'error',
        errorMessage: 'overloaded',
      },
      user('later'),
      assistant('success'),
    ];
    await hydrate(c, history);
    const initial = rows(c);
    const ids = c.messages.map((e) => e.id);
    expect(c.messages.map((e) => e.kind)).toEqual([
      'user',
      'error',
      'user',
      'assistant',
    ]);
    await hydrate(c, history);
    expect(rows(c)).toEqual(initial);
    expect(c.messages.map((e) => e.id)).toEqual(ids);
  });

  it.each([false, true])(
    'settles bootstrap feedback once (empty history: %s)',
    async (empty) => {
      const c = controller({ messagesLoaded: false });
      await notify(c, 'bootstrap');
      const history = empty ? [] : [user('loaded'), assistant('loaded reply')];
      await hydrate(c, history);
      const initial = rows(c);
      await start(c);
      await delta(c, 'new');
      await hydrate(c, [...history, assistant('new')]);
      await hydrate(c, [...history, assistant('new')]);
      expect(rows(c)).toEqual([...initial, ['assistant', 'new']]);
    },
  );
});
