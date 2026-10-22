import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  effectScope,
  nextTick,
  reactive,
  ref,
  watch,
  type Ref,
  type EffectScope,
} from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';
import { TranscriptSearchIndex } from '../lib/transcript-search';

import {
  useTranscriptFind,
  type TranscriptFindController,
} from './useTranscriptFind';

const constructed = vi.hoisted(() => vi.fn());
vi.mock('../lib/transcript-search', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('../lib/transcript-search')>();
  return {
    ...actual,
    TranscriptSearchIndex: class extends actual.TranscriptSearchIndex {
      constructor() {
        super((text) => text);
        constructed();
      }
    },
  };
});

const cleanups: (() => void)[] = [];
afterEach(() => {
  cleanups.splice(0).forEach((cleanup) => cleanup());
  vi.restoreAllMocks();
  vi.useRealTimers();
  constructed.mockClear();
});

function setup(indexing?: {
  batchSize: number;
  yieldControl: () => Promise<void>;
}): {
  api: ReturnType<typeof useTranscriptFind>;
  owners: TranscriptFindController[];
  active: Ref<TranscriptFindController | null>;
  messages: Ref<TranscriptEntry[]>;
  scope: EffectScope;
} {
  const owners = reactive<TranscriptFindController[]>([
    { key: 'a', sessionId: 'one', sessionOwnerSequence: 1, disposed: false },
    { key: 'b', sessionId: 'two', sessionOwnerSequence: 2, disposed: false },
  ]);
  const active = ref<TranscriptFindController | null>(owners[0]!);
  const messages = ref<TranscriptEntry[]>([
    { id: 'reply', kind: 'assistant', text: 'cat cat dog' },
  ]);
  const scope = effectScope();
  const api = scope.run(() =>
    useTranscriptFind({
      activeController: active,
      controllers: () => owners,
      messages: () => messages.value,
      indexing,
    }),
  )!;
  cleanups.push(() => scope.stop());
  return { api, owners, active, messages, scope };
}

async function settle(): Promise<void> {
  await nextTick();
  await Promise.resolve();
  await nextTick();
}

async function search(
  api: ReturnType<typeof useTranscriptFind>,
  query = 'cat',
): Promise<void> {
  api.open();
  api.setQuery(query);
  await settle();
}

describe('useTranscriptFind', () => {
  it('is lazy, selects the first result, wraps navigation, and retains state on close', async () => {
    const { api } = setup();
    expect(constructed).not.toHaveBeenCalled();
    api.open();
    await settle();
    expect(constructed).not.toHaveBeenCalled();
    await search(api);
    expect(constructed).toHaveBeenCalledTimes(1);
    expect(api.current.value.matches).toHaveLength(2);
    expect(api.current.value.activeMatch?.start).toBe(0);
    expect(api.revealRequest.value?.sequence).toBe(1);
    api.navigate(-1);
    expect(api.current.value.activeMatch?.start).toBe(4);
    api.navigate(3);
    expect(api.current.value.activeMatch?.start).toBe(0);
    api.close();
    await settle();
    expect(api.current.value).toMatchObject({
      query: 'cat',
      open: false,
      pending: false,
      busy: false,
    });
    const request = api.revealRequest.value;
    api.open();
    await settle();
    expect(api.revealRequest.value).toBe(request);
    expect(constructed).toHaveBeenCalledTimes(1);
  });

  it('isolates controllers and restores independently without revealing', async () => {
    const { api, active, owners, messages } = setup();
    await search(api);
    api.navigate(1);
    const request = api.revealRequest.value;
    active.value = owners[1]!;
    messages.value = [
      { id: 'other', kind: 'user', text: 'dog dog dog', pending: true },
    ];
    await settle();
    expect(api.current.value).toMatchObject({
      query: '',
      open: false,
      matches: [],
    });
    await search(api, 'dog');
    api.close();
    const last = api.revealRequest.value;
    active.value = owners[0]!;
    messages.value = [{ id: 'reply', kind: 'assistant', text: 'cat cat dog' }];
    await settle();
    expect(api.current.value).toMatchObject({ query: 'cat', open: true });
    expect(api.current.value.activeMatch?.start).toBe(4);
    expect(api.revealRequest.value).toBe(last);
    expect(last!.sequence).toBeGreaterThan(request!.sequence);
    active.value = owners[1]!;
    await settle();
    expect(api.current.value).toMatchObject({ query: 'dog', open: false });
  });

  it('tracks eligible in-place text and hydration, preserving exact identity or clamping ordinal without reveal', async () => {
    const { api, messages } = setup();
    await search(api);
    api.navigate(1);
    const request = api.revealRequest.value;
    messages.value[0]!.text += ' cat';
    await settle();
    expect(api.current.value.matches).toHaveLength(3);
    expect(api.current.value.activeMatch?.start).toBe(4);
    messages.value.unshift({
      id: 'optimistic',
      kind: 'user',
      text: 'cat',
      pending: true,
    });
    await settle();
    expect(api.current.value.activeMatch).toMatchObject({
      entryId: 'reply',
      start: 4,
    });
    messages.value = [{ id: 'hydrated', kind: 'assistant', text: 'cat cat' }];
    await settle();
    expect(api.current.value.activeMatch).toMatchObject({
      entryId: 'hydrated',
      start: 4,
    });
    messages.value[0]!.kind = 'thinking';
    await settle();
    expect(api.current.value.matches).toEqual([]);
    expect(api.current.value.activeMatch).toBeNull();
    expect(api.revealRequest.value).toBe(request);
  });

  it('resets on either identity component and prunes removed/disposed inactive owners', async () => {
    const clear = vi.spyOn(TranscriptSearchIndex.prototype, 'clear');
    const { api, owners, active } = setup();
    await search(api);
    owners[0]!.sessionOwnerSequence++;
    await settle();
    expect(api.current.value.query).toBe('');
    expect(clear).toHaveBeenCalledTimes(1);
    await search(api);
    owners[0]!.sessionId = 'replacement';
    await settle();
    expect(api.current.value.query).toBe('');
    await search(api);
    const old = owners[0]!;
    active.value = owners[1]!;
    old.disposed = true;
    await settle();
    expect(clear).toHaveBeenCalledTimes(3);
    old.disposed = false;
    active.value = old;
    await settle();
    expect(api.current.value.query).toBe('');
    await search(api);
    owners.splice(0, 1);
    await settle();
    expect(api.current.value).toMatchObject({ query: '', open: false });
    expect(clear).toHaveBeenCalledTimes(4);
  });

  it('publishes atomic counts, delays busy 150ms, and drops superseded query work', async () => {
    vi.useFakeTimers();
    const releases: (() => void)[] = [];
    const { api, messages } = setup({
      batchSize: 1,
      yieldControl: () => new Promise((resolve) => releases.push(resolve)),
    });
    messages.value.push({ id: 'second', kind: 'user', text: 'dog' });
    await search(api);
    expect(api.current.value.matches).toEqual([]);
    expect(api.current.value.pending).toBe(true);
    expect(releases).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(149);
    expect(api.current.value.busy).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(api.current.value.busy).toBe(true);
    // Cached projections no longer yield. A third row forces the replacement
    // update to pause on the uncached second row while the old update is held.
    messages.value.push({ id: 'third', kind: 'user', text: 'bird' });
    api.setQuery('dog');
    expect(api.current.value).toMatchObject({ pending: true, busy: false });
    await settle();
    expect(releases).toHaveLength(2);
    releases.shift()!();
    await settle();
    expect(api.current.value.matches).toEqual([]);
    expect(api.revealRequest.value).toBeNull();
    expect(api.current.value.pending).toBe(true);
    releases.shift()!();
    await settle();
    expect(api.current.value.matches).toHaveLength(2);
    expect(api.current.value.pending).toBe(false);
    expect(api.current.value.busy).toBe(false);
    expect(api.revealRequest.value?.sequence).toBe(1);
  });

  it('marks queries pending synchronously, including cached searches, and clears pending on blank queries', async () => {
    const { api } = setup();
    await search(api);
    const request = api.revealRequest.value;
    const snapshots: { query: string; pending: boolean; count: number }[] = [];
    const stop = watch(
      api.current,
      (state) => {
        snapshots.push({
          query: state.query,
          pending: state.pending,
          count: state.matches.length,
        });
      },
      { flush: 'sync' },
    );
    api.setQuery('dog');
    expect(api.current.value).toMatchObject({
      query: 'dog',
      pending: true,
      busy: false,
      matches: [],
    });
    api.navigate(1);
    expect(api.revealRequest.value).toBe(request);
    await settle();
    expect(api.current.value).toMatchObject({ pending: false, busy: false });
    expect(api.current.value.matches).toHaveLength(1);
    expect(
      snapshots.filter((state) => state.query === 'dog'),
    ).not.toContainEqual({
      query: 'dog',
      pending: false,
      count: 0,
    });
    stop();

    api.setQuery('absent');
    expect(api.current.value.pending).toBe(true);
    await settle();
    expect(api.current.value).toMatchObject({ pending: false, matches: [] });
    api.setQuery('cat');
    expect(api.current.value.pending).toBe(true);
    api.setQuery('   ');
    expect(api.current.value).toMatchObject({ pending: false, busy: false });
    await settle();
    expect(api.current.value.matches).toEqual([]);
  });

  it('blocks navigation during pending refreshes before busy appears without passive reveal', async () => {
    const releases: (() => void)[] = [];
    const { api, messages } = setup({
      batchSize: 1,
      yieldControl: () => new Promise((resolve) => releases.push(resolve)),
    });
    await search(api);
    const request = api.revealRequest.value;
    const activeMatch = api.current.value.activeMatch;
    messages.value[0]!.text += ' cat';
    messages.value.push({ id: 'second', kind: 'user', text: 'cat' });
    await settle();
    expect(api.current.value).toMatchObject({ pending: true, busy: false });
    expect(api.current.value.matches).toHaveLength(2);
    api.navigate(1);
    expect(api.current.value.activeMatch).toBe(activeMatch);
    expect(api.revealRequest.value).toBe(request);
    expect(releases).toHaveLength(1);
    releases.shift()!();
    await settle();
    expect(api.current.value).toMatchObject({ pending: false, busy: false });
    expect(api.current.value.matches).toHaveLength(4);
    expect(api.revealRequest.value).toBe(request);
    api.navigate(1);
    expect(api.revealRequest.value?.sequence).toBe(request!.sequence + 1);
  });

  it.each(['switch', 'replace', 'close', 'dispose'] as const)(
    'cancels cold work on %s',
    async (action) => {
      const releases: (() => void)[] = [];
      const { api, messages, active, owners, scope } = setup({
        batchSize: 1,
        yieldControl: () => new Promise((resolve) => releases.push(resolve)),
      });
      messages.value.push({ id: 'second', kind: 'user', text: 'cat' });
      await search(api);
      expect(api.current.value.pending).toBe(true);
      if (action === 'switch') active.value = owners[1]!;
      if (action === 'replace') owners[0]!.sessionOwnerSequence++;
      if (action === 'close') api.close();
      if (action === 'dispose') scope.stop();
      expect(api.current.value.pending).toBe(false);
      await settle();
      releases.shift()!();
      await settle();
      expect(api.current.value.matches).toEqual([]);
      expect(api.current.value.busy).toBe(false);
      expect(api.current.value.pending).toBe(false);
      expect(api.revealRequest.value).toBeNull();
    },
  );
});
