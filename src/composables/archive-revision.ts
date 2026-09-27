const revisions = new Map<string, number>();

function archiveRevision(projectPath: string, sessionId: string): number {
  return revisions.get(JSON.stringify([projectPath, sessionId])) ?? 0;
}

function advanceArchiveRevision(
  projectPath: string,
  sessionId: string,
): number {
  const key = JSON.stringify([projectPath, sessionId]);
  const next = (revisions.get(key) ?? 0) + 1;
  revisions.set(key, next);
  return next;
}

export { archiveRevision, advanceArchiveRevision };
