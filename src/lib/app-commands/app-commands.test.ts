import { describe, expect, it, vi } from 'vitest';

import {
  CommandRegistry,
  type KeyboardShortcutEvent,
  filterFuzzy,
  formatShortcut,
  normalizeShortcut,
  paletteBackspaceReturnsRoot,
  shortcutMatches,
} from './index';

const event = (
  overrides: Partial<KeyboardShortcutEvent> = {},
): KeyboardShortcutEvent => ({
  key: 'k',
  ctrlKey: false,
  metaKey: true,
  altKey: false,
  shiftKey: false,
  repeat: false,
  ...overrides,
});

describe('shortcuts', () => {
  it('normalizes modifier order and formats both platforms', () => {
    expect(normalizeShortcut('Shift+Mod+P')).toEqual({
      key: 'p',
      modifiers: ['Mod', 'Shift'],
      allowRepeat: false,
    });
    expect(formatShortcut('Mod+Shift+P', 'mac')).toBe('⌘⇧P');
    expect(formatShortcut('Mod+Shift+P', 'non-mac')).toBe('Ctrl+Shift+P');
    expect(formatShortcut('Ctrl+Tab', 'mac')).toBe('⌃Tab');
    expect(formatShortcut('Mod+Alt+ArrowUp', 'non-mac')).toBe('Ctrl+Alt+Up');
  });

  it('requires exact modifiers and handles shifted punctuation', () => {
    expect(shortcutMatches(event(), 'Mod+K', 'mac')).toBe(true);
    expect(shortcutMatches(event({ ctrlKey: true }), 'Mod+K', 'mac')).toBe(
      false,
    );
    expect(
      shortcutMatches(
        event({ key: '?', metaKey: false, shiftKey: true }),
        'Shift+/',
        'non-mac',
      ),
    ).toBe(true);
  });

  it('matches Option-modified Mac letters by physical key code', () => {
    // Native macOS WebKit emits the produced Option character, not `o`.
    expect(
      shortcutMatches(
        event({ key: 'ø', code: 'KeyO', metaKey: false, altKey: true }),
        'Alt+O',
        'mac',
      ),
    ).toBe(true);
    expect(
      shortcutMatches(
        event({ key: 'ø', code: 'KeyO', metaKey: false, altKey: true }),
        'Alt+O',
        'non-mac',
      ),
    ).toBe(false);
  });

  it('rejects composition, AltGraph, and repeats unless explicitly allowed', () => {
    expect(shortcutMatches(event({ isComposing: true }), 'Mod+K', 'mac')).toBe(
      false,
    );
    expect(
      shortcutMatches(
        event({ getModifierState: (name) => name === 'AltGraph' }),
        'Mod+K',
        'mac',
      ),
    ).toBe(false);
    expect(
      shortcutMatches(
        event({
          key: 'ø',
          code: 'KeyO',
          metaKey: false,
          altKey: true,
          getModifierState: (name) => name === 'AltGraph',
        }),
        'Alt+O',
        'mac',
      ),
    ).toBe(false);
    expect(shortcutMatches(event({ repeat: true }), 'Mod+K', 'mac')).toBe(
      false,
    );
    expect(
      shortcutMatches(
        event({ repeat: true }),
        { key: 'k', modifiers: ['Mod'], allowRepeat: true },
        'mac',
      ),
    ).toBe(true);
  });
});

describe('command registry', () => {
  it('keeps palette and shortcut exposure independent', async () => {
    const registry = new CommandRegistry<{ enabled: boolean }>();
    const paletteOnly = vi.fn();
    const shortcutOnly = vi.fn();
    registry.register({
      id: 'palette',
      title: 'Palette',
      palette: true,
      execute: paletteOnly,
    });
    registry.register({
      id: 'shortcut',
      title: 'Shortcut',
      shortcuts: [{ scope: 'app', shortcut: 'Mod+S' }],
      execute: shortcutOnly,
    });
    registry.register({
      id: 'hidden',
      title: 'Hidden',
      palette: true,
      availability: (context) =>
        context.enabled
          ? { available: true }
          : { available: false, reason: 'Unavailable' },
      execute: vi.fn(),
    });

    expect(
      registry.palette({ enabled: false }).map(({ title }) => title),
    ).toEqual(['Palette']);
    await registry.dispatchShortcut(
      event({ key: 's' }),
      { enabled: false },
      ['app'],
      'mac',
    );
    expect(shortcutOnly).toHaveBeenCalledOnce();
    expect(paletteOnly).not.toHaveBeenCalled();
  });

  it('rejects duplicate IDs and collisions in overlapping scopes', () => {
    const registry = new CommandRegistry<object>({
      scopesOverlap: (left, right): boolean =>
        left === 'global' || right === 'global' || left === right,
    });
    registry.register({
      id: 'one',
      title: 'One',
      shortcuts: [{ scope: 'global', shortcut: 'Mod+K' }],
      execute: vi.fn(),
    });
    expect(() =>
      registry.register({ id: 'one', title: 'Again', execute: vi.fn() }),
    ).toThrow('Duplicate');
    expect(() =>
      registry.register({
        id: 'two',
        title: 'Two',
        shortcuts: [{ scope: 'dialog', shortcut: 'Mod+K' }],
        execute: vi.fn(),
      }),
    ).toThrow('collision');

    const disjoint = new CommandRegistry<object>();
    disjoint.register({
      id: 'local-a',
      title: 'A',
      shortcuts: [{ scope: 'rename', shortcut: 'Enter' }],
      execute: vi.fn(),
    });
    expect(() =>
      disjoint.register({
        id: 'local-b',
        title: 'B',
        shortcuts: [{ scope: 'prompt', shortcut: 'Enter' }],
        execute: vi.fn(),
      }),
    ).not.toThrow();
  });

  it('revalidates targets and consumes disabled local shortcuts without fallback', async () => {
    const registry = new CommandRegistry<{ target?: string }>();
    const local = vi.fn();
    const global = vi.fn();
    registry.register({
      id: 'local',
      title: 'Local',
      shortcuts: [{ scope: 'dialog', shortcut: 'Mod+Enter' }],
      resolveTarget: (context) => context.target,
      availability: (_context, target) =>
        target
          ? { available: true }
          : { available: false, reason: 'No target' },
      execute: local,
    });
    registry.register({
      id: 'global',
      title: 'Global',
      shortcuts: [{ scope: 'app', shortcut: 'Mod+Enter' }],
      execute: global,
    });

    await expect(
      registry.dispatchShortcut(
        event({ key: 'Enter' }),
        {},
        ['dialog', 'app'],
        'mac',
      ),
    ).resolves.toEqual({ status: 'unavailable', reason: 'No target' });
    expect(global).not.toHaveBeenCalled();
    await registry.dispatchShortcut(
      event({ key: 'Enter' }),
      { target: 'current' },
      ['dialog', 'app'],
      'mac',
    );
    expect(local).toHaveBeenCalledWith({ target: 'current' }, 'current');
  });

  it('does not execute a pending command twice', async () => {
    let release: (() => void) | undefined;
    const registry = new CommandRegistry<object>();
    registry.register({
      id: 'save',
      title: 'Save',
      execute: (): Promise<void> =>
        new Promise<void>((resolve: () => void) => {
          release = resolve;
        }),
    });
    const first = registry.dispatch('save', {});
    await expect(registry.dispatch('save', {})).resolves.toEqual({
      status: 'pending',
    });
    release?.();
    await expect(first).resolves.toEqual({ status: 'executed' });
  });
});

describe('stable palette filtering', () => {
  const entries = [
    { title: 'Open Project', keywords: ['local remote'] },
    { title: 'Switch Session', keywords: ['recent'] },
    { title: 'Choose Model', keywords: ['provider'] },
  ];

  it('fuzzy filters titles and aliases without reordering', () => {
    expect(filterFuzzy(entries, 'op')).toEqual([entries[0]]);
    expect(filterFuzzy(entries, 'rec')).toEqual([entries[0], entries[1]]);
    expect(filterFuzzy(entries, 'zzz')).toEqual([]);
  });

  it('returns to the root only for Backspace on an empty nested search', () => {
    expect(paletteBackspaceReturnsRoot('Backspace', '', 'sessions')).toBe(true);
    expect(paletteBackspaceReturnsRoot('Backspace', 's', 'sessions')).toBe(
      false,
    );
    expect(paletteBackspaceReturnsRoot('Backspace', '', 'root')).toBe(false);
  });
});
