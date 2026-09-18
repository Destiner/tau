import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createFrontendOwnership } from './ownership';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe('Pi frontend ownership', () => {
  it('reads once, claims once, and keeps one owner for the document', async () => {
    mockInvoke.mockResolvedValueOnce(7).mockResolvedValueOnce(undefined);
    const ownership = createFrontendOwnership('document-a');

    await Promise.all([ownership.claim(), ownership.claim()]);
    await ownership.claim();

    expect(mockInvoke).toHaveBeenNthCalledWith(
      1,
      'read_pi_frontend_revision',
      expect.objectContaining({ telemetryContext: expect.any(Object) }),
    );
    expect(mockInvoke).toHaveBeenNthCalledWith(
      2,
      'claim_pi_frontend',
      expect.objectContaining({ ownerId: 'document-a', expectedRevision: 7 }),
    );
  });

  it('retries a failed cleanup with the same revision and owner', async () => {
    mockInvoke
      .mockResolvedValueOnce(3)
      .mockRejectedValueOnce(new Error('cleanup failed'))
      .mockResolvedValueOnce(undefined);
    const ownership = createFrontendOwnership('document-a');

    await expect(ownership.claim()).rejects.toMatchObject({
      kind: 'retryable',
    });
    await ownership.claim();

    expect(mockInvoke).toHaveBeenCalledTimes(3);
    for (const [, args] of mockInvoke.mock.calls.slice(1)) {
      expect(args).toEqual(
        expect.objectContaining({ ownerId: 'document-a', expectedRevision: 3 }),
      );
    }
  });

  it('classifies a stale revision without rereading it', async () => {
    mockInvoke
      .mockResolvedValueOnce(4)
      .mockRejectedValue({ kind: 'conflict', message: 'raw native message' })
      .mockRejectedValue({ kind: 'conflict', message: 'raw native message' });
    const ownership = createFrontendOwnership('document-a');

    await expect(ownership.claim()).rejects.toMatchObject({ kind: 'conflict' });
    await expect(ownership.claim()).rejects.toMatchObject({ kind: 'conflict' });

    expect(
      mockInvoke.mock.calls.filter(
        ([command]) => command === 'read_pi_frontend_revision',
      ),
    ).toHaveLength(1);
  });

  it('gives independent documents independent owners', () => {
    expect(createFrontendOwnership('document-a').ownerId).not.toBe(
      createFrontendOwnership('document-b').ownerId,
    );
  });
});
