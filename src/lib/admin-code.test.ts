import { describe, expect, it } from 'vitest';

import { createAdminCodeMatcher } from './admin-code';

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
  it('unlocks only on the final key, repeatedly and case-insensitively', () => {
    const admin = matcher();

    expect(typeCode(admin, 'iddq')).toBe(false);
    expect(admin.press(keyEvent('d'), 5)).toBe(true);
    expect(typeCode(admin, 'iddqd', {}, 10)).toBe(true);
    expect(typeCode(matcher(), 'IDDQD')).toBe(true);
  });

  it('never unlocks from a text field, however the code is typed', () => {
    const admin = matcher();

    expect(typeCode(admin, 'iddqd', { editable: true })).toBe(false);

    // outside one must not complete the code either.
    expect(typeCode(admin, 'idd', { editable: true }, 10)).toBe(false);
    expect(typeCode(admin, 'qd', {}, 13)).toBe(false);
  });
});
