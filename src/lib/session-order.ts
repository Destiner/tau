/**
 * The session list sorts by activity, so a session working in the background
 * can move a row out from under the pointer between the user aiming and
 * clicking — at worst archiving the wrong session. While the pointer is over
 * the list, the rows visible on entry are held, and the current sort resumes
 * as soon as the pointer leaves.
 */

/** Sessions a project listed when the pointer entered the list. */
function heldSessions<T extends { id: string }>(sessions: readonly T[]): T[] {
  return [...sessions];
}

/**
 * Projects every still-present session from the held snapshot into its entry
 * order. New sessions stay hidden until release: inserting even at the end of
 * an earlier project would move every project below it, defeating a
 * workspace-wide hold. Snapshot objects are retained because a phantom's id
 * changes when it materializes. `undefined` means there is no hold, while an
 * empty array deliberately holds an empty project empty.
 */
function applyHeldOrder<T extends { id: string }>(
  sessions: readonly T[],
  held: readonly { id: string }[] | undefined,
): T[] {
  if (held === undefined) return [...sessions];

  const current = new Map(sessions.map((session) => [session.id, session]));
  return held
    .map((session) => current.get(session.id))
    .filter((session): session is T => session !== undefined);
}

export { applyHeldOrder, heldSessions };
