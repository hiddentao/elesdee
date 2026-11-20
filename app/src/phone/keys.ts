/**
 * Terminal key definitions and encoding for the phone interfaces.
 *
 * The phone sends the bytes a terminal expects. Nothing here runs an action in
 * the interface, and nothing depends on the desktop window.
 */

export type Modifier = 'shift' | 'ctrl' | 'alt';

export interface KeyModifiers {
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
}

export interface KeySpec {
  key: string;
  modifiers: Modifier[];
  label: string;
  name: string;
  /** Catalog key for the spoken name of a base key. */
  nameKey?: string;
}

export interface AgentShortcut {
  key: string;
  label: string;
  name: string;
  description: string;
}

export type KeySource = 'base' | 'agent' | 'saved';

export interface RailKey extends KeySpec {
  id: string;
  description?: string;
  source: KeySource;
}

export interface SavedKey {
  id: string;
  key: string;
  modifiers: Modifier[];
}

export const MAX_SAVED_KEYS = 8;
export const SAVED_KEYS_STORAGE = 'elesdee.phone.savedKeys';

const REPEATABLE = new Set([
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Backspace',
  'Delete',
]);

/** Base keys every session gets. Agent shortcuts add to these. */
export const BASE_KEYS: readonly KeySpec[] = [
  { key: 'Escape', modifiers: [], label: 'Esc', name: 'Escape', nameKey: 'phone.key.escape' },
  { key: 'Tab', modifiers: [], label: 'Tab', name: 'Tab', nameKey: 'phone.key.tab' },
  { key: 'Tab', modifiers: ['shift'], label: '⇧Tab', name: 'Shift Tab', nameKey: 'phone.key.shiftTab' },
  { key: 'Backspace', modifiers: [], label: '⌫', name: 'Backspace', nameKey: 'phone.key.backspace' },
  { key: 'Delete', modifiers: [], label: 'Del', name: 'Delete', nameKey: 'phone.key.delete' },
  { key: 'ArrowLeft', modifiers: [], label: '←', name: 'Left arrow', nameKey: 'phone.key.left' },
  { key: 'ArrowUp', modifiers: [], label: '↑', name: 'Up arrow', nameKey: 'phone.key.up' },
  { key: 'ArrowDown', modifiers: [], label: '↓', name: 'Down arrow', nameKey: 'phone.key.down' },
  { key: 'ArrowRight', modifiers: [], label: '→', name: 'Right arrow', nameKey: 'phone.key.right' },
  { key: 'c', modifiers: ['ctrl'], label: '^C', name: 'Control C', nameKey: 'phone.key.ctrlC' },
  { key: 'd', modifiers: ['ctrl'], label: '^D', name: 'Control D', nameKey: 'phone.key.ctrlD' },
];

const KEY_ALIASES: Record<string, string> = {
  esc: 'Escape',
  escape: 'Escape',
  tab: 'Tab',
  backspace: 'Backspace',
  delete: 'Delete',
  del: 'Delete',
  insert: 'Insert',
  ins: 'Insert',
  home: 'Home',
  end: 'End',
  pageup: 'PageUp',
  pgup: 'PageUp',
  pagedown: 'PageDown',
  pgdn: 'PageDown',
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  arrowup: 'ArrowUp',
  arrowdown: 'ArrowDown',
  arrowleft: 'ArrowLeft',
  arrowright: 'ArrowRight',
  enter: 'Enter',
  return: 'Enter',
  space: ' ',
};

function canonicalKey(raw: string): string | null {
  const value = raw.trim();
  if (value.length === 0) {
    return null;
  }
  const alias = KEY_ALIASES[value.toLowerCase()];
  if (alias) {
    return alias;
  }
  if (/^f([1-9]|1[0-2])$/.test(value.toLowerCase())) {
    return value.toUpperCase();
  }
  if (value.length === 1) {
    return value.toLowerCase();
  }
  return null;
}

export function noModifiers(): KeyModifiers {
  return { shift: false, ctrl: false, alt: false };
}

export function hasModifiers(modifiers: KeyModifiers): boolean {
  return modifiers.shift || modifiers.ctrl || modifiers.alt;
}

export function toggleModifier(
  modifiers: KeyModifiers,
  modifier: Modifier,
): KeyModifiers {
  return { ...modifiers, [modifier]: !modifiers[modifier] };
}

export function modifiersFromList(list: readonly Modifier[]): KeyModifiers {
  return {
    shift: list.includes('shift'),
    ctrl: list.includes('ctrl'),
    alt: list.includes('alt'),
  };
}

function mergeModifiers(
  base: readonly Modifier[],
  latches: KeyModifiers,
): KeyModifiers {
  return {
    shift: latches.shift || base.includes('shift'),
    ctrl: latches.ctrl || base.includes('ctrl'),
    alt: latches.alt || base.includes('alt'),
  };
}

/** True when a key repeats while the user holds it. */
export function isRepeatable(key: string): boolean {
  return REPEATABLE.has(key);
}

function modifierParameter(modifiers: KeyModifiers): number {
  return (
    1 +
    (modifiers.shift ? 1 : 0) +
    (modifiers.alt ? 2 : 0) +
    (modifiers.ctrl ? 4 : 0)
  );
}

function csi(parameter: string, final: string, modifiers: KeyModifiers): string {
  const value = modifierParameter(modifiers);
  if (value === 1) {
    return parameter === '1'
      ? `\x1b[${final}`
      : `\x1b[${parameter}${final}`;
  }
  return `\x1b[${parameter};${value}${final}`;
}

function namedKey(key: string, modifiers: KeyModifiers): string | null {
  switch (key) {
    case 'Escape':
      return modifiers.alt ? '\x1b\x1b' : '\x1b';
    case 'Tab':
      if (modifiers.shift) {
        return modifiers.alt ? '\x1b\x1b[Z' : '\x1b[Z';
      }
      return modifiers.alt ? '\x1b\t' : '\t';
    case 'Enter':
      return modifiers.alt ? '\x1b\r' : '\r';
    case 'Backspace':
      if (modifiers.ctrl) {
        return '\x08';
      }
      return modifiers.alt ? '\x1b\x7f' : '\x7f';
    case 'Delete':
      return csi('3', '~', modifiers);
    case 'Insert':
      return csi('2', '~', modifiers);
    case 'Home':
      return modifierParameter(modifiers) === 1
        ? '\x1b[H'
        : `\x1b[1;${modifierParameter(modifiers)}H`;
    case 'End':
      return modifierParameter(modifiers) === 1
        ? '\x1b[F'
        : `\x1b[1;${modifierParameter(modifiers)}F`;
    case 'PageUp':
      return csi('5', '~', modifiers);
    case 'PageDown':
      return csi('6', '~', modifiers);
    case 'ArrowUp':
      return csi('1', 'A', modifiers);
    case 'ArrowDown':
      return csi('1', 'B', modifiers);
    case 'ArrowRight':
      return csi('1', 'C', modifiers);
    case 'ArrowLeft':
      return csi('1', 'D', modifiers);
    case 'F1':
    case 'F2':
    case 'F3':
    case 'F4': {
      const final = { F1: 'P', F2: 'Q', F3: 'R', F4: 'S' }[key];
      const value = modifierParameter(modifiers);
      return value === 1 ? `\x1bO${final}` : `\x1b[1;${value}${final}`;
    }
    case 'F5':
    case 'F6':
    case 'F7':
    case 'F8':
    case 'F9':
    case 'F10':
    case 'F11':
    case 'F12': {
      const parameter = {
        F5: '15',
        F6: '17',
        F7: '18',
        F8: '19',
        F9: '20',
        F10: '21',
        F11: '23',
        F12: '24',
      }[key];
      return csi(parameter, '~', modifiers);
    }
    default:
      return null;
  }
}

function controlCode(character: string): string | null {
  if (character === '?' || character === '/') {
    return '\x7f';
  }
  const code = character.toUpperCase().charCodeAt(0);
  if (code < 32 || code > 126) {
    return null;
  }
  return String.fromCharCode(code & 0x1f);
}

/**
 * Encodes one key and its modifiers as the bytes a terminal reads.
 * Unknown key names are sent as their literal text.
 */
export function encodeKey(
  key: string,
  modifiers: KeyModifiers = noModifiers(),
): string {
  const named = namedKey(key, modifiers);
  if (named !== null) {
    return named;
  }
  if (key.length === 0) {
    return '';
  }
  if (key === ' ') {
    return modifiers.alt ? '\x1b ' : ' ';
  }
  if (key.length === 1) {
    if (modifiers.ctrl) {
      const code = controlCode(key);
      if (code !== null) {
        return code;
      }
    }
    let character = key;
    if (modifiers.shift && /[a-z]/.test(character)) {
      character = character.toUpperCase();
    }
    return modifiers.alt ? `\x1b${character}` : character;
  }
  return modifiers.alt ? `\x1b${key}` : key;
}

/** Splits an agent shortcut such as `Ctrl+O` into a key and modifiers. */
export function parseShortcut(shortcut: string): {
  key: string;
  modifiers: Modifier[];
} | null {
  const parts = shortcut
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length === 0) {
    return null;
  }
  const rawKey = parts.pop() as string;
  const key = canonicalKey(rawKey);
  if (!key) {
    return null;
  }
  const modifiers: Modifier[] = [];
  for (const part of parts) {
    const value = part.toLowerCase();
    if (value === 'shift' || value === 'ctrl' || value === 'alt') {
      if (!modifiers.includes(value)) {
        modifiers.push(value);
      }
    } else {
      return null;
    }
  }
  return { key, modifiers };
}

function sameKey(
  key: string,
  modifiers: readonly Modifier[],
  otherKey: string,
  otherModifiers: readonly Modifier[],
): boolean {
  return (
    key === otherKey &&
    modifiers.length === otherModifiers.length &&
    modifiers.every((modifier) => otherModifiers.includes(modifier))
  );
}

/**
 * Builds the rail: base keys, agent keys and saved keys. A key the rail
 * already has keeps one chip and takes the agent's purpose as its detail.
 */
export function railKeys(
  shortcuts: readonly AgentShortcut[],
  saved: readonly SavedKey[],
): RailKey[] {
  const keys: RailKey[] = BASE_KEYS.map((spec, index) => ({
    ...spec,
    id: `base:${index}`,
    source: 'base',
  }));
  for (const shortcut of shortcuts) {
    const parsed = parseShortcut(shortcut.key);
    if (!parsed) {
      continue;
    }
    const existing = keys.find((entry) =>
      sameKey(entry.key, entry.modifiers, parsed.key, parsed.modifiers),
    );
    if (existing) {
      existing.description = shortcut.description;
      existing.name = shortcut.name || existing.name;
      continue;
    }
    keys.push({
      id: `agent:${keys.length}`,
      key: parsed.key,
      modifiers: parsed.modifiers,
      label: shortcut.label,
      name: shortcut.name || shortcut.key,
      description: shortcut.description,
      source: 'agent',
    });
  }
  for (const entry of saved) {
    keys.push({
      id: entry.id,
      key: entry.key,
      modifiers: entry.modifiers,
      label: savedKeyLabel(entry.key, entry.modifiers),
      name: savedKeyLabel(entry.key, entry.modifiers),
      source: 'saved',
    });
  }
  return keys;
}

/** The chip text for a saved key. */
export function savedKeyLabel(
  key: string,
  modifiers: readonly Modifier[],
): string {
  const prefix = (['shift', 'ctrl', 'alt'] as const)
    .filter((modifier) => modifiers.includes(modifier))
    .map((modifier) =>
      modifier === 'shift' ? '⇧' : modifier === 'ctrl' ? '^' : '⌥',
    )
    .join('');
  const label =
    key === ' ' ? 'Space' : key === 'Escape' ? 'Esc' : key.length === 1 ? key.toUpperCase() : key;
  return `${prefix}${label}`;
}

/** The bytes for a rail key, with the latches added. */
export function encodeRailKey(
  entry: Pick<KeySpec, 'key' | 'modifiers'>,
  latches: KeyModifiers,
): string {
  return encodeKey(entry.key, mergeModifiers(entry.modifiers, latches));
}

/** Validates a typed key name and builds a saved key. */
export function savedKeyFrom(input: {
  name: string;
  modifiers: readonly Modifier[];
}): SavedKey | null {
  const key = canonicalKey(input.name);
  if (!key) {
    return null;
  }
  const modifiers = (['shift', 'ctrl', 'alt'] as const).filter((modifier) =>
    input.modifiers.includes(modifier),
  );
  return { id: savedKeyId(key, modifiers), key, modifiers };
}

function savedKeyId(key: string, modifiers: readonly Modifier[]): string {
  const ordered = (['shift', 'ctrl', 'alt'] as const).filter((modifier) =>
    modifiers.includes(modifier),
  );
  return `saved:${ordered.join('+')}:${key}`;
}

/** Adds a key, or reports why it cannot be added. */
export function addSavedKey(
  keys: readonly SavedKey[],
  key: SavedKey,
): { keys: SavedKey[]; error?: 'full' | 'duplicate' } {
  if (keys.some((entry) => entry.id === key.id)) {
    return { keys: [...keys], error: 'duplicate' };
  }
  if (keys.length >= MAX_SAVED_KEYS) {
    return { keys: [...keys], error: 'full' };
  }
  return { keys: [...keys, key] };
}

export function removeSavedKey(
  keys: readonly SavedKey[],
  id: string,
): SavedKey[] {
  return keys.filter((entry) => entry.id !== id);
}

export interface KeyStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Reads saved keys safely. A broken or foreign value gives an empty list. */
export function readSavedKeys(storage: KeyStorage | null): SavedKey[] {
  if (!storage) {
    return [];
  }
  try {
    const raw = storage.getItem(SAVED_KEYS_STORAGE);
    if (!raw) {
      return [];
    }
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) {
      return [];
    }
    const keys: SavedKey[] = [];
    for (const item of value) {
      if (typeof item !== 'object' || item === null) {
        continue;
      }
      const entry = item as Partial<SavedKey>;
      const key = typeof entry.key === 'string' ? canonicalKey(entry.key) : null;
      const modifiers = Array.isArray(entry.modifiers)
        ? entry.modifiers.filter(
            (modifier): modifier is Modifier =>
              modifier === 'shift' || modifier === 'ctrl' || modifier === 'alt',
          )
        : [];
      if (!key) {
        continue;
      }
      const id = savedKeyId(key, modifiers);
      if (!keys.some((candidate) => candidate.id === id)) {
        keys.push({ id, key, modifiers });
      }
    }
    return keys.slice(0, MAX_SAVED_KEYS);
  } catch {
    return [];
  }
}

export function writeSavedKeys(
  storage: KeyStorage | null,
  keys: readonly SavedKey[],
): void {
  if (!storage) {
    return;
  }
  try {
    storage.setItem(SAVED_KEYS_STORAGE, JSON.stringify(keys.slice(0, MAX_SAVED_KEYS)));
  } catch {
    // Storage can be unavailable in a private window. The rail still works.
  }
}
