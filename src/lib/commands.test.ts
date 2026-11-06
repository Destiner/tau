import { describe, expect, it } from 'vitest';

import {
  type CommandOption,
  commandSelection,
  filterCommands,
  projectCommands,
} from './commands';
import { isReloadCommand, TAU_RELOAD_COMMAND } from './pi/extension-command';

const reload: CommandOption = {
  name: 'reload',
  description: 'Reload Pi extensions, skills, prompts, and context files.',
  source: 'builtin',
};

describe('visible command projection', () => {
  it('exposes an honest builtin without discovered commands', () => {
    expect(projectCommands([])).toEqual([reload]);
  });

  it('hides only the exact helper and deduplicates the public alias', () => {
    const commands: CommandOption[] = [
      { name: 'implement', source: 'extension' },
      { name: TAU_RELOAD_COMMAND, source: 'extension' },
      { name: 'reload', source: 'extension' },
      { name: 'reload', source: 'prompt' },
      { name: `${TAU_RELOAD_COMMAND}:other`, source: 'extension' },
      { name: 'skill:review', source: 'skill' },
    ];
    const original = structuredClone(commands);
    expect(projectCommands(commands)).toEqual([
      reload,
      commands[0],
      commands[4],
      commands[5],
    ]);
    expect(commands).toEqual(original);
    expect(projectCommands(projectCommands(commands))).toEqual(
      projectCommands(commands),
    );
  });

  it('keeps the builtin after catalogue refreshes', () => {
    const first = projectCommands([{ name: 'before', source: 'extension' }]);
    const refreshed = projectCommands([{ name: 'after', source: 'prompt' }]);
    expect(first.map(({ name }) => name)).toEqual(['reload', 'before']);
    expect(refreshed.map(({ name }) => name)).toEqual(['reload', 'after']);
  });

  it('selects reload through existing filtering and completion semantics', () => {
    const [selected] = filterCommands(projectCommands([]), 'rld');
    expect(selected).toEqual(reload);
    expect(commandSelection(selected!)).toEqual({
      draft: '/reload',
      submit: true,
    });
    const completion = commandSelection(selected!, true);
    expect(completion).toEqual({ draft: '/reload ', submit: false });
    expect(isReloadCommand(completion.draft)).toBe(true);
  });
});
