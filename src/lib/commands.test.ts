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

  it("keeps Pi's command order until a query is entered", () => {
    expect(filterCommands(commands, '')).toEqual(commands);
  });

  it('fuzzy matches command names and ranks tighter matches first', () => {
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

  it('formats a command for immediate invocation', () => {
    expect(commandInvocation(commands[0]!)).toBe('/session-name');
  });

  it('uses Tab to complete any command without submitting', () => {
    expect(commandSelection(commands[0]!, true)).toEqual({
      draft: '/session-name ',
      submit: false,
    });
  });

  it('immediately invokes selected commands, including skills', () => {
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
  // A composer docked at the bottom of a 800px tall window.
  const docked: CommandMenuGeometry = {
    contentHeight: 200,
    composerTop: 660,
    textTop: 664,
    textLineHeight: 20,
    topBoundary: 40,
    bottomBoundary: 800,
  };

  it('opens above the composer when the content fits there', () => {
    expect(commandMenuLayout(docked)).toMatchObject({
      placement: 'above',
      maxHeight: 200,
    });
  });

  it('opens below the draft text when the space above is smaller', () => {
    // An empty session composer filling the pane below the header.
    const layout = commandMenuLayout({
      ...docked,
      composerTop: 40,
      textTop: 56,
    });

    expect(layout.placement).toBe('below');
    expect(layout.maxHeight).toBe(200);
    expect(layout.offset).toBe(41);
  });

  it('caps the height at the space it was given', () => {
    expect(commandMenuLayout({ ...docked, contentHeight: 900 }).maxHeight).toBe(
      300,
    );
    // The same composer in a 320px tall window: still above, but shorter.
    expect(
      commandMenuLayout({
        ...docked,
        contentHeight: 900,
        composerTop: 180,
        textTop: 184,
        bottomBoundary: 320,
      }),
    ).toMatchObject({ placement: 'above', maxHeight: 135 });
  });

  it('keeps a usable height when neither side has room', () => {
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
