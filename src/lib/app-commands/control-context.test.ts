import { describe, expect, it } from 'vitest';

import controlBlocked from './control-context';

const target = { projectPath: '/project', sessionId: 'saved' };

describe('injected control command context', () => {
  it.each(['session.archive', 'session.markRead', 'session.markUnread'])(
    'permits explicit %s despite an inline prompt, for lookup and dispatch',
    (id) => {
      expect(controlBlocked(id, target, true, false)).toBe(false);
      expect(controlBlocked(id, target, true, true)).toBe(true);
      expect(controlBlocked(id, undefined, true, false)).toBe(true);
    },
  );

  it('keeps unrelated controls blocked by the prompt', () => {
    expect(controlBlocked('session.new', target, true, false)).toBe(true);
    expect(controlBlocked('session.unarchive', target, true, false)).toBe(true);
    expect(controlBlocked('session.archive', target, false, true)).toBe(true);
  });

  it('preserves the project-opening pointer exception without bypassing other layers', () => {
    expect(controlBlocked('project.openLocal', undefined, true, false)).toBe(
      false,
    );
    expect(controlBlocked('project.openRemote', undefined, true, true)).toBe(
      true,
    );
  });
});
