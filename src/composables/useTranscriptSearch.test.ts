/* eslint-disable vue/one-component-per-file -- minimal parent/provider fixture */
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createRenderer,
  defineComponent,
  h,
  nextTick,
  reactive,
  type VNode,
} from 'vue';

import { resolveTranscriptRanges } from '../lib/transcript-search';

import {
  useTranscriptSearch,
  useTranscriptSearchSurface,
} from './useTranscriptSearch';

vi.mock('../lib/transcript-search', () => ({
  resolveTranscriptRanges: vi.fn(() => []),
}));

const renderer = createRenderer<object, object>({
  createElement: () => ({}),
  createText: () => ({}),
  createComment: () => ({}),
  insert: () => {},
  remove: () => {},
  setText: () => {},
  setElementText: () => {},
  parentNode: () => null,
  nextSibling: () => null,
  patchProp: () => {},
});
const match = { entryId: 'reply', start: 0, end: 4 };

function setup(): {
  api: ReturnType<typeof useTranscriptSearch>;
  props: {
    searchOpen: boolean;
    searchMatches: (typeof match)[];
    sessionKey: string;
  };
  scrollToIndex: ReturnType<typeof vi.fn>;
  scrollToOffset: ReturnType<typeof vi.fn>;
  highlights: Map<string, unknown>;
  frames: (() => void)[];
  mutations: (() => void)[];
  root: HTMLElement;
  unmount: () => void;
} {
  const highlights = new Map<string, unknown>();
  const frames: (() => void)[] = [];
  const mutations: (() => void)[] = [];
  vi.stubGlobal('CSS', { highlights });
  vi.stubGlobal(
    'Highlight',
    class {
      priority = 0;
    },
  );
  vi.stubGlobal('requestAnimationFrame', (callback: () => void) =>
    frames.push(callback),
  );
  vi.stubGlobal(
    'MutationObserver',
    class {
      constructor(callback: () => void) {
        mutations.push(callback);
      }
      observe(): void {}
      disconnect(): void {}
    },
  );
  const root = { isConnected: true } as HTMLElement;
  const props = reactive({
    searchOpen: true,
    searchMatches: [match],
    sessionKey: 'one',
  });
  const scrollToIndex = vi.fn();
  const scrollToOffset = vi.fn();
  let api!: ReturnType<typeof useTranscriptSearch>;
  const Child = defineComponent({
    setup() {
      useTranscriptSearchSurface(
        () => root,
        () => match.entryId,
      );
      return (): VNode => h('div');
    },
  });
  const app = renderer.createApp(
    defineComponent({
      setup() {
        api = useTranscriptSearch(props, {
          viewport: () =>
            ({
              scrollTop: 0,
              getBoundingClientRect: () => ({ top: 0, bottom: 600 }),
            }) as HTMLElement,
          indexOf: () => 12,
          disengage: vi.fn(),
          cancelScroll: vi.fn(),
          scrollToIndex,
          measure: vi.fn(),
          scrollToOffset,
        });
        return (): VNode => h(Child);
      },
    }),
  );
  app.mount({});
  return {
    api,
    props,
    scrollToIndex,
    scrollToOffset,
    highlights,
    frames,
    mutations,
    root,
    unmount: () => app.unmount(),
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('transcript search surfaces', () => {
  it('resolves only the opted-in Markdown root and repaints DOM replacements without navigating', async () => {
    const test = setup();
    await nextTick();
    expect(resolveTranscriptRanges).toHaveBeenCalledWith(test.root, [match]);
    vi.mocked(resolveTranscriptRanges).mockClear();
    test.mutations[0]!();
    await nextTick();
    expect(resolveTranscriptRanges).toHaveBeenCalledWith(test.root, [match]);
    test.props.searchMatches = [{ ...match, end: 3 }];
    await nextTick();
    await nextTick();
    expect(test.scrollToIndex).not.toHaveBeenCalled();
    expect(test.scrollToOffset).not.toHaveBeenCalled();
    test.unmount();
  });

  it('removes only owned highlights on close and unmount', async () => {
    const test = setup();
    await nextTick();
    const foreign = {};
    test.highlights.set('unrelated', foreign);
    test.highlights.set('tau-transcript-active', foreign);
    test.props.searchOpen = false;
    expect(test.highlights.has('tau-transcript-matches')).toBe(false);
    expect(test.highlights.get('tau-transcript-active')).toBe(foreign);
    test.unmount();
    expect(test.highlights.get('unrelated')).toBe(foreign);
  });

  it('refines range geometry inside the viewport and reveals code using pre.scrollLeft', async () => {
    const test = setup();
    const pre = {
      scrollLeft: 0,
      getBoundingClientRect: (): { left: number; right: number } => ({
        left: 0,
        right: 300,
      }),
    };
    Object.assign(test.root, { contains: () => true });
    vi.mocked(resolveTranscriptRanges).mockReturnValue([
      {
        startContainer: { parentElement: { closest: () => pre } },
        getClientRects: () => [
          { top: 20, bottom: 40, height: 20, left: 500, right: 540 },
        ],
      } as unknown as Range,
    ]);
    const pending = test.api.revealSearchMatch(match);
    await nextTick();
    test.frames.shift()!();
    await nextTick();
    expect(test.scrollToOffset).toHaveBeenCalledWith(-46);
    expect(pre.scrollLeft).toBe(252);
    test.api.cancelSearchNavigation();
    await nextTick();
    test.frames.shift()?.();
    await pending;
    test.unmount();
    vi.mocked(resolveTranscriptRanges).mockReturnValue([]);
  });

  it.each(['cancel', 'session', 'close', 'unmount'] as const)(
    'drops stale navigation after %s while waiting for a virtual row',
    async (reason) => {
      const test = setup();
      const pending = test.api.revealSearchMatch(match);
      expect(test.scrollToIndex).toHaveBeenCalledWith(12);
      await nextTick();
      if (reason === 'cancel') test.api.cancelSearchNavigation();
      if (reason === 'session') test.props.sessionKey = 'two';
      if (reason === 'close') test.props.searchOpen = false;
      if (reason === 'unmount') test.unmount();
      test.frames.shift()!();
      await pending;
      expect(test.scrollToOffset).not.toHaveBeenCalled();
      expect(test.api.isNavigating()).toBe(false);
      if (reason !== 'unmount') test.unmount();
    },
  );
});
