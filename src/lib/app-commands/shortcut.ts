type ShortcutPlatform = 'mac' | 'non-mac';

type ShortcutModifier = 'Mod' | 'Ctrl' | 'Alt' | 'Shift';

type ShortcutInput =
  | string
  | {
      key: string;
      modifiers?: readonly ShortcutModifier[];
      /** Mutating commands normally leave this false. */
      allowRepeat?: boolean;
    };

type NormalizedShortcut = Readonly<{
  key: string;
  modifiers: readonly ShortcutModifier[];
  allowRepeat: boolean;
}>;

interface KeyboardShortcutEvent {
  key: string;
  /** Physical key identity, independent of the active keyboard layout. */
  code?: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  repeat: boolean;
  isComposing?: boolean;
  getModifierState?: (key: string) => boolean;
}

const modifierOrder: readonly ShortcutModifier[] = [
  'Mod',
  'Ctrl',
  'Alt',
  'Shift',
];

const modifierAliases: Readonly<Record<string, ShortcutModifier>> = {
  mod: 'Mod',
  cmd: 'Mod',
  command: 'Mod',
  ctrl: 'Ctrl',
  control: 'Ctrl',
  alt: 'Alt',
  option: 'Alt',
  opt: 'Alt',
  shift: 'Shift',
};

const keyAliases: Readonly<Record<string, string>> = {
  esc: 'Escape',
  return: 'Enter',
  space: ' ',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  up: 'ArrowUp',
  down: 'ArrowDown',
};

const shiftedPunctuation: Readonly<Record<string, string>> = {
  '~': '`',
  '!': '1',
  '@': '2',
  '#': '3',
  $: '4',
  '%': '5',
  '^': '6',
  '&': '7',
  '*': '8',
  '(': '9',
  ')': '0',
  _: '-',
  '+': '=',
  '{': '[',
  '}': ']',
  '|': '\\',
  ':': ';',
  '"': "'",
  '<': ',',
  '>': '.',
  '?': '/',
};

function normalizeKey(key: string, shift = false): string {
  const trimmed = key.trim();
  const alias = keyAliases[trimmed.toLowerCase()];
  const normalized = alias ?? trimmed;
  if (normalized.length === 1) {
    const unshifted = shift ? shiftedPunctuation[normalized] : undefined;
    return (unshifted ?? normalized).toLowerCase();
  }
  return normalized;
}

/** Parses a portable chord such as `Mod+Shift+P` into a canonical form. */
function normalizeShortcut(input: ShortcutInput): NormalizedShortcut {
  const parts = typeof input === 'string' ? input.split('+') : undefined;
  const rawKey = typeof input === 'string' ? parts?.pop() : input.key;
  if (!rawKey) throw new Error('A shortcut must include a key.');

  const rawModifiers =
    typeof input === 'string'
      ? (parts ?? [])
      : input.modifiers
        ? [...input.modifiers]
        : [];
  const modifiers = new Set<ShortcutModifier>();
  for (const modifier of rawModifiers) {
    const normalized = modifierAliases[modifier.toLowerCase()];
    if (!normalized) throw new Error(`Unknown shortcut modifier: ${modifier}`);
    modifiers.add(normalized);
  }

  return {
    key: normalizeKey(rawKey, modifiers.has('Shift')),
    modifiers: modifierOrder.filter((modifier) => modifiers.has(modifier)),
    allowRepeat:
      typeof input === 'string' ? false : (input.allowRepeat ?? false),
  };
}

function shortcutMatches(
  event: KeyboardShortcutEvent,
  input: ShortcutInput | NormalizedShortcut,
  platform: ShortcutPlatform,
): boolean {
  if (event.isComposing || event.getModifierState?.('AltGraph')) return false;
  const shortcut = normalizeShortcut(input);
  if (event.repeat && !shortcut.allowRepeat) return false;

  const required = new Set(shortcut.modifiers);
  const expectedCtrl =
    required.has('Ctrl') || (platform === 'non-mac' && required.has('Mod'));
  const expectedMeta = platform === 'mac' && required.has('Mod');
  if (
    event.ctrlKey !== expectedCtrl ||
    event.metaKey !== expectedMeta ||
    event.altKey !== required.has('Alt') ||
    event.shiftKey !== required.has('Shift')
  ) {
    return false;
  }

  if (normalizeKey(event.key, event.shiftKey) === shortcut.key) return true;

  // macOS reports the character produced by Option (for example, `ø`), while
  // `code` retains the physical letter key. Do not use this during composition
  // or AltGraph input (rejected above), where the character is intentional.
  return (
    platform === 'mac' &&
    required.has('Alt') &&
    /^[a-z]$/.test(shortcut.key) &&
    event.code === `Key${shortcut.key.toUpperCase()}`
  );
}

function formatShortcut(
  input: ShortcutInput | NormalizedShortcut,
  platform: ShortcutPlatform,
): string {
  const shortcut = normalizeShortcut(input);
  const labels: Record<ShortcutModifier, string> =
    platform === 'mac'
      ? { Mod: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' }
      : { Mod: 'Ctrl', Ctrl: 'Ctrl', Alt: 'Alt', Shift: 'Shift' };
  const key = formatKey(shortcut.key, platform);
  const modifierLabels = shortcut.modifiers.map((modifier) => labels[modifier]);
  return platform === 'mac'
    ? `${modifierLabels.join('')}${key}`
    : [...modifierLabels, key].join('+');
}

function formatKey(key: string, platform: ShortcutPlatform): string {
  const macLabels: Readonly<Record<string, string>> = {
    ArrowLeft: '←',
    ArrowRight: '→',
    ArrowUp: '↑',
    ArrowDown: '↓',
    Escape: 'Esc',
    Enter: 'Enter',
    ' ': 'Space',
  };
  const otherLabels: Readonly<Record<string, string>> = {
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ' ': 'Space',
  };
  const label = (platform === 'mac' ? macLabels : otherLabels)[key] ?? key;
  return label.length === 1 ? label.toUpperCase() : label;
}

export {
  formatShortcut,
  normalizeShortcut,
  shortcutMatches,
  type KeyboardShortcutEvent,
  type NormalizedShortcut,
  type ShortcutInput,
  type ShortcutModifier,
  type ShortcutPlatform,
};
