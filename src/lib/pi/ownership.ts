import { invoke } from '@tauri-apps/api/core';

interface FrontendOwnership {
  readonly ownerId: string;
  claim: () => Promise<void>;
}

function newOwnerId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `tau-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
  );
}

function createFrontendOwnership(ownerId = newOwnerId()): FrontendOwnership {
  let expectedRevision: number | undefined;
  let claimPromise: Promise<void> | undefined;

  return {
    ownerId,
    claim(): Promise<void> {
      if (claimPromise) return claimPromise;
      claimPromise = (async (): Promise<void> => {
        if (expectedRevision === undefined) {
          expectedRevision = await invoke<number>('read_pi_frontend_revision');
        }
        await invoke('claim_pi_frontend', { ownerId, expectedRevision });
      })().catch((error: unknown): never => {
        claimPromise = undefined;
        throw error;
      });
      return claimPromise;
    },
  };
}

const frontendOwnership = createFrontendOwnership();

function piOwnerArgs(): { ownerId: string } {
  return { ownerId: frontendOwnership.ownerId };
}

export { createFrontendOwnership, frontendOwnership, piOwnerArgs };
