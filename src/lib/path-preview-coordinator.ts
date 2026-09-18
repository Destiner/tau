import type { InjectionKey } from 'vue';

interface PreviewIntent {
  origin: symbol;
  key: string;
  requestId: string;
  cancel: () => void;
}

interface PathPreviewCoordinator {
  start(
    origin: symbol,
    key: string,
    requestId: string,
    cancel: () => void,
  ): 'started' | 'duplicate';
  finish(requestId: string): void;
  cancelOrigin(origin: symbol): void;
}

function createPathPreviewCoordinator(): PathPreviewCoordinator {
  let active: PreviewIntent | undefined;
  return {
    start(origin, key, requestId, cancel): 'started' | 'duplicate' {
      if (active?.key === key) return 'duplicate';
      active?.cancel();
      active = { origin, key, requestId, cancel };
      return 'started';
    },
    finish(requestId): void {
      if (active?.requestId === requestId) active = undefined;
    },
    cancelOrigin(origin): void {
      if (active?.origin !== origin) return;
      const pending = active;
      active = undefined;
      pending.cancel();
    },
  };
}

const pathPreviewCoordinatorKey: InjectionKey<PathPreviewCoordinator> = Symbol(
  'path-preview-coordinator',
);

export type { PathPreviewCoordinator };
export { createPathPreviewCoordinator, pathPreviewCoordinatorKey };
