import { describe, expect, it } from 'vitest';

import { createReproLaunch, parseReproArgs, reproUsage } from './repro-command';

const raceScenario = 'saved-session-stale-generation';

describe('reproduction command', () => {
  it('lists the shared scenario catalogue for help and explicit listing', () => {
    const usage = reproUsage();

    expect(parseReproArgs(['--help'])).toEqual({
      kind: 'print',
      text: usage,
    });
    expect(parseReproArgs(['--list'])).toMatchObject({
      kind: 'print',
      text: expect.stringContaining(raceScenario),
    });
    expect(usage).toContain('saved-session-bootstrap');
    expect(usage).toContain('saved-session-conversation');
    expect(usage).toContain(raceScenario);
    expect(usage).not.toContain('saved-session-stream-then-stale-generation');
  });

  it('rejects missing, unknown, and extra names before launch', () => {
    expect(parseReproArgs([])).toMatchObject({
      kind: 'error',
      text: expect.stringMatching(
        /Missing scenario name[\s\S]*Available scenarios/,
      ),
    });
    expect(parseReproArgs(['not-checked-in'])).toMatchObject({
      kind: 'error',
      text: expect.stringMatching(
        /Unknown scenario "not-checked-in"[\s\S]*saved-session-stale-generation/,
      ),
    });
    expect(parseReproArgs([raceScenario, 'extra'])).toMatchObject({
      kind: 'error',
      text: expect.stringMatching(
        /Expected exactly one scenario name[\s\S]*Available scenarios/,
      ),
    });
  });

  it('constructs a Vite launch at the selected real-App scenario', () => {
    expect(parseReproArgs([raceScenario])).toMatchObject({
      kind: 'launch',
      scenario: { name: raceScenario },
    });
    expect(createReproLaunch(raceScenario, '/fixture/bin/bun')).toEqual({
      command: '/fixture/bin/bun',
      args: ['run', 'dev', '--', '--open', `/?test-scenario=${raceScenario}`],
      scenarioUrl: `/?test-scenario=${raceScenario}`,
    });
    expect(() => createReproLaunch('not-checked-in')).toThrowError(
      'Cannot launch unknown scenario "not-checked-in".',
    );
  });
});
