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

    expect(mockInvoke.mock.calls).toEqual([
      ['read_pi_frontend_revision'],
      ['claim_pi_frontend', { ownerId: 'document-a', expectedRevision: 7 }],
    ]);
  });

  it('retries a failed cleanup with the same revision and owner', async () => {
    mockInvoke
      .mockResolvedValueOnce(3)
      .mockRejectedValueOnce(new Error('cleanup failed'))
      .mockResolvedValueOnce(undefined);
    const ownership = createFrontendOwnership('document-a');

    await expect(ownership.claim()).rejects.toThrow('cleanup failed');
    await ownership.claim();

    expect(mockInvoke).toHaveBeenCalledTimes(3);
    expect(mockInvoke.mock.calls.slice(1)).toEqual([
      ['claim_pi_frontend', { ownerId: 'document-a', expectedRevision: 3 }],
      ['claim_pi_frontend', { ownerId: 'document-a', expectedRevision: 3 }],
    ]);
  });

  it('gives independent documents independent owners', () => {
    expect(createFrontendOwnership('document-a').ownerId).not.toBe(
      createFrontendOwnership('document-b').ownerId,
    );
  });
});
