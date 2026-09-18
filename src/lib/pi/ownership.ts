import { invokeTraced } from '../telemetry';

type OwnershipFailureKind = 'conflict' | 'retryable';

class OwnershipClaimError extends Error {
  readonly kind: OwnershipFailureKind;

  constructor(kind: OwnershipFailureKind) {
    super(
      kind === 'conflict' ? 'Pi ownership conflict' : 'Pi ownership failed',
    );
    this.name = 'OwnershipClaimError';
    this.kind = kind;
  }
}

function ownershipFailureKind(error: unknown): OwnershipFailureKind {
  if (
    typeof error === 'object' &&
    error !== null &&
    'kind' in error &&
    error.kind === 'conflict'
  ) {
    return 'conflict';
  }
  return 'retryable';
}

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
          expectedRevision = await invokeTraced<number>(
            'read_pi_frontend_revision',
          );
        }
        await invokeTraced('claim_pi_frontend', { ownerId, expectedRevision });
      })().catch((error: unknown): never => {
        claimPromise = undefined;
        throw new OwnershipClaimError(ownershipFailureKind(error));
      });
      return claimPromise;
    },
  };
}

const frontendOwnership = createFrontendOwnership();

function piOwnerArgs(): { ownerId: string } {
  return { ownerId: frontendOwnership.ownerId };
}

export type { OwnershipFailureKind };

export {
  createFrontendOwnership,
  frontendOwnership,
  OwnershipClaimError,
  piOwnerArgs,
};
