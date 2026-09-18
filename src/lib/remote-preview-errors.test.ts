import { describe, expect, it } from 'vitest';

import {
  remotePreviewErrorCopy,
  remotePreviewErrorKind,
} from './remote-preview-errors';

describe('remote preview failure copy', () => {
  it.each([
    ['too_large', 'File is too large to preview (64 MiB maximum).'],
    [
      'not_found_or_unreadable',
      'File could not be read. Check that it exists and you have access.',
    ],
    ['timed_out', 'Preview timed out. Try again.'],
    ['presentation_failed', 'Quick Look could not open the file. Try again.'],
    [
      'unavailable',
      'Could not prepare preview. Check the connection and try again.',
    ],
  ] as const)('maps %s to reviewed copy', (kind, copy) => {
    expect(remotePreviewErrorCopy(kind)).toBe(copy);
  });

  it('accepts structured command errors and classifies bounded legacy strings', () => {
    expect(
      remotePreviewErrorKind({ kind: 'too_large', message: 'private' }),
    ).toBe('too_large');
    expect(remotePreviewErrorKind('The remote file preview timed out.')).toBe(
      'timed_out',
    );
    expect(remotePreviewErrorKind('unexpected private failure')).toBe(
      'unavailable',
    );
  });
});
