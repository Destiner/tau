import {
  type KeyboardShortcutEvent,
  type NormalizedShortcut,
  type ShortcutInput,
  type ShortcutPlatform,
  normalizeShortcut,
  shortcutMatches,
} from './shortcut';

type CommandAvailability =
  { available: true } | { available: false; reason: string };

type CommandExposure<Context> = boolean | ((context: Context) => boolean);

interface CommandShortcut {
  shortcut: ShortcutInput;
  /** Scopes are ordered by the UI's active keyboard layer. */
  scope: string;
  /** Mutually exclusive commands may share a chord within a scope. */
  variantGroup?: string;
}

interface CommandDefinition<
  Context,
  Target = undefined,
  Id extends string = string,
> {
  id: Id;
  title: string | ((context: Context) => string);
  keywords?: readonly string[];
  group?: string;
  /** Palette and shortcut exposure are deliberately independent. */
  palette?: CommandExposure<Context>;
  shortcuts?: readonly CommandShortcut[];
  resolveTarget?: (context: Context) => Target | undefined;
  availability?: (
    context: Context,
    target: Target | undefined,
  ) => CommandAvailability;
  execute: (
    context: Context,
    target: Target | undefined,
  ) => void | Promise<void>;
}

interface PaletteCommand<Context, Id extends string = string> {
  definition: CommandDefinition<Context, unknown, Id>;
  title: string;
  keywords?: readonly string[];
  group?: string;
}

type DispatchResult =
  | { status: 'executed' }
  | { status: 'unavailable'; reason: string }
  | { status: 'pending' }
  | { status: 'missing' };

interface CommandRegistryOptions {
  /** Return true only when two scopes can receive the same key event. */
  scopesOverlap?: (left: string, right: string) => boolean;
}

/**
 * A UI-neutral command registry. Dispatch always resolves target and
 * availability immediately before execution, so every entry point gets the
 * same guard.
 */
class CommandRegistry<Context, Id extends string = string> {
  private readonly definitions = new Map<
    Id,
    CommandDefinition<Context, unknown, Id>
  >();
  private readonly pending = new Set<string>();
  private readonly scopesOverlap: (left: string, right: string) => boolean;

  constructor(options: CommandRegistryOptions = {}) {
    this.scopesOverlap =
      options.scopesOverlap ??
      ((left: string, right: string): boolean => left === right);
  }

  register<Target>(
    definition: CommandDefinition<Context, Target, Id>,
  ): () => void {
    if (this.definitions.has(definition.id)) {
      throw new Error(`Duplicate app command ID: ${definition.id}`);
    }
    const normalized = definition.shortcuts?.map((binding) => ({
      ...binding,
      shortcut: normalizeShortcut(binding.shortcut),
    }));
    for (const existing of this.definitions.values()) {
      for (const next of normalized ?? []) {
        for (const binding of existing.shortcuts ?? []) {
          if (
            sameShortcut(next.shortcut, binding.shortcut) &&
            this.scopesOverlap(next.scope, binding.scope) &&
            !(next.variantGroup && next.variantGroup === binding.variantGroup)
          ) {
            throw new Error(
              `Shortcut collision: ${definition.id} and ${existing.id} share ${next.scope}`,
            );
          }
        }
      }
    }
    const stored: CommandDefinition<Context, unknown, Id> = {
      id: definition.id,
      title: definition.title,
      ...(definition.keywords ? { keywords: definition.keywords } : {}),
      ...(definition.group ? { group: definition.group } : {}),
      ...(definition.palette === undefined
        ? {}
        : { palette: definition.palette }),
      ...(normalized ? { shortcuts: normalized } : {}),
      ...(definition.resolveTarget
        ? { resolveTarget: (context) => definition.resolveTarget?.(context) }
        : {}),
      ...(definition.availability
        ? {
            availability: (context, target) =>
              definition.availability?.(
                context,
                target as Target | undefined,
              ) ?? {
                available: true,
              },
          }
        : {}),
      execute: (context, target) =>
        definition.execute(context, target as Target | undefined),
    };
    this.definitions.set(definition.id, stored);
    return () => this.definitions.delete(definition.id);
  }

  get(id: Id): CommandDefinition<Context, unknown, Id> | undefined {
    return this.definitions.get(id);
  }

  discover(
    id: Id,
    context: Context,
  ): (PaletteCommand<Context, Id> & { available: boolean }) | undefined {
    const definition = this.definitions.get(id);
    if (!definition) return;
    return {
      definition,
      title: resolveTitle(definition, context),
      available: this.check(definition, context).available,
    };
  }

  palette(context: Context): PaletteCommand<Context, Id>[] {
    return [...this.definitions.values()]
      .filter((definition) => exposed(definition.palette, context))
      .filter((definition) => this.check(definition, context).available)
      .map((definition) => ({
        definition,
        title: resolveTitle(definition, context),
        ...(definition.keywords ? { keywords: definition.keywords } : {}),
        ...(definition.group ? { group: definition.group } : {}),
      }));
  }

  async dispatch(id: Id, context: Context): Promise<DispatchResult> {
    const definition = this.definitions.get(id);
    if (!definition) return { status: 'missing' };
    const target = definition.resolveTarget?.(context);
    const availability = definition.availability?.(context, target) ?? {
      available: true,
    };
    if (!availability.available) {
      return { status: 'unavailable', reason: availability.reason };
    }
    if (this.pending.has(id)) return { status: 'pending' };

    this.pending.add(id);
    try {
      await definition.execute(context, target);
      return { status: 'executed' };
    } finally {
      this.pending.delete(id);
    }
  }

  /** Finds only the highest-priority active scope. A disabled match consumes it. */
  async dispatchShortcut(
    event: KeyboardShortcutEvent,
    context: Context,
    activeScopes: readonly string[],
    platform: ShortcutPlatform,
  ): Promise<DispatchResult> {
    for (const scope of activeScopes) {
      const matches = [...this.definitions.values()].filter((definition) =>
        definition.shortcuts?.some(
          (binding) =>
            binding.scope === scope &&
            shortcutMatches(event, binding.shortcut, platform),
        ),
      );
      const match =
        matches.find(
          (definition) => this.check(definition, context).available,
        ) ?? matches[0];
      if (match) return this.dispatch(match.id, context);
    }
    return { status: 'missing' };
  }

  private check(
    definition: CommandDefinition<Context, unknown>,
    context: Context,
  ): CommandAvailability {
    const target = definition.resolveTarget?.(context);
    return definition.availability?.(context, target) ?? { available: true };
  }
}

function exposed<Context>(
  exposure: CommandExposure<Context> | undefined,
  context: Context,
): boolean {
  return typeof exposure === 'function' ? exposure(context) : exposure === true;
}

function resolveTitle<Context>(
  definition: CommandDefinition<Context, unknown>,
  context: Context,
): string {
  return typeof definition.title === 'function'
    ? definition.title(context)
    : definition.title;
}

function sameShortcut(
  left: ShortcutInput | NormalizedShortcut,
  right: ShortcutInput | NormalizedShortcut,
): boolean {
  const normalizedLeft = normalizeShortcut(left);
  const normalizedRight = normalizeShortcut(right);
  return (
    normalizedLeft.key === normalizedRight.key &&
    normalizedLeft.allowRepeat === normalizedRight.allowRepeat &&
    normalizedLeft.modifiers.length === normalizedRight.modifiers.length &&
    normalizedLeft.modifiers.every(
      (modifier, index) => modifier === normalizedRight.modifiers[index],
    )
  );
}

export {
  CommandRegistry,
  type CommandAvailability,
  type CommandDefinition,
  type CommandExposure,
  type CommandRegistryOptions,
  type CommandShortcut,
  type DispatchResult,
  type PaletteCommand,
};
