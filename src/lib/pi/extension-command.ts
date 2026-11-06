import type { SessionController } from '../../composables/state';

const TAU_RELOAD_COMMAND = 'tau-internal-reload-v1';

function isReloadCommand(message: string): boolean {
  return message.trim() === '/reload';
}

// Discovery stays raw: an alias must never dispatch an unverified helper or
// guess Pi's collision suffixes.
function reloadHelperAvailable(
  controller: Pick<SessionController, 'commands' | 'commandsLoaded'>,
): boolean {
  return (
    controller.commandsLoaded &&
    controller.commands.filter((command) => command.name === TAU_RELOAD_COMMAND)
      .length === 1 &&
    controller.commands.some(
      (command) =>
        command.name === TAU_RELOAD_COMMAND && command.source === 'extension',
    )
  );
}

// Existing consumers use this classifier to bypass model setup and avoid
// optimistic transcript messages, including before cold-session discovery.
export default function invokesExtensionCommand(
  controller: Pick<SessionController, 'commands'>,
  message: string,
): boolean {
  if (isReloadCommand(message)) return true;
  if (!message.startsWith('/')) return false;
  // Pi splits command names at the first space, not arbitrary whitespace.
  const space = message.indexOf(' ');
  const name = space < 0 ? message.slice(1) : message.slice(1, space);
  return (
    Boolean(name) &&
    controller.commands.some(
      (command) => command.name === name && command.source === 'extension',
    )
  );
}

export { TAU_RELOAD_COMMAND, isReloadCommand, reloadHelperAvailable };
