import { afterEach, describe, expect, it } from 'vitest';

import {
  DEFAULT_SIDEBAR_WIDTH,
  loadSidebarWidth,
  persistSidebarWidth,
  setSidebarWidthStorage,
  type SidebarWidthStorage,
} from './sidebar-width';

function memoryStorage(initial?: string): {
  storage: SidebarWidthStorage;
  value: () => string | null;
} {
  let stored = initial ?? null;
  return {
    storage: {
      getItem: (): string | null => stored,
      setItem: (_key, value): void => {
        stored = value;
      },
    },
    value: (): string | null => stored,
  };
}

afterEach(() => setSidebarWidthStorage());

describe('sidebar width storage', () => {
  it('can use isolated storage without touching browser persistence', () => {
    const first = memoryStorage();
    const second = memoryStorage();

    setSidebarWidthStorage(first.storage);
    expect(loadSidebarWidth()).toBe(DEFAULT_SIDEBAR_WIDTH);
    persistSidebarWidth(410);
    expect(first.value()).toBe('410');

    setSidebarWidthStorage(second.storage);
    expect(loadSidebarWidth()).toBe(DEFAULT_SIDEBAR_WIDTH);
    expect(second.value()).toBeNull();
  });

  it('clamps values loaded through the storage seam', () => {
    setSidebarWidthStorage(memoryStorage('999').storage);
    expect(loadSidebarWidth()).toBe(480);
  });
});
