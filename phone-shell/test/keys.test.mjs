import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', '..', 'app', 'src', 'phone', 'keys.ts'),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const keys = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

const ctrl = { shift: false, ctrl: true, alt: false };
const shift = { shift: true, ctrl: false, alt: false };
const alt = { shift: false, ctrl: false, alt: true };
const none = { shift: false, ctrl: false, alt: false };

function memoryStorage(initial = null) {
  let value = initial;
  return {
    getItem() {
      return value;
    },
    setItem(_key, next) {
      value = next;
    },
    read() {
      return value;
    },
  };
}

test('the rail starts with every base key', () => {
  const labels = keys.railKeys([], []).map((entry) => entry.label);
  assert.deepEqual(labels, [
    'Esc',
    'Tab',
    '⇧Tab',
    '⌫',
    'Del',
    '←',
    '↑',
    '↓',
    '→',
    '^C',
    '^D',
  ]);
});

test('base keys encode the bytes a terminal expects', () => {
  assert.equal(keys.encodeKey('Escape'), '\x1b');
  assert.equal(keys.encodeKey('Tab'), '\t');
  assert.equal(keys.encodeKey('Tab', shift), '\x1b[Z');
  assert.equal(keys.encodeKey('Backspace'), '\x7f');
  assert.equal(keys.encodeKey('Delete'), '\x1b[3~');
  assert.equal(keys.encodeKey('ArrowUp'), '\x1b[A');
  assert.equal(keys.encodeKey('ArrowDown'), '\x1b[B');
  assert.equal(keys.encodeKey('ArrowRight'), '\x1b[C');
  assert.equal(keys.encodeKey('ArrowLeft'), '\x1b[D');
  assert.equal(keys.encodeKey('c', ctrl), '\x03');
  assert.equal(keys.encodeKey('d', ctrl), '\x04');
  assert.equal(keys.encodeKey('o', ctrl), '\x0f');
});

test('latches combine with the tapped key', () => {
  assert.equal(keys.encodeKey('ArrowUp', ctrl), '\x1b[1;5A');
  assert.equal(keys.encodeKey('ArrowUp', shift), '\x1b[1;2A');
  assert.equal(keys.encodeKey('ArrowLeft', alt), '\x1b[1;3D');
  assert.equal(keys.encodeKey('c', { shift: true, ctrl: true, alt: false }), '\x03');
  assert.equal(keys.encodeKey('c', alt), '\x1bc');
  assert.equal(keys.encodeKey('c', shift), 'C');
  assert.equal(keys.encodeKey('F5'), '\x1b[15~');
  assert.equal(keys.encodeKey('F5', ctrl), '\x1b[15;5~');
  assert.equal(keys.encodeKey('PageUp'), '\x1b[5~');
  assert.equal(keys.encodeKey('Home', ctrl), '\x1b[1;5H');
});

test('modifier latches toggle and clear after one key', () => {
  let state = keys.noModifiers();
  state = keys.toggleModifier(state, 'ctrl');
  assert.equal(state.ctrl, true);
  state = keys.toggleModifier(state, 'ctrl');
  assert.equal(state.ctrl, false);
  assert.deepEqual(keys.modifiersFromList(['shift', 'alt']), {
    shift: true,
    ctrl: false,
    alt: true,
  });
  const merged = keys.encodeRailKey(
    { key: 'c', modifiers: ['ctrl'] },
    none,
  );
  assert.equal(merged, '\x03');
});

test('agent keys merge into one chip with their purpose', () => {
  const shortcuts = [
    { key: 'Shift+Tab', label: '⇧Tab', name: 'Shift Tab', description: 'Cycle permission modes' },
    { key: 'Ctrl+O', label: '^O', name: 'Control O', description: 'Verbose transcript' },
    { key: 'Ctrl+T', label: '^T', name: 'Control T', description: 'To-do list' },
  ];
  const rail = keys.railKeys(shortcuts, []);
  const shiftTab = rail.filter((entry) => entry.label === '⇧Tab');
  assert.equal(shiftTab.length, 1);
  assert.equal(shiftTab[0].description, 'Cycle permission modes');
  assert.equal(shiftTab[0].source, 'base');
  assert.equal(rail.filter((entry) => entry.label === '^O').length, 1);
  assert.equal(
    rail.find((entry) => entry.label === '^O').description,
    'Verbose transcript',
  );
});

test('an agent with no shortcut keeps the base keys only', () => {
  const rail = keys.railKeys([], []);
  assert.equal(rail.length, 11);
  assert.equal(rail.some((entry) => entry.source === 'agent'), false);
});

test('arrows, Backspace and Delete repeat when held', () => {
  for (const key of ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Backspace', 'Delete']) {
    assert.equal(keys.isRepeatable(key), true, key);
  }
  for (const key of ['Tab', 'Escape', 'c']) {
    assert.equal(keys.isRepeatable(key), false, key);
  }
});

test('saved keys validate, add and remove', () => {
  const letter = keys.savedKeyFrom({ name: 'C', modifiers: ['ctrl'] });
  assert.equal(letter.key, 'c');
  assert.deepEqual(letter.modifiers, ['ctrl']);
  assert.equal(keys.savedKeyFrom({ name: 'nonsense', modifiers: [] }), null);
  assert.equal(keys.savedKeyFrom({ name: '', modifiers: [] }), null);

  let list = [];
  list = keys.addSavedKey(list, letter).keys;
  const functionKey = keys.savedKeyFrom({ name: 'F5', modifiers: ['shift'] });
  list = keys.addSavedKey(list, functionKey).keys;
  assert.equal(list.length, 2);
  assert.deepEqual(
    keys.addSavedKey(list, letter),
    { keys: list, error: 'duplicate' },
  );

  list = keys.removeSavedKey(list, letter.id);
  assert.deepEqual(list.map((entry) => entry.key), ['F5']);
});

test('at most eight saved keys are accepted', () => {
  let list = [];
  for (let index = 1; index <= 8; index += 1) {
    const saved = keys.savedKeyFrom({ name: `F${index}`, modifiers: [] });
    const result = keys.addSavedKey(list, saved);
    assert.equal(result.error, undefined, `F${index}`);
    list = result.keys;
  }
  const ninth = keys.savedKeyFrom({ name: 'F9', modifiers: [] });
  assert.deepEqual(keys.addSavedKey(list, ninth), { keys: list, error: 'full' });
});

test('saved keys round-trip through storage', () => {
  const storage = memoryStorage();
  const saved = keys.savedKeyFrom({ name: 'PageUp', modifiers: ['alt'] });
  keys.writeSavedKeys(storage, [saved]);
  const read = keys.readSavedKeys(storage);
  assert.equal(read.length, 1);
  assert.equal(read[0].key, 'PageUp');
  assert.deepEqual(read[0].modifiers, ['alt']);
});

test('a broken stored value gives an empty list', () => {
  assert.deepEqual(keys.readSavedKeys(memoryStorage('not json')), []);
  assert.deepEqual(keys.readSavedKeys(memoryStorage('{"nope":true}')), []);
  assert.deepEqual(keys.readSavedKeys(memoryStorage('[{"key":"??","modifiers":[]}]')), []);
  assert.deepEqual(keys.readSavedKeys(null), []);
});

test('a saved key shows its modifiers on the rail', () => {
  const saved = keys.savedKeyFrom({ name: 'C', modifiers: ['shift', 'ctrl'] });
  assert.equal(keys.savedKeyLabel(saved.key, saved.modifiers), '⇧^C');
  const rail = keys.railKeys([], [saved]);
  assert.equal(rail.at(-1).label, '⇧^C');
  assert.equal(rail.at(-1).source, 'saved');
});
