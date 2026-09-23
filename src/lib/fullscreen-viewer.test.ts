import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

let events: Event[];
let menuOpen: boolean;

beforeEach(() => {
  events = [];
  menuOpen = false;
  vi.stubGlobal('window', {
    dispatchEvent(event: Event) {
      events.push(event);
      return true;
    },
  });
  vi.stubGlobal('document', {
    querySelector: () => (menuOpen ? {} : null),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

async function viewerStack(): Promise<typeof import('./fullscreen-viewer')> {
  return import('./fullscreen-viewer');
}

describe('fullscreen viewer stack', () => {
  it('keeps the aggregate open state until the final viewer closes', async () => {
    const { registerFullscreenViewer, unregisterFullscreenViewer } =
      await viewerStack();
    const first = registerFullscreenViewer();
    const second = registerFullscreenViewer();

    unregisterFullscreenViewer(first);
    unregisterFullscreenViewer(second);

    expect(
      events.map((event) => (event as CustomEvent<boolean>).detail),
    ).toEqual([true, false]);
  });

  it('gives Escape ownership to only the newest viewer', async () => {
    const {
      isTopmostFullscreenViewer,
      registerFullscreenViewer,
      unregisterFullscreenViewer,
    } = await viewerStack();
    const first = registerFullscreenViewer();
    const second = registerFullscreenViewer();

    expect(isTopmostFullscreenViewer(first)).toBe(false);
    expect(isTopmostFullscreenViewer(second)).toBe(true);

    unregisterFullscreenViewer(second);

    expect(isTopmostFullscreenViewer(first)).toBe(true);
  });

  it('recognizes an open portal menu', async () => {
    const { isUiMenuOpen } = await viewerStack();

    expect(isUiMenuOpen()).toBe(false);
    menuOpen = true;
    expect(isUiMenuOpen()).toBe(true);
  });
});
