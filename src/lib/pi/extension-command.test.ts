import { describe, expect, it } from 'vitest';

import type { CommandOption } from '../commands';

import invokesExtensionCommand, {
  isReloadCommand,
  reloadHelperAvailable,
  TAU_RELOAD_COMMAND,
} from './extension-command';

function controller(
  commands: CommandOption[] = [],
  commandsLoaded = true,
): { commands: CommandOption[]; commandsLoaded: boolean } {
  return { commands, commandsLoaded };
}

describe('reload invocation', () => {
  it.each(['/reload', ' /reload ', '\t/reload\n', '/reload\t'])(
    'recognizes standalone %j before discovery',
    (message) => {
      expect(isReloadCommand(message)).toBe(true);
      expect(invokesExtensionCommand(controller([], false), message)).toBe(
        true,
      );
    },
  );

  it.each([
    '/reload extra',
    '/reload\textra',
    '/reload-now',
    '/reload/other',
    '/Reload',
    'reload',
    '/reload\nextra',
  ])('does not reserve %j', (message) => {
    expect(isReloadCommand(message)).toBe(false);
    expect(invokesExtensionCommand(controller(), message)).toBe(false);
  });

  it('preserves Pi extension parsing for non-reserved invocations', () => {
    const discovered = controller([{ name: 'reload', source: 'extension' }]);
    expect(invokesExtensionCommand(discovered, '/reload extra')).toBe(true);
    expect(invokesExtensionCommand(discovered, '/reload\textra')).toBe(false);
  });

  it('retains extension semantics for the discovered helper', () => {
    expect(
      invokesExtensionCommand(
        controller([{ name: TAU_RELOAD_COMMAND, source: 'extension' }]),
        `/${TAU_RELOAD_COMMAND}`,
      ),
    ).toBe(true);
  });
});

describe('reload helper verification', () => {
  const helper: CommandOption = {
    name: TAU_RELOAD_COMMAND,
    source: 'extension',
  };

  it('requires discovery and the exact extension name', () => {
    expect(reloadHelperAvailable(controller([helper]))).toBe(true);
    expect(reloadHelperAvailable(controller([helper], false))).toBe(false);
    expect(reloadHelperAvailable(controller())).toBe(false);
    expect(
      reloadHelperAvailable(
        controller([{ ...helper, name: `${TAU_RELOAD_COMMAND}:tau` }]),
      ),
    ).toBe(false);
    expect(
      reloadHelperAvailable(controller([{ ...helper, source: 'prompt' }])),
    ).toBe(false);
  });

  it('rejects ambiguous discovery instead of guessing', () => {
    expect(reloadHelperAvailable(controller([helper, helper]))).toBe(false);
    expect(
      reloadHelperAvailable(
        controller([helper, { ...helper, source: 'skill' }]),
      ),
    ).toBe(false);
  });
});
