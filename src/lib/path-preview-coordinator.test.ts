import { describe, expect, it, vi } from 'vitest';

import { createPathPreviewCoordinator } from './path-preview-coordinator';

describe('path preview coordinator', () => {
  it('deduplicates equivalent targets across origins', () => {
    const coordinator = createPathPreviewCoordinator();
    const cancel = vi.fn();

    expect(coordinator.start(Symbol(), 'project\0a.txt', 'a', cancel)).toBe(
      'started',
    );
    expect(coordinator.start(Symbol(), 'project\0a.txt', 'b', vi.fn())).toBe(
      'duplicate',
    );
    expect(cancel).not.toHaveBeenCalled();
  });

  it('supersedes a different target and ignores stale completion', () => {
    const coordinator = createPathPreviewCoordinator();
    const firstCancel = vi.fn();
    const secondCancel = vi.fn();
    const secondOrigin = Symbol();

    coordinator.start(Symbol(), 'project\0a.txt', 'a', firstCancel);
    coordinator.start(secondOrigin, 'project\0b.txt', 'b', secondCancel);
    coordinator.finish('a');
    coordinator.cancelOrigin(secondOrigin);

    expect(firstCancel).toHaveBeenCalledOnce();
    expect(secondCancel).toHaveBeenCalledOnce();
  });
});
