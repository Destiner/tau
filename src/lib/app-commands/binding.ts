import type { InjectionKey } from 'vue';

import type { ShortcutInput } from './shortcut';

interface AppCommandTarget {
  projectPath?: string;
  sessionId?: string;
}

type AppCommandDispatch = (id: string, target?: AppCommandTarget) => void;

const appCommandDispatchKey: InjectionKey<AppCommandDispatch> = Symbol(
  'app-command-dispatch',
);

interface AppCommandHint {
  title: string;
  shortcut?: ShortcutInput;
  available: boolean;
}

type AppCommandLookup = (
  id: string,
  target?: AppCommandTarget,
) => AppCommandHint | undefined;
const appCommandLookupKey: InjectionKey<AppCommandLookup> =
  Symbol('app-command-lookup');

export {
  appCommandDispatchKey,
  appCommandLookupKey,
  type AppCommandHint,
  type AppCommandLookup,
  type AppCommandTarget,
  type AppCommandDispatch,
};
