const SIDEBAR_WIDTH_STORAGE_KEY = 'tau.sidebar-width';
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 480;
const DEFAULT_SIDEBAR_WIDTH = 260;

interface SidebarWidthStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

let storageOverride: SidebarWidthStorage | undefined;

function sidebarWidthStorage(): SidebarWidthStorage {
  return storageOverride ?? localStorage;
}

function setSidebarWidthStorage(storage?: SidebarWidthStorage): void {
  storageOverride = storage;
}

function loadSidebarWidth(): number {
  try {
    const storedWidth = Number.parseFloat(
      sidebarWidthStorage().getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? '',
    );
    if (Number.isFinite(storedWidth)) return clampSidebarWidth(storedWidth);
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
  return DEFAULT_SIDEBAR_WIDTH;
}

function clampSidebarWidth(width: number): number {
  return Math.min(
    MAX_SIDEBAR_WIDTH,
    Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)),
  );
}

function persistSidebarWidth(width: number): void {
  try {
    sidebarWidthStorage().setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(width));
  } catch {
    return;
  }
}

export {
  MIN_SIDEBAR_WIDTH,
  MAX_SIDEBAR_WIDTH,
  DEFAULT_SIDEBAR_WIDTH,
  loadSidebarWidth,
  clampSidebarWidth,
  persistSidebarWidth,
  setSidebarWidthStorage,
};
export type { SidebarWidthStorage };
