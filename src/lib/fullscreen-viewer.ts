const FULLSCREEN_VIEWER_STATE_EVENT = 'tau:fullscreen-viewer-state';

type FullscreenViewer = symbol;

const viewers: FullscreenViewer[] = [];

function registerFullscreenViewer(): FullscreenViewer {
  const viewer = Symbol('fullscreen-viewer');
  const wasEmpty = viewers.length === 0;
  viewers.push(viewer);
  if (wasEmpty) dispatchState(true);
  return viewer;
}

function unregisterFullscreenViewer(viewer: FullscreenViewer): void {
  const index = viewers.indexOf(viewer);
  if (index === -1) return;
  viewers.splice(index, 1);
  if (viewers.length === 0) dispatchState(false);
}

function isTopmostFullscreenViewer(viewer: FullscreenViewer): boolean {
  return viewers.at(-1) === viewer;
}

function isUiMenuOpen(): boolean {
  return document.querySelector('.ui-menu') !== null;
}

function dispatchState(open: boolean): void {
  window.dispatchEvent(
    new CustomEvent(FULLSCREEN_VIEWER_STATE_EVENT, { detail: open }),
  );
}

export {
  type FullscreenViewer,
  isTopmostFullscreenViewer,
  isUiMenuOpen,
  registerFullscreenViewer,
  unregisterFullscreenViewer,
};
