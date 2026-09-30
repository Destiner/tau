import type { InjectionKey } from 'vue';

interface AppCommandTarget {
  projectPath?: string;
  sessionId?: string;
}

type AppCommandDispatch = (id: string, target?: AppCommandTarget) => void;

const appCommandDispatchKey: InjectionKey<AppCommandDispatch> = Symbol(
  'app-command-dispatch',
);

export {
  appCommandDispatchKey,
  type AppCommandTarget,
  type AppCommandDispatch,
};
