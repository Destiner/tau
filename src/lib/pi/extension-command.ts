import type { SessionController } from '../../composables/state';

export default function invokesExtensionCommand(
  controller: SessionController,
  message: string,
): boolean {
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
