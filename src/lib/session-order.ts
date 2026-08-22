/**
 * The session list sorts by activity, so a session working in the background
 * can move a row out from under the pointer between the user aiming and
 * clicking — at worst archiving the wrong session. While the pointer is over
 * the list, the order it had on entry is held, and the sort resumes as soon as
 * the pointer leaves.
 */

/** Ids of the sessions a project listed when the pointer entered the list. */
function heldSessionIds(sessions: readonly { id: string }[]): string[] {
  return sessions.map((session) => session.id);
}

/**
 * Re-sorts a freshly sorted list back into the held order.
 *
 * Only the sessions that were there on entry are held: a session that appears
 * while the pointer is over the list keeps the position the sort gives it,
 * because it has no place the user could have been aiming at. The held ones
 * fill the remaining slots in the order they were entered with, so no row the
 * user can already see moves.
 */
function applyHeldOrder<T extends { id: string }>(
  sessions: readonly T[],
  heldIds: readonly string[],
): T[] {
  const held = new Map(
    sessions
      .filter((session) => heldIds.includes(session.id))
      .map((session) => [session.id, session]),
  );
  if (held.size < 2) return [...sessions];

  const queue = heldIds
    .map((id) => held.get(id))
    .filter((session): session is T => session !== undefined);
  // One queued session per slot, so the fallback is never reached.
  return sessions.map((session) =>
    held.has(session.id) ? (queue.shift() ?? session) : session,
  );
}

export { applyHeldOrder, heldSessionIds };
