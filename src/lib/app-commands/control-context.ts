import type { AppCommandTarget } from './binding';

/** Only explicit row-local controls may act while an inline prompt is waiting. */
export default function controlBlocked(
  id: string,
  target: AppCommandTarget | undefined,
  inlinePromptOpen: boolean,
  otherSurfaceOpen: boolean,
): boolean {
  const sessionControl =
    (id === 'session.archive' ||
      id === 'session.markRead' ||
      id === 'session.markUnread') &&
    Boolean(target?.projectPath && target?.sessionId);
  const projectControl =
    id === 'project.openLocal' || id === 'project.openRemote';
  return (
    otherSurfaceOpen || (inlinePromptOpen && !sessionControl && !projectControl)
  );
}
