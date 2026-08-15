const SIDEBAR_WIDTH_STORAGE_KEY = 'tau.sidebar-width';
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 480;
const DEFAULT_SIDEBAR_WIDTH = 260;

function loadSidebarWidth(): number {
  try {
    const storedWidth = Number.parseFloat(
      localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY) ?? '',
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
    localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(width));
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
};
