/**
 * The session list sorts by activity, so a session working in the background
 * can move a row out from under the pointer between the user aiming and
 * clicking — at worst archiving the wrong session. While the pointer is over
 * the list, the rows visible on entry are held, and the current sort resumes
 * as soon as the pointer leaves.
 */

interface HeldSession<T> {
  /** Identity shown when the pointer entered the list. */
  id: string;
  /** Follows an ephemeral row through its first Pi materialization. */
  source: T;
}

/** Sessions a project listed when the pointer entered the list. */
function heldSessions<T extends { id: string }>(
  sessions: readonly T[],
): HeldSession<T>[] {
  return sessions.map((session) => ({ id: session.id, source: session }));
}

/**
 * Projects every still-present session from the held snapshot into its entry
 * order. New sessions stay hidden until release: inserting even at the end of
 * an earlier project would move every project below it, defeating a
 * workspace-wide hold. The captured id wins when a replacement registers the
 * outgoing phase before rebinding its ephemeral object to the next phase. If
 * no row has that id, the ephemeral object's current id follows an ordinary
 * phantom-to-saved materialization even after that object leaves the list.
 * `undefined` means there is no hold, while an empty
 * array deliberately holds an empty project empty.
 */
function applyHeldOrder<T extends { id: string }>(
  sessions: readonly T[],
  held: readonly HeldSession<T>[] | undefined,
): T[] {
  if (held === undefined) return [...sessions];

  const current = new Map(sessions.map((session) => [session.id, session]));
  const currentObjects = new Set(sessions);
  return held
    .map(
      (session) =>
        current.get(session.id) ??
        current.get(session.source.id) ??
        (currentObjects.has(session.source) ? session.source : undefined),
    )
    .filter((session): session is T => session !== undefined);
}

export { applyHeldOrder, heldSessions };
export type { HeldSession };
