/*
 * The cheat code that unlocks admin mode. Kept as a plain state machine so
 * the matching rules — which keystrokes count, which reset the sequence,
 * and how long a partial one survives — are testable without a keyboard.
 */

/** Doom's, because a hidden switch may as well be the familiar one. */
const ADMIN_CODE = 'iddqd';

/** A partial sequence expires between keystrokes, so characters typed
 * minutes apart never add up to the code by accident. */
const KEY_INTERVAL_MS = 2_000;

interface AdminCodeMatcher {
  press(event: KeyboardEvent, now: number): boolean;
}

function createAdminCodeMatcher(
  isEditableTarget: (target: EventTarget | null) => boolean,
): AdminCodeMatcher {
  let matched = 0;
  let lastPressAt = 0;

  function press(event: KeyboardEvent, now: number): boolean {
    if (
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      event.key.length !== 1 ||
      isEditableTarget(event.target)
    ) {
      matched = 0;
      return false;
    }
    if (matched > 0 && now - lastPressAt > KEY_INTERVAL_MS) matched = 0;
    lastPressAt = now;

    const key = event.key.toLowerCase();

    matched =
      key === ADMIN_CODE[matched] ? matched + 1 : Number(key === ADMIN_CODE[0]);
    if (matched < ADMIN_CODE.length) return false;
    matched = 0;
    return true;
  }

  return { press };
}

export type { AdminCodeMatcher };

export { ADMIN_CODE, KEY_INTERVAL_MS, createAdminCodeMatcher };
