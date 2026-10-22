import {
  inject,
  nextTick,
  onBeforeUnmount,
  provide,
  watch,
  type InjectionKey,
} from 'vue';

import {
  resolveTranscriptRanges,
  type SearchMatch,
} from '../lib/transcript-search';

interface SearchProps {
  searchMatches?: readonly SearchMatch[];
  activeSearchMatch?: SearchMatch;
  searchOpen?: boolean;
  sessionKey?: string;
}

type RegisterSurface = (id: string, root: HTMLElement) => () => void;
const searchSurfaceKey: InjectionKey<RegisterSurface> =
  Symbol('transcript-search');

/** Opt in only the Markdown content root, never shells, previews or activity. */
function useTranscriptSearchSurface(
  root: () => HTMLElement | undefined,
  entryId: () => string | undefined,
): void {
  const register = inject(searchSurfaceKey, undefined);
  let unregister: (() => void) | undefined;
  watch(
    () => [root(), entryId()] as const,
    ([element, id]) => {
      unregister?.();
      unregister = element && id ? register?.(id, element) : undefined;
    },
    { flush: 'post', immediate: true },
  );
  onBeforeUnmount(() => unregister?.());
}

function useTranscriptSearch(
  props: SearchProps,
  navigation: {
    viewport: () => HTMLElement | undefined;
    indexOf: (id: string) => number;
    disengage: () => void;
    prepareReveal?: (clearance: number) => Promise<void>;
    cancelScroll: () => void;
    scrollToIndex: (index: number) => void;
    measure: (root: HTMLElement) => void;
    scrollToOffset: (offset: number) => void;
  },
): {
  revealSearchMatch: (match: SearchMatch) => Promise<void>;
  cancelSearchNavigation: () => void;
  isNavigating: () => boolean;
} {
  const surfaces = new Map<string, HTMLElement>();
  const owned = new Map<string, Highlight>();
  let generation = 0;
  let navigating = false;
  let disposed = false;
  let paintQueued = false;

  function clearHighlights(): void {
    if (typeof CSS === 'undefined' || !CSS.highlights) return;
    for (const [name, highlight] of owned) {
      if (CSS.highlights.get(name) === highlight) CSS.highlights.delete(name);
    }
    owned.clear();
  }

  function paint(): void {
    clearHighlights();
    if (
      disposed ||
      !props.searchOpen ||
      typeof CSS === 'undefined' ||
      !CSS.highlights ||
      typeof Highlight === 'undefined'
    )
      return;
    const grouped = new Map<string, SearchMatch[]>();
    for (const match of props.searchMatches ?? []) {
      const group = grouped.get(match.entryId) ?? [];
      group.push(match);
      grouped.set(match.entryId, group);
    }
    const ranges: Range[] = [];
    for (const [id, matches] of grouped) {
      const root = surfaces.get(id);
      if (root?.isConnected)
        ranges.push(...resolveTranscriptRanges(root, matches));
    }
    const active = props.activeSearchMatch;
    const root = active && surfaces.get(active.entryId);
    const activeRanges =
      root?.isConnected && active
        ? resolveTranscriptRanges(root, [active])
        : [];
    for (const [name, values, priority] of [
      ['tau-transcript-matches', ranges, 0],
      ['tau-transcript-active', activeRanges, 1],
    ] as const) {
      const highlight = new Highlight(...values);
      highlight.priority = priority;
      CSS.highlights.set(name, highlight);
      owned.set(name, highlight);
    }
  }

  function schedulePaint(): void {
    if (paintQueued || disposed) return;
    paintQueued = true;
    void nextTick(() => {
      paintQueued = false;
      paint();
    });
  }

  provide(searchSurfaceKey, (id, root) => {
    surfaces.set(id, root);
    const observer = new MutationObserver(schedulePaint);
    observer.observe(root, {
      childList: true,
      characterData: true,
      subtree: true,
    });
    schedulePaint();
    return (): void => {
      observer.disconnect();
      if (surfaces.get(id) === root) surfaces.delete(id);
      schedulePaint();
    };
  });

  function cancelSearchNavigation(): void {
    generation++;
    // Replace the virtualizer's pending index reconciliation with a fixed offset.
    if (navigating) navigation.cancelScroll();
    navigating = false;
  }

  watch(
    () => [props.searchMatches, props.activeSearchMatch, props.searchOpen],
    schedulePaint,
    { flush: 'post' },
  );
  watch(() => props.sessionKey, cancelSearchNavigation, { flush: 'sync' });
  watch(
    () => props.searchOpen,
    (open) => {
      if (!open) {
        cancelSearchNavigation();
        clearHighlights();
      }
    },
    { flush: 'sync' },
  );

  async function revealSearchMatch(match: SearchMatch): Promise<void> {
    cancelSearchNavigation();
    if (disposed || !props.searchOpen || navigation.indexOf(match.entryId) < 0)
      return;
    const revision = generation;
    navigating = true;
    navigation.disengage();
    const current = (): boolean => !disposed && revision === generation;
    try {
      if (navigation.prepareReveal) await navigation.prepareReveal(66);
      if (!current()) return;
      // Resolve identity afresh after each measurement: prepends can change indices.
      navigation.scrollToIndex(navigation.indexOf(match.entryId));
      let settled = 0;
      for (let attempt = 0; attempt < 16 && current(); attempt++) {
        await nextTick();
        await new Promise<void>((resolve) =>
          requestAnimationFrame(() => resolve()),
        );
        if (!current()) return;
        const viewport = navigation.viewport();
        const root = surfaces.get(match.entryId);
        if (!viewport) return;
        if (!root?.isConnected) {
          const index = navigation.indexOf(match.entryId);
          if (index < 0) return;
          navigation.scrollToIndex(index);
          continue;
        }
        navigation.measure(root);
        const range = resolveTranscriptRanges(root, [match])[0];
        const rect = range?.getClientRects()[0];
        if (!range || !rect) continue;
        const bounds = viewport.getBoundingClientRect();
        const top = bounds.top + 66;
        const bottom = bounds.bottom - 12;
        const delta =
          rect.top < top || rect.height > bottom - top
            ? rect.top - top
            : Math.max(0, rect.bottom - bottom);
        if (Math.abs(delta) > 1) {
          navigation.scrollToOffset(viewport.scrollTop + delta);
          settled = 0;
        } else {
          settled++;
        }
        // Reveal inner scrollers first, without scrolling ancestors outside the message.
        for (
          let container = range.startContainer.parentElement;
          container && root.contains(container);
          container = container.parentElement
        ) {
          if (
            container.scrollWidth <= container.clientWidth ||
            !/^(auto|scroll)$/.test(getComputedStyle(container).overflowX)
          )
            continue;
          const matchBounds = range.getClientRects()[0];
          if (!matchBounds) continue;
          const containerBounds = container.getBoundingClientRect();
          if (matchBounds.left < containerBounds.left + 12)
            container.scrollLeft +=
              matchBounds.left - containerBounds.left - 12;
          else if (matchBounds.right > containerBounds.right - 12)
            container.scrollLeft += Math.min(
              matchBounds.left - containerBounds.left - 12,
              matchBounds.right - containerBounds.right + 12,
            );
        }
        if (settled >= 2) break;
      }
    } finally {
      if (current()) navigating = false;
    }
  }

  onBeforeUnmount(() => {
    disposed = true;
    cancelSearchNavigation();
    clearHighlights();
    surfaces.clear();
  });

  return {
    revealSearchMatch,
    cancelSearchNavigation,
    isNavigating: () => navigating,
  };
}

export { useTranscriptSearch, useTranscriptSearchSurface };
