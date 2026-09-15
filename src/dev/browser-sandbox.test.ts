import { describe, expect, it } from 'vitest';

import type { PiBridgeEvent } from '../lib/pi/bridge';

import {
  createBrowserSandboxHandler,
  createBrowserSandboxWorkspace,
} from './browser-sandbox';

function rpcLines(events: PiBridgeEvent[]): Record<string, unknown>[] {
  return events
    .filter((event) => event.kind === 'rpc' && event.line)
    .map((event) => JSON.parse(event.line ?? '') as Record<string, unknown>);
}

describe('browser sandbox seed', () => {
  it('starts fresh with the requested projects and five selected sessions', () => {
    const first = createBrowserSandboxWorkspace();
    const second = createBrowserSandboxWorkspace();

    expect(first.projects.map((project) => project.name)).toEqual([
      'atlas',
      'notes',
    ]);
    expect(first.projects.map((project) => project.sessions.length)).toEqual([
      3, 2,
    ]);
    expect(first.activeProjectPath).toBe('/browser-dev/projects/atlas');
    expect(
      first.projects
        .flatMap((project) => project.sessions)
        .filter((session) => session.selected),
    ).toHaveLength(1);

    first.projects[0]?.sessions.splice(0, 1);
    expect(second.projects[0]?.sessions).toHaveLength(3);
  });

  it('preserves mutable history when a runtime stops and starts again', async () => {
    const events: PiBridgeEvent[] = [];
    const handle = createBrowserSandboxHandler(async (event) => {
      events.push(event);
    });
    const runtimeId = 'runtime-1';
    const projectPath = '/browser-dev/projects/atlas';
    const sessionPath = `${projectPath}/atlas-overview.jsonl`;

    await handle('start_pi', { runtimeId, projectPath, sessionPath });
    await handle('send_pi', {
      runtimeId,
      request: { type: 'prompt', id: 'prompt-1', message: 'Keep this turn' },
    });
    await handle('stop_pi', { runtimeId });
    await handle('start_pi', { runtimeId, projectPath, sessionPath });
    await handle('send_pi', {
      runtimeId,
      request: { type: 'get_messages', id: 'messages-1' },
    });

    const response = rpcLines(events).find(
      (line) => line.type === 'response' && line.id === 'messages-1',
    );
    expect(response?.data).toMatchObject({
      messages: expect.arrayContaining([
        { role: 'user', content: 'Keep this turn' },
        {
          role: 'assistant',
          content: [
            {
              type: 'text',
              text: expect.stringContaining('Keep this turn'),
            },
          ],
        },
      ]),
    });
  });

  it.each(['abort', 'stop'] as const)(
    'does not continue a prompt after %s invalidates its async work',
    async (interruption) => {
      const events: PiBridgeEvent[] = [];
      let releaseAgentStart: (() => void) | undefined;
      const agentStartBlocked = new Promise<void>((resolve) => {
        releaseAgentStart = resolve;
      });
      const handle = createBrowserSandboxHandler(async (event) => {
        events.push(event);
        if (event.line && JSON.parse(event.line).type === 'agent_start') {
          await agentStartBlocked;
        }
      });
      const runtimeId = 'runtime-1';

      await handle('start_pi', {
        runtimeId,
        projectPath: '/browser-dev/projects/atlas',
        sessionPath: '/browser-dev/projects/atlas/atlas-overview.jsonl',
      });
      const prompt = handle('send_pi', {
        runtimeId,
        request: { type: 'prompt', id: 'prompt-1', message: 'Stop here' },
      });
      await Promise.resolve();
      if (interruption === 'abort') {
        await handle('send_pi', {
          runtimeId,
          request: { type: 'abort', id: 'abort-1' },
        });
      } else {
        await handle('stop_pi', { runtimeId });
      }
      releaseAgentStart?.();
      await prompt;

      expect(
        rpcLines(events).filter((line) => line.type === 'message_start'),
      ).toHaveLength(0);
    },
  );
});
