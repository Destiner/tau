import {
  computed,
  onScopeDispose,
  shallowReactive,
  shallowRef,
  toValue,
  watch,
  type MaybeRefOrGetter,
  type ComputedRef,
} from 'vue';

import type { TranscriptEntry } from '../lib/pi/transcript';
import {
  TranscriptSearchIndex,
  type SearchMatch,
  type TranscriptIndexOptions,
} from '../lib/transcript-search';

interface TranscriptFindController {
  key: string;
  sessionId: string;
  sessionOwnerSequence: number;
  disposed: boolean;
}

interface TranscriptFindInputs {
  activeController: MaybeRefOrGetter<
    TranscriptFindController | null | undefined
  >;
  controllers: MaybeRefOrGetter<readonly TranscriptFindController[]>;
  /** The displayed transcript, including optimistic rows, not controller.messages. */
  messages: MaybeRefOrGetter<readonly TranscriptEntry[]>;
  /** Optional scheduling controls; defaults to the index's 32-row event-loop batches. */
  indexing?: Pick<TranscriptIndexOptions, 'batchSize' | 'yieldControl'>;
}

interface TranscriptFindState {
  readonly query: string;
  readonly open: boolean;
  readonly matches: readonly SearchMatch[];
  readonly activeMatch: SearchMatch | null;
  readonly pending: boolean;
  readonly busy: boolean;
}

interface LocalState {
  identity: string;
  query: string;
  open: boolean;
  matches: SearchMatch[];
  active: number;
  pending: boolean;
  busy: boolean;
  index?: TranscriptSearchIndex;
}

const empty: TranscriptFindState = {
  query: '',
  open: false,
  matches: [],
  activeMatch: null,
  pending: false,
  busy: false,
};

function identity(controller: TranscriptFindController): string {
  return JSON.stringify([
    controller.sessionId,
    controller.sessionOwnerSequence,
  ]);
}

/**
 * Scroll on revealRequest, not activeMatch: only query edits and explicit
 * navigation request a reveal. Ignore requests for inactive controllers.
 * Streaming, hydration, and session restoration must not move the reader.
 */
interface TranscriptFindApi {
  current: ComputedRef<TranscriptFindState>;
  revealRequest: ComputedRef<{
    sequence: number;
    key: string;
    match: SearchMatch;
  } | null>;
  open(): void;
  close(): void;
  setQuery(query: string): void;
  navigate(delta: number): void;
}

function useTranscriptFind(inputs: TranscriptFindInputs): TranscriptFindApi {
  const locals = new Map<string, LocalState>();
  const selected = shallowRef<LocalState>();
  const revealRequest = shallowRef<{
    sequence: number;
    key: string;
    match: SearchMatch;
  } | null>(null);
  let sequence = 0;
  let abort: AbortController | undefined;
  let busyTimer: ReturnType<typeof setTimeout> | undefined;
  let revision = 0;
  let revealPending: LocalState | undefined;

  function cancel(pending = false): void {
    revision++;
    abort?.abort();
    abort = undefined;
    clearTimeout(busyTimer);
    if (selected.value) {
      selected.value.pending = pending;
      selected.value.busy = false;
    }
  }

  function release(local: LocalState): void {
    local.index?.clear();
    if (revealPending === local) revealPending = undefined;
  }

  // Read identities even for inactive controllers so replacement prunes promptly.
  const stopOwners = watch(
    () => ({
      active: toValue(inputs.activeController),
      owners: toValue(inputs.controllers).map((controller) => ({
        key: controller.key,
        identity: identity(controller),
        disposed: controller.disposed,
      })),
      activeIdentity: toValue(inputs.activeController)
        ? identity(toValue(inputs.activeController)!)
        : undefined,
      disposed: toValue(inputs.activeController)?.disposed,
    }),
    ({ active, owners }) => {
      const live = new Map(
        owners
          .filter((owner) => !owner.disposed)
          .map((owner) => [owner.key, owner.identity]),
      );
      for (const [key, local] of locals) {
        if (live.get(key) !== local.identity) {
          release(local);
          locals.delete(key);
        }
      }
      let next: LocalState | undefined;
      if (
        active &&
        !active.disposed &&
        live.get(active.key) === identity(active)
      ) {
        next = locals.get(active.key);
        if (!next) {
          next = shallowReactive({
            identity: identity(active),
            query: '',
            open: false,
            matches: [],
            active: -1,
            pending: false,
            busy: false,
          });
          locals.set(active.key, next);
        }
      }
      if (selected.value !== next) {
        cancel();
        revealPending = undefined;
        if (next) next.pending = next.open && !!next.query.trim();
        selected.value = next;
      }
    },
    { immediate: true, flush: 'sync' },
  );

  function reveal(local: LocalState): void {
    const match = local.matches[local.active];
    const controller = toValue(inputs.activeController);
    if (match && controller && selected.value === local) {
      revealRequest.value = {
        sequence: ++sequence,
        key: controller.key,
        match,
      };
    }
  }

  const stopSearch = watch(
    () => {
      const local = selected.value;
      return {
        local,
        query: local?.query ?? '',
        open: local?.open ?? false,
        // Snapshot eligible text, not just array/row identity. This also keeps
        // async work independent of subsequent in-place streaming mutations.
        entries:
          local?.open && local.query.trim()
            ? toValue(inputs.messages)
                .filter(
                  (entry) =>
                    entry.kind === 'user' || entry.kind === 'assistant',
                )
                .map(({ id, kind, text }) => ({ id, kind, text }))
            : [],
      };
    },
    async ({ local, query, open, entries }) => {
      cancel(!!local && open && !!query.trim());
      if (!local || !open || !query.trim()) {
        if (local && !query.trim()) {
          local.matches = [];
          local.active = -1;
        }
        return;
      }
      const ticket = revision;
      const signal = (abort = new AbortController()).signal;
      local.index ??= new TranscriptSearchIndex();
      busyTimer = setTimeout(() => {
        if (ticket === revision) local.busy = true;
      }, 150);
      try {
        const updated = await local.index.updateAsync(entries, {
          ...inputs.indexing,
          signal,
        });
        if (!updated || ticket !== revision || selected.value !== local) return;
        const matches = local.index.search(query);
        const previous = local.matches[local.active];
        const exact = previous
          ? matches.findIndex(
              (match) =>
                match.entryId === previous.entryId &&
                match.start === previous.start &&
                match.end === previous.end,
            )
          : -1;
        // If the old occurrence vanished, clamp its ordinal to the new list.
        const active =
          revealPending === local
            ? 0
            : exact >= 0
              ? exact
              : Math.max(0, Math.min(local.active, matches.length - 1));
        local.matches = matches;
        local.active = matches.length ? active : -1;
        local.pending = false;
        if (revealPending === local) {
          revealPending = undefined;
          reveal(local);
        }
      } finally {
        if (ticket === revision) {
          clearTimeout(busyTimer);
          local.pending = false;
          local.busy = false;
        }
      }
    },
    { immediate: true },
  );

  const current = computed<TranscriptFindState>(() => {
    const local = selected.value;
    return local
      ? {
          query: local.query,
          open: local.open,
          matches: local.matches,
          activeMatch: local.matches[local.active] ?? null,
          pending: local.pending,
          busy: local.busy,
        }
      : empty;
  });

  function open(): void {
    const local = selected.value;
    if (!local || local.open) return;
    local.pending = !!local.query.trim();
    local.open = true;
  }
  function close(): void {
    cancel();
    revealPending = undefined;
    if (selected.value) selected.value.open = false;
  }
  function setQuery(query: string): void {
    const local = selected.value;
    if (!local || local.query === query) return;
    cancel(local.open && !!query.trim());
    local.query = query;
    local.matches = [];
    local.active = -1;
    revealPending = local.open && query.trim() ? local : undefined;
  }
  function navigate(delta: number): void {
    const local = selected.value;
    if (
      !local?.open ||
      local.pending ||
      !local.matches.length ||
      !Number.isSafeInteger(delta)
    )
      return;
    local.active =
      (((local.active + delta) % local.matches.length) + local.matches.length) %
      local.matches.length;
    revealPending = undefined;
    reveal(local);
  }

  onScopeDispose(() => {
    stopSearch();
    stopOwners();
    cancel();
    for (const local of locals.values()) release(local);
    locals.clear();
  });

  return {
    current,
    revealRequest: computed(() => revealRequest.value),
    open,
    close,
    setQuery,
    navigate,
  };
}

export { useTranscriptFind };
export type {
  TranscriptFindController,
  TranscriptFindInputs,
  TranscriptFindState,
  TranscriptFindApi,
};
