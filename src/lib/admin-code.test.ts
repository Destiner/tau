import { describe, expect, it } from 'vitest';

import { KEY_INTERVAL_MS, createAdminCodeMatcher } from './admin-code';

/** Stand-ins for the real DOM nodes: the matcher only ever hands its
 * target to the editable-target predicate. */
const EDITABLE = {} as EventTarget;
const NOT_EDITABLE = {} as EventTarget;

interface PressOptions {
  editable?: boolean;
  metaKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
}

function keyEvent(key: string, options: PressOptions = {}): KeyboardEvent {
  return {
    key,
    metaKey: options.metaKey ?? false,
    ctrlKey: options.ctrlKey ?? false,
    altKey: options.altKey ?? false,
    target: options.editable === true ? EDITABLE : NOT_EDITABLE,
  } as unknown as KeyboardEvent;
}

function typeCode(
  matcher: ReturnType<typeof createAdminCodeMatcher>,
  keys: string,
  options: PressOptions = {},
  startAt = 0,
): boolean {
  return [...keys].reduce(
    (unlocked, key, index) =>
      matcher.press(keyEvent(key, options), startAt + index) || unlocked,
    false,
  );
}

function matcher(): ReturnType<typeof createAdminCodeMatcher> {
  return createAdminCodeMatcher((target) => target === EDITABLE);
}

describe('createAdminCodeMatcher', () => {
  it('unlocks on the code and only on its last keystroke', () => {
    const admin = matcher();

    expect(typeCode(admin, 'iddq')).toBe(false);
    expect(admin.press(keyEvent('d'), 5)).toBe(true);
  });

  it('unlocks again so the same code turns admin mode back off', () => {
    const admin = matcher();

    expect(typeCode(admin, 'iddqd')).toBe(true);
    expect(typeCode(admin, 'iddqd', {}, 10)).toBe(true);
  });

  it('ignores case', () => {
    expect(typeCode(matcher(), 'IDDQD')).toBe(true);
  });

  it('never unlocks from a text field, however the code is typed', () => {
    const admin = matcher();

    expect(typeCode(admin, 'iddqd', { editable: true })).toBe(false);
    // The half typed in the composer counts for nothing: finishing it
    // outside one must not complete the code either.
    expect(typeCode(admin, 'idd', { editable: true }, 10)).toBe(false);
    expect(typeCode(admin, 'qd', {}, 13)).toBe(false);
  });

  it('restarts the sequence on a wrong key, a modifier, or a control key', () => {
    const admin = matcher();
    expect(typeCode(admin, 'idxdqd')).toBe(false);

    expect(typeCode(admin, 'id', {}, 10)).toBe(false);
    expect(admin.press(keyEvent('d', { metaKey: true }), 12)).toBe(false);
    expect(typeCode(admin, 'dqd', {}, 13)).toBe(false);

    expect(typeCode(admin, 'id', {}, 20)).toBe(false);
    expect(admin.press(keyEvent('Shift'), 22)).toBe(false);
    expect(typeCode(admin, 'dqd', {}, 23)).toBe(false);
  });

  it('starts a fresh sequence from a wrong key that is the code’s own first letter', () => {
    const admin = matcher();

    expect(typeCode(admin, 'idi')).toBe(false);
    expect(typeCode(admin, 'ddqd', {}, 10)).toBe(true);
  });

  it('expires a partial sequence rather than adding up keystrokes typed minutes apart', () => {
    const admin = matcher();

    expect(typeCode(admin, 'idd')).toBe(false);
    expect(admin.press(keyEvent('q'), KEY_INTERVAL_MS + 100)).toBe(false);
    expect(admin.press(keyEvent('d'), KEY_INTERVAL_MS + 101)).toBe(false);
  });
});
