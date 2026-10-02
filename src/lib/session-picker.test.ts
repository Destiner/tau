import { describe, expect, it } from 'vitest';

import type { ProjectSummary, SessionSummary } from '../composables/state';

import {
  preferredSessionPickerId,
  resolveSessionPickerTarget,
  sessionPickerEntries,
  sessionPickerId,
} from './session-picker';

function session(id: string, archived = false): SessionSummary {
  return {
    id,
    path: id,
    title: id,
    lastActive: 'now',
    lastUserMessageAt: 0,
    sortAt: 0,
    archived,
    selected: false,
  };
}
function project(
  path: string,
  sessions: SessionSummary[],
  collapsed = false,
): ProjectSummary {
  return {
    path,
    name: 'Same name',
    workingDirectory: path,
    collapsed,
    selected: false,
    sessions,
  };
}
const sessions = (item: ProjectSummary): SessionSummary[] => item.sessions;

describe('session picker', () => {
  it('keeps order and excludes archived and collapsed entries without merging duplicate names', () => {
    const projects = [
      project('/a', [session('shared'), session('archived', true)]),
      project('/hidden', [session('hidden')], true),
      project('/empty', []),
      project('/b', [session('shared')]),
    ];
    const entries = sessionPickerEntries(projects, sessions);
    expect(
      entries.map((entry) => [entry.projectPath, entry.sessionId]),
    ).toEqual([
      ['/a', 'shared'],
      ['/b', 'shared'],
    ]);
    expect(entries[0]?.id).not.toBe(entries[1]?.id);
    expect(preferredSessionPickerId(entries, '/b', 'shared')).toBe(
      entries[1]?.id,
    );
    expect(preferredSessionPickerId(entries, '/hidden', 'hidden')).toBe(
      entries[0]?.id,
    );
    expect(preferredSessionPickerId([], '/a', 'shared')).toBeNull();
  });

  it('encodes tuple identities without delimiter collisions', () => {
    expect(sessionPickerId('/a:b', 'c')).not.toBe(sessionPickerId('/a', 'b:c'));
    expect(sessionPickerId('/a", "b', 'c')).not.toBe(
      sessionPickerId('/a', 'b", "c'),
    );
  });

  it('resolves only currently eligible exact targets, never a same-ID fallback', () => {
    const first = project('/a', [session('shared')]);
    const second = project('/b', [session('shared')]);
    const projects = [first, second];
    const id = sessionPickerId('/a', 'shared');
    expect(resolveSessionPickerTarget(id, projects, sessions)?.project).toBe(
      first,
    );
    first.sessions.splice(0);
    expect(resolveSessionPickerTarget(id, projects, sessions)).toBeUndefined();
    first.sessions.push(session('shared', true));
    expect(resolveSessionPickerTarget(id, projects, sessions)).toBeUndefined();
    first.sessions[0]!.archived = false;
    first.collapsed = true;
    expect(resolveSessionPickerTarget(id, projects, sessions)).toBeUndefined();
    expect(
      resolveSessionPickerTarget(
        sessionPickerId('/b', 'shared'),
        projects,
        sessions,
      )?.project,
    ).toBe(second);
  });
});
