import { describe, expect, it } from 'vitest';

import {
  type CommandMenuGeometry,
  type CommandOption,
  commandInvocation,
  commandMenuLayout,
  commandSelection,
  filterCommands,
  slashCommandQuery,
} from './commands';

const commands: CommandOption[] = [
  {
    name: 'session-name',
    description: 'Name the session',
    source: 'extension',
  },
  { name: 'summarize', description: 'Summarize the session', source: 'prompt' },
  { name: 'skill:source-check', description: 'Check a claim', source: 'skill' },
];

describe('slash command completion', () => {
  it('extracts a query only from a leading command token', () => {
    expect(slashCommandQuery('/')).toBe('');
    expect(slashCommandQuery('/ssn')).toBe('ssn');
    expect(slashCommandQuery(' /ssn')).toBeNull();
    expect(slashCommandQuery('/session-name ')).toBeNull();
    expect(slashCommandQuery('/session-name value')).toBeNull();
    expect(slashCommandQuery('/session\n')).toBeNull();
  });

  it('preserves Pi order until fuzzy filtering ranks matching names', () => {
    expect(filterCommands(commands, '')).toEqual(commands);
    expect(filterCommands(commands, 'ssn').map(({ name }) => name)).toEqual([
      'session-name',
    ]);
    expect(filterCommands(commands, 'sum').map(({ name }) => name)).toEqual([
      'summarize',
    ]);
    expect(filterCommands(commands, 'sc').map(({ name }) => name)).toEqual([
      'skill:source-check',
    ]);
  });

  it('formats and selects commands for completion or submission', () => {
    expect(commandInvocation(commands[0]!)).toBe('/session-name');
    expect(commandSelection(commands[0]!, true)).toEqual({
      draft: '/session-name ',
      submit: false,
    });
    expect(commandSelection(commands[0]!)).toEqual({
      draft: '/session-name',
      submit: true,
    });
    expect(commandSelection(commands[2]!)).toEqual({
      draft: '/skill:source-check',
      submit: true,
    });
  });
});

describe('command menu placement', () => {
  const docked: CommandMenuGeometry = {
    contentHeight: 200,
    composerTop: 660,
    textTop: 664,
    textLineHeight: 20,
    topBoundary: 40,
    bottomBoundary: 800,
  };

  it('places the menu above or below based on available space', () => {
    expect(commandMenuLayout(docked)).toMatchObject({
      placement: 'above',
      maxHeight: 200,
    });
    const layout = commandMenuLayout({
      ...docked,
      composerTop: 40,
      textTop: 56,
    });

    expect(layout.placement).toBe('below');
    expect(layout.maxHeight).toBe(200);
    expect(layout.offset).toBe(41);
  });

  it('caps height and retains a usable minimum in constrained space', () => {
    expect(commandMenuLayout({ ...docked, contentHeight: 900 }).maxHeight).toBe(
      300,
    );

    expect(
      commandMenuLayout({
        ...docked,
        contentHeight: 900,
        composerTop: 180,
        textTop: 184,
        bottomBoundary: 320,
      }),
    ).toMatchObject({ placement: 'above', maxHeight: 135 });
    expect(
      commandMenuLayout({
        ...docked,
        composerTop: 60,
        textTop: 64,
        bottomBoundary: 120,
      }).maxHeight,
    ).toBe(96);
  });
});
