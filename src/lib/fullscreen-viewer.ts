const FULLSCREEN_VIEWER_STATE_EVENT = 'tau:fullscreen-viewer-state';

/** An opaque entry in the fullscreen viewer stack. */
type FullscreenViewer = symbol;

const viewers: FullscreenViewer[] = [];

/**
 * Adds a fullscreen viewer above the existing stack and reports the aggregate
 * open state to the app. Call unregisterFullscreenViewer when it unmounts.
 */
function registerFullscreenViewer(): FullscreenViewer {
  const viewer = Symbol('fullscreen-viewer');
  const wasEmpty = viewers.length === 0;
  viewers.push(viewer);
  if (wasEmpty) dispatchState(true);
  return viewer;
}

/** Removes a viewer from the stack. It is safe to call more than once. */
function unregisterFullscreenViewer(viewer: FullscreenViewer): void {
  const index = viewers.indexOf(viewer);
  if (index === -1) return;
  viewers.splice(index, 1);
  if (viewers.length === 0) dispatchState(false);
}

/** Only the newest mounted fullscreen viewer may own Escape. */
function isTopmostFullscreenViewer(viewer: FullscreenViewer): boolean {
  return viewers.at(-1) === viewer;
}

/** Rêka portals open menus to the document body with this shared class. */
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
