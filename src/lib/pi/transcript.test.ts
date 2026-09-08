import { describe, expect, it } from 'vitest';

import type { LocalError, TranscriptEntry } from './transcript';
import {
  hydrateTranscript,
  historyLayersFromEntries,
  historyPrefix,
  mergeLocalEntries,
  messageFailure,
  parseSkillBlock,
  projectOrdinaryUserMessage,
  toolSummary,
} from './transcript';

describe('ordinary user event projection', () => {
  it('keeps repeated identical extension or steering events distinct', () => {
    const entries: TranscriptEntry[] = [];

    projectOrdinaryUserMessage(entries, 'Repeat this', 'stream-user-1');
    projectOrdinaryUserMessage(entries, 'Repeat this', 'stream-user-2');

    expect(entries).toEqual([
      { id: 'stream-user-1', kind: 'user', text: 'Repeat this' },
      { id: 'stream-user-2', kind: 'user', text: 'Repeat this' },
    ]);
  });

  it('reconciles a pending optimistic row across only local entries', () => {
    const entries: TranscriptEntry[] = [
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
    ];

    projectOrdinaryUserMessage(entries, 'Transformed input', 'stream-user-1');

    expect(entries).toEqual([
      {
        id: 'optimistic-user-1',
        kind: 'user',
        text: 'Transformed input',
      },
      expect.objectContaining({ id: 'extension-notify:1' }),
      expect.objectContaining({ id: 'local-error-1' }),
    ]);
  });

  it('does not reconcile an earlier completed optimistic or user row', () => {
    const entries: TranscriptEntry[] = [
      {
        id: 'optimistic-user-old',
        kind: 'user',
        text: 'Repeat this',
      },
      {
        id: 'extension-notify:1',
        kind: 'notice',
        text: 'Queued another turn',
        anchor: 1,
      },
    ];

    projectOrdinaryUserMessage(entries, 'Repeat this', 'stream-user-2');

    expect(entries.map((entry) => entry.id)).toEqual([
      'optimistic-user-old',
      'extension-notify:1',
      'stream-user-2',
    ]);
  });

  it('suppresses only the matching event for a live hydration row', () => {
    const entries = hydrateTranscript(
      [{ role: 'user', content: 'Injected by extension' }],
      [],
      true,
    );
    const id = entries[0]?.id;

    projectOrdinaryUserMessage(
      entries,
      'Injected by extension',
      'stream-user-1',
    );
    projectOrdinaryUserMessage(
      entries,
      'Injected by extension',
      'stream-user-2',
    );

    expect(entries).toEqual([
      { id, kind: 'user', text: 'Injected by extension' },
      {
        id: 'stream-user-2',
        kind: 'user',
        text: 'Injected by extension',
      },
    ]);
  });

  it('keeps a pending optimistic row through startup hydration', () => {
    const optimistic: TranscriptEntry[] = [
      {
        id: 'optimistic-user-1',
        kind: 'user',
        text: 'Not dispatched yet',
        pending: true,
      },
    ];

    expect(hydrateTranscript([], optimistic, true)).toEqual(optimistic);
    expect(hydrateTranscript([], optimistic, false)).toEqual([]);
  });

  it('keeps a projected user row through early streaming hydration', () => {
    const projected: TranscriptEntry[] = [
      {
        id: 'stream-user-1',
        kind: 'user',
        text: 'Not persisted yet',
        pendingUserEvent: 'optimistic',
      },
    ];

    expect(hydrateTranscript([], projected, true)).toEqual(projected);
    expect(hydrateTranscript([], projected, false)).toEqual([]);
  });

  it('lets settled hydration adopt repeated event rows without duplication', () => {
    const projected: TranscriptEntry[] = [];
    projectOrdinaryUserMessage(projected, 'Repeat this', 'stream-user-7');
    projectOrdinaryUserMessage(projected, 'Repeat this', 'stream-user-8');

    const settled = hydrateTranscript(
      [
        { role: 'user', content: 'Repeat this' },
        { role: 'user', content: 'Repeat this' },
        {
          role: 'assistant',
          content: [{ type: 'text', text: 'Done.' }],
        },
      ],
      projected,
    );

    expect(settled.filter((entry) => entry.kind === 'user')).toEqual([
      { id: 'stream-user-7', kind: 'user', text: 'Repeat this' },
      { id: 'stream-user-8', kind: 'user', text: 'Repeat this' },
    ]);
  });
});

describe('hydrateTranscript', () => {
  it('keeps assistant content order and resolves tool outcomes', () => {
    const result = hydrateTranscript([
      { role: 'user', content: 'Inspect the project' },
      {
        role: 'assistant',
        content: [
          { type: 'thinking', thinking: 'I should list it.' },
          {
            type: 'toolCall',
            id: 'call-1',
            name: 'bash',
            arguments: { command: 'ls' },
          },
          { type: 'text', text: 'Done.' },
        ],
      },
      {
        role: 'toolResult',
        toolCallId: 'call-1',
        toolName: 'bash',
        content: [{ type: 'text', text: 'src\ntests' }],
        isError: false,
      },
    ]);

    expect(result.map((entry) => entry.kind)).toEqual([
      'user',
      'thinking',
      'tool',
      'assistant',
    ]);
    expect(result[2]).toMatchObject({
      text: 'ls',
      toolName: 'bash',
      toolRunning: false,
      toolErrored: false,
      toolArguments: '{\n  "command": "ls"\n}',
      toolResult: 'src\ntests',
    });
  });

  it('marks an unmatched tool call in an aborted turn as errored', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        stopReason: 'aborted',
        content: [
          {
            type: 'toolCall',
            id: 'call-aborted',
            name: 'bash',
            arguments: { command: 'sleep 10' },
          },
        ],
      },
    ]);

    expect(result[0]).toMatchObject({
      toolRunning: false,
      toolErrored: true,
    });
  });

  it('marks an unmatched tool call in an errored turn as errored', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        stopReason: 'error',
        content: [
          {
            type: 'toolCall',
            id: 'call-errored',
            name: 'bash',
            arguments: { command: 'sleep 10' },
          },
        ],
      },
    ]);

    expect(result[0]).toMatchObject({
      toolRunning: false,
      toolErrored: true,
    });
  });

  it('lets a later successful tool result override a terminal turn failure', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        stopReason: 'error',
        content: [
          {
            type: 'toolCall',
            id: 'call-recovered',
            name: 'bash',
            arguments: { command: 'echo done' },
          },
        ],
      },
      {
        role: 'toolResult',
        toolCallId: 'call-recovered',
        isError: false,
        content: [{ type: 'text', text: 'done' }],
      },
    ]);

    expect(result[0]).toMatchObject({
      toolRunning: false,
      toolErrored: false,
      toolResult: 'done',
    });
  });

  it('keeps an unmatched tool call running in a nonterminal turn', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        content: [
          {
            type: 'toolCall',
            id: 'call-running',
            name: 'bash',
            arguments: { command: 'sleep 10' },
          },
        ],
      },
    ]);

    expect(result[0]).toMatchObject({
      toolRunning: true,
      toolErrored: false,
    });
  });

  it("carries a bash execution's own output", () => {
    const result = hydrateTranscript([
      {
        role: 'bashExecution',
        command: 'ls',
        output: 'src\ntests',
        exitCode: 0,
      },
    ]);

    expect(result[0]).toMatchObject({
      kind: 'tool',
      text: 'ls',
      toolResult: 'src\ntests',
    });
  });

  it('accepts block-based user content', () => {
    const result = hydrateTranscript([
      { role: 'user', content: [{ type: 'text', text: 'Hello' }] },
    ]);
    expect(result[0]?.text).toBe('Hello');
  });

  it('turns Pi compaction context into a loadable divider', () => {
    const result = hydrateTranscript([
      {
        role: 'compactionSummary',
        summary: 'Earlier work',
        tokensBefore: 5000,
      },
      { role: 'user', content: 'Continue' },
    ]);

    expect(result[0]).toMatchObject({
      kind: 'compaction',
      historyAvailable: true,
      historyLoading: false,
    });
    expect(result[1]?.kind).toBe('user');
  });

  it('keeps a skill invocation and its prompt in one entry', () => {
    const result = hydrateTranscript([
      {
        role: 'user',
        content: [
          {
            type: 'text',
            text: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
References are relative to /skills/pr-review.

# PR review

Review the active branch.
</skill>

Focus on correctness`,
          },
        ],
      },
    ]);

    expect(result).toMatchObject([
      {
        kind: 'skill',
        skillName: 'pr-review',
        skillPrompt: 'Focus on correctness',
        text: `References are relative to /skills/pr-review.

# PR review

Review the active branch.`,
      },
    ]);
  });

  it('carries an optimistic skill id into the expanded message', () => {
    const previous: TranscriptEntry[] = [
      {
        id: 'optimistic-user-1',
        kind: 'skill',
        skillName: 'pr-review',
        text: '',
      },
    ];
    const result = hydrateTranscript(
      [
        {
          role: 'user',
          content: `<skill name="pr-review" location="/skills/pr-review/SKILL.md">
Full instructions
</skill>`,
        },
      ],
      previous,
    );

    expect(result[0]).toMatchObject({
      id: 'optimistic-user-1',
      kind: 'skill',
      skillName: 'pr-review',
      text: 'Full instructions',
    });
  });

  it('carries streamed ids into the settled turn', () => {
    const streamed: TranscriptEntry[] = [
      { id: 'optimistic-1', kind: 'user', text: 'Inspect the project' },
      { id: 'stream-thinking-0', kind: 'thinking', text: 'I should list' },
      {
        id: 'stream-tool-1',
        kind: 'tool',
        text: 'ls',
        toolCallId: 'call-1',
        toolName: 'bash',
        toolRunning: true,
        toolErrored: false,
      },
      { id: 'stream-assistant-2', kind: 'assistant', text: 'Do' },
    ];

    const settled = hydrateTranscript(
      [
        { role: 'user', content: 'Inspect the project' },
        {
          role: 'assistant',
          content: [
            { type: 'thinking', thinking: 'I should list it.' },
            {
              type: 'toolCall',
              id: 'call-1',
              name: 'bash',
              arguments: { command: 'ls' },
            },
            { type: 'text', text: 'Done.' },
          ],
        },
        {
          role: 'toolResult',
          toolCallId: 'call-1',
          toolName: 'bash',
          isError: false,
        },
      ],
      streamed,
    );

    expect(settled.map((entry) => entry.id)).toEqual([
      'optimistic-1',
      'stream-thinking-0',
      'stream-tool-1',
      'stream-assistant-2',
    ]);
    expect(settled[3]?.text).toBe('Done.');
  });

  it('gives rows the settled turn adds ids of their own', () => {
    const streamed: TranscriptEntry[] = [
      { id: 'stream-assistant-0', kind: 'assistant', text: 'Done.' },
    ];

    const settled = hydrateTranscript(
      [
        {
          role: 'assistant',
          content: [
            { type: 'text', text: 'Done.' },
            { type: 'text', text: 'One more thing.' },
          ],
        },
      ],
      streamed,
    );

    expect(settled[0]?.id).toBe('stream-assistant-0');
    expect(settled[1]?.id).not.toBe('stream-assistant-0');
    expect(new Set(settled.map((entry) => entry.id)).size).toBe(2);
  });

  it("keeps a reordered tool row from adopting another call's id", () => {
    const streamed: TranscriptEntry[] = [
      {
        id: 'stream-tool-0',
        kind: 'tool',
        text: 'ls',
        toolCallId: 'call-1',
        toolName: 'bash',
        toolRunning: true,
        toolErrored: false,
      },
    ];

    const settled = hydrateTranscript(
      [
        {
          role: 'assistant',
          content: [
            {
              type: 'toolCall',
              id: 'call-2',
              name: 'bash',
              arguments: { command: 'pwd' },
            },
          ],
        },
      ],
      streamed,
    );

    expect(settled[0]?.id).not.toBe('stream-tool-0');
  });

  /** The shape Pi records when a provider rejects the request outright. */
  it('stands a failed turn in for the reply it replaced', () => {
    const errorMessage = `402: {"message":"Out of credits","code":402}`;
    const result = hydrateTranscript([
      { role: 'user', content: 'hello' },
      {
        role: 'assistant',
        content: [],
        stopReason: 'error',
        errorMessage,
      },
    ]);

    expect(result.map((entry) => entry.kind)).toEqual(['user', 'error']);
    expect(result[1]).toMatchObject({
      errorLabel: 'Reply Failed',
      text: 'The account has no available credit for this request. Add credit or choose another model, then try again.',
    });
    expect(result[1]?.text).not.toContain(errorMessage);
  });

  it('keeps the text of a turn that failed part way through', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'Halfway through' }],
        stopReason: 'error',
        errorMessage: 'terminated',
      },
    ]);

    expect(result.map((entry) => entry.kind)).toEqual(['assistant', 'error']);
    expect(result[0]?.text).toBe('Halfway through');
  });

  it('leaves an aborted turn alone, since the reader stopped it', () => {
    const result = hydrateTranscript([
      {
        role: 'assistant',
        content: [{ type: 'text', text: 'Half a reply' }],
        stopReason: 'aborted',
        errorMessage: 'Aborted',
      },
    ]);

    expect(result.map((entry) => entry.kind)).toEqual(['assistant']);
  });

  it('carries a streamed error row into the settled turn', () => {
    const errorMessage = '429: rate limited';
    const streamed: TranscriptEntry[] = [
      { id: 'user-0', kind: 'user', text: 'hello' },
      {
        id: 'stream-error-0',
        kind: 'error',
        text: 'The model provider is receiving too many requests. Wait a moment or choose another model, then try again.',
        errorLabel: 'Reply Failed',
      },
    ];
    const settled = hydrateTranscript(
      [
        { role: 'user', content: 'hello' },
        { role: 'assistant', content: [], stopReason: 'error', errorMessage },
      ],
      streamed,
    );

    expect(settled[1]?.id).toBe('stream-error-0');
  });
});

describe('historyLayersFromEntries', () => {
  const message = (
    id: string,
    parentId: string | null,
    role: 'user' | 'assistant',
    text: string,
  ): Record<string, unknown> => ({
    type: 'message',
    id,
    parentId,
    message:
      role === 'user'
        ? { role, content: text }
        : { role, content: [{ type: 'text', text }] },
  });

  it('reveals one active-branch compaction layer at a time', () => {
    const entries = [
      message('a', null, 'user', 'root question'),
      message('b', 'a', 'assistant', 'root reply'),
      message('c', 'b', 'user', 'middle question'),
      message('d', 'c', 'assistant', 'middle reply'),
      {
        type: 'compaction',
        id: 'e',
        parentId: 'd',
        firstKeptEntryId: 'c',
        summary: 'first summary',
      },
      message('f', 'e', 'user', 'later question'),
      message('g', 'f', 'assistant', 'later reply'),
      message('h', 'g', 'user', 'current question'),
      message('i', 'h', 'assistant', 'current reply'),
      {
        type: 'compaction',
        id: 'j',
        parentId: 'i',
        firstKeptEntryId: 'h',
        summary: 'latest summary',
      },
      message('k', 'j', 'assistant', 'after compaction'),
    ];

    const layers = historyLayersFromEntries(entries, 'k');
    expect(layers).toHaveLength(2);
    expect(layers[0]?.rows.map((row) => row.text)).toEqual([
      'root question',
      'root reply',
    ]);
    expect(layers[1]?.rows.map((row) => row.text)).toEqual([
      'middle question',
      'middle reply',
      'later question',
      'later reply',
    ]);

    const firstReveal = historyPrefix(layers, 1);
    expect(firstReveal[0]).toMatchObject({
      kind: 'compaction',
      historyAvailable: true,
    });
    expect(firstReveal.slice(1).map((row) => row.text)).toEqual([
      'middle question',
      'middle reply',
      'later question',
      'later reply',
    ]);

    const complete = historyPrefix(layers, 0);
    expect(complete.map((row) => row.kind)).toEqual([
      'user',
      'assistant',
      'compaction',
      'user',
      'assistant',
      'user',
      'assistant',
    ]);
    expect(complete[2]).toMatchObject({
      historyAvailable: false,
      id: layers[1]?.markerId,
    });
  });

  it('ignores entries from an abandoned branch', () => {
    const entries = [
      message('a', null, 'user', 'root'),
      message('abandoned', 'a', 'assistant', 'not active'),
      message('b', 'a', 'assistant', 'active reply'),
      message('c', 'b', 'user', 'kept'),
      {
        type: 'compaction',
        id: 'd',
        parentId: 'c',
        firstKeptEntryId: 'c',
        summary: 'summary',
      },
    ];

    const layers = historyLayersFromEntries(entries, 'd');
    expect(
      layers.flatMap((layer) => layer.rows).map((row) => row.text),
    ).toEqual(['root', 'active reply']);
  });
});

describe('mergeLocalEntries', () => {
  const notice = (id: string, anchor?: number): TranscriptEntry => ({
    id,
    kind: 'notice',
    text: id,
    noticeType: 'info',
    ...(anchor === undefined ? {} : { anchor }),
  });

  it('adds the failures Pi keeps nowhere to the hydrated list', () => {
    const entries = mergeLocalEntries(
      hydrateTranscript([{ role: 'user', content: 'hello' }]),
      [
        {
          key: 4,
          label: 'Conversation Not Shortened',
          text: 'The conversation could not be shortened.',
          anchor: 1,
        },
      ],
      [],
    );

    expect(entries.map((entry) => entry.kind)).toEqual(['user', 'error']);
    expect(entries[1]).toMatchObject({
      id: 'local-error-4',
      errorLabel: 'Conversation Not Shortened',
      text: 'The conversation could not be shortened.',
    });
  });

  it('keeps extension notices that Pi messages cannot carry', () => {
    const carried = notice('extension-notify:1', 1);
    const entries = mergeLocalEntries(
      hydrateTranscript([{ role: 'user', content: 'hello' }]),
      [],
      [carried],
    );

    expect(entries).toEqual([
      expect.objectContaining({ kind: 'user', text: 'hello' }),
      carried,
    ]);
  });

  it('holds a notice at its own spot as the transcript grows past it', () => {
    const entries = mergeLocalEntries(
      hydrateTranscript([
        { role: 'user', content: 'hello' },
        { role: 'assistant', content: [{ type: 'text', text: 'first' }] },
        { role: 'assistant', content: [{ type: 'text', text: 'second' }] },
      ]),
      [],
      [notice('extension-notify:1', 1)],
    );

    expect(entries.map((entry) => entry.text)).toEqual([
      'hello',
      'extension-notify:1',
      'first',
      'second',
    ]);
  });

  it('orders entries sharing a spot by arrival and settles later ones at the end', () => {
    const entries = mergeLocalEntries(
      hydrateTranscript([{ role: 'user', content: 'hello' }]),
      [
        {
          key: 0,
          label: 'Conversation Not Shortened',
          text: 'compaction failed',
          anchor: 1,
        },
      ],
      [notice('extension-notify:1', 0), notice('extension-notify:2', 9)],
    );

    expect(entries.map((entry) => entry.text)).toEqual([
      'extension-notify:1',
      'hello',
      'compaction failed',
      'extension-notify:2',
    ]);
  });

  it('holds a failure with no spot where the first rebuild settled it', () => {
    const failure: LocalError = {
      key: 0,
      label: 'Conversation Not Shortened',
      text: 'compaction failed',
    };
    const settled = mergeLocalEntries(
      hydrateTranscript([{ role: 'user', content: 'hello' }]),
      [failure],
      [],
    );

    expect(settled.map((entry) => entry.text)).toEqual([
      'hello',
      'compaction failed',
    ]);

    const grown = mergeLocalEntries(
      hydrateTranscript([
        { role: 'user', content: 'hello' },
        { role: 'assistant', content: [{ type: 'text', text: 'a reply' }] },
      ]),
      [failure],
      settled,
    );

    expect(grown.map((entry) => entry.text)).toEqual([
      'hello',
      'compaction failed',
      'a reply',
    ]);
  });

  it('drops a notice already carried and never duplicates one', () => {
    const carried = notice('extension-notify:1', 0);
    const entries = mergeLocalEntries(
      hydrateTranscript([]),
      [],
      [carried, carried],
    );

    expect(entries).toEqual([carried]);
  });

  it('adopts ids across a notice so rows below it keep their measured height', () => {
    const messages = [
      { role: 'user', content: 'hello' },
      { role: 'assistant', content: [{ type: 'text', text: 'first' }] },
    ];
    const previous = mergeLocalEntries(
      hydrateTranscript(messages),
      [],
      [notice('extension-notify:1', 1)],
    );

    const settled = hydrateTranscript(
      [
        ...messages,
        { role: 'assistant', content: [{ type: 'text', text: 'second' }] },
      ],
      previous,
    );

    expect(settled.map((entry) => entry.id)).toEqual([
      'user-0',
      'assistant-1',
      'assistant-0',
    ]);
  });
});

describe('parseSkillBlock', () => {
  it('rejects ordinary user text and malformed envelopes', () => {
    expect(parseSkillBlock('Use the review skill')).toBeUndefined();
    expect(
      parseSkillBlock('<skill name="review">missing location</skill>'),
    ).toBeUndefined();
  });
});

describe('toolSummary', () => {
  it('prefers a recognizable path', () => {
    expect(toolSummary({ path: 'src/main.ts' })).toBe('src/main.ts');
  });
});

describe('messageFailure', () => {
  it('reports an errored assistant turn', () => {
    expect(
      messageFailure({
        role: 'assistant',
        stopReason: 'error',
        errorMessage: '402: out of credits',
      }),
    ).toEqual({
      kind: 'credit',
      label: 'Reply Failed',
      message:
        'The account has no available credit for this request. Add credit or choose another model, then try again.',
    });
  });

  it('ignores anything that is not a failed assistant turn', () => {
    expect(messageFailure({ role: 'user', content: 'hello' })).toBeUndefined();
    expect(
      messageFailure({
        role: 'assistant',
        stopReason: 'aborted',
        errorMessage: 'Aborted',
      }),
    ).toBeUndefined();
    expect(messageFailure(undefined)).toBeUndefined();
  });
});
