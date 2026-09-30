import { describe, expect, it, vi } from 'vitest';

import { CommandRegistry, formatShortcut } from './index';

describe('command discovery', () => {
  it('discovers available variant metadata and dispatches the matching variant', async () => {
    const registry = new CommandRegistry<{
      archived: boolean;
      unread: boolean;
    }>();
    const archive = vi.fn();
    const unarchive = vi.fn();
    const read = vi.fn();
    const unread = vi.fn();
    for (const [id, title, chord, group, available, execute] of [
      [
        'archive',
        'Archive Session',
        'Mod+Shift+A',
        'archive',
        (ctx: { archived: boolean }): boolean => !ctx.archived,
        archive,
      ],
      [
        'unarchive',
        'Unarchive Session',
        'Mod+Shift+A',
        'archive',
        (ctx: { archived: boolean }): boolean => ctx.archived,
        unarchive,
      ],
      [
        'read',
        'Mark as Read',
        'Mod+Shift+U',
        'read',
        (ctx: { unread: boolean }): boolean => ctx.unread,
        read,
      ],
      [
        'unread',
        'Mark as Unread',
        'Mod+Shift+U',
        'read',
        (ctx: { unread: boolean }): boolean => !ctx.unread,
        unread,
      ],
    ] as const) {
      registry.register({
        id,
        title,
        palette: true,
        shortcuts: [{ shortcut: chord, scope: 'app', variantGroup: group }],
        availability: (ctx) =>
          available(ctx)
            ? { available: true }
            : { available: false, reason: 'Unavailable' },
        execute,
      });
    }
    const before = { archived: false, unread: true };
    expect(registry.palette(before).map(({ title }) => title)).toEqual([
      'Archive Session',
      'Mark as Read',
    ]);
    expect(
      formatShortcut(
        registry.discover('archive', before)!.definition.shortcuts![0]!
          .shortcut,
        'mac',
      ),
    ).toBe('⌘⇧A');
    expect(registry.discover('unarchive', before)?.available).toBe(false);
    await registry.dispatchShortcut(
      {
        key: 'a',
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
        repeat: false,
      },
      before,
      ['app'],
      'mac',
    );
    await registry.dispatchShortcut(
      {
        key: 'u',
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
        repeat: false,
      },
      before,
      ['app'],
      'mac',
    );
    const after = { archived: true, unread: false };
    expect(registry.palette(after).map(({ title }) => title)).toEqual([
      'Unarchive Session',
      'Mark as Unread',
    ]);
    await registry.dispatchShortcut(
      {
        key: 'a',
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
        repeat: false,
      },
      after,
      ['app'],
      'mac',
    );
    await registry.dispatchShortcut(
      {
        key: 'u',
        metaKey: true,
        ctrlKey: false,
        altKey: false,
        shiftKey: true,
        repeat: false,
      },
      after,
      ['app'],
      'mac',
    );
    for (const callback of [archive, unarchive, read, unread])
      expect(callback).toHaveBeenCalledOnce();
  });

  it('exposes local shortcut hints in the palette without activating app shortcuts', async () => {
    const registry = new CommandRegistry<{ ready: boolean }>();
    const send = vi.fn();
    registry.register({
      id: 'send',
      title: 'Send Message',
      palette: true,
      shortcuts: [{ shortcut: 'Enter', scope: 'composer' }],
      availability: ({ ready }) =>
        ready ? { available: true } : { available: false, reason: 'Empty' },
      execute: send,
    });
    expect(registry.palette({ ready: false })).toEqual([]);
    expect(registry.discover('send', { ready: false })?.available).toBe(false);
    expect(
      formatShortcut(
        registry.palette({ ready: true })[0]!.definition.shortcuts![0]!
          .shortcut,
        'mac',
      ),
    ).toBe('Enter');
    await registry.dispatchShortcut(
      {
        key: 'Enter',
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        repeat: false,
      },
      { ready: true },
      ['app'],
      'mac',
    );
    expect(send).not.toHaveBeenCalled();
  });
});
