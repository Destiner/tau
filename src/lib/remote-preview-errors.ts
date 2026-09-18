type RemotePreviewErrorKind =
  | 'too_large'
  | 'not_found_or_unreadable'
  | 'timed_out'
  | 'presentation_failed'
  | 'superseded'
  | 'unavailable';

function remotePreviewErrorKind(error: unknown): RemotePreviewErrorKind {
  if (typeof error === 'object' && error && 'kind' in error) {
    const kind = String(error.kind);
    if (
      kind === 'too_large' ||
      kind === 'not_found_or_unreadable' ||
      kind === 'timed_out' ||
      kind === 'presentation_failed' ||
      kind === 'superseded'
    )
      return kind;
  }
  if (typeof error === 'string') {
    if (error.includes('64 MiB')) return 'too_large';
    if (error.includes('timed out')) return 'timed_out';
    if (error.includes('superseded') || error.includes('no longer available'))
      return 'superseded';
    if (error.includes('could not be read') || error.includes('inspected'))
      return 'not_found_or_unreadable';
    if (error.includes('Quick Look')) return 'presentation_failed';
  }
  return 'unavailable';
}

function remotePreviewErrorCopy(kind: RemotePreviewErrorKind): string {
  switch (kind) {
    case 'too_large':
      return 'File is too large to preview (64 MiB maximum).';
    case 'not_found_or_unreadable':
      return 'File could not be read. Check that it exists and you have access.';
    case 'timed_out':
      return 'Preview timed out. Try again.';
    case 'presentation_failed':
      return 'Quick Look could not open the file. Try again.';
    case 'superseded':
      return '';
    default:
      return 'Could not prepare preview. Check the connection and try again.';
  }
}

export type { RemotePreviewErrorKind };
export { remotePreviewErrorCopy, remotePreviewErrorKind };
