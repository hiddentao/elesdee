import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', '..', 'app', 'src', 'phone', 'history.ts'),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const history = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function item(seq, id, overrides = {}) {
  return {
    seq,
    id,
    kind: 'reply',
    text: `text ${seq}`,
    state: 'streaming',
    ...overrides,
  };
}

test('retained items keep their replay order', () => {
  let items = [];
  items = history.mergeHistory(items, item(1, 'prompt', { kind: 'prompt' }));
  items = history.mergeHistory(items, item(2, 'reply'));
  items = history.mergeHistory(items, item(3, 'tool', { kind: 'tool' }));
  assert.deepEqual(
    items.map((entry) => entry.id),
    ['prompt', 'reply', 'tool'],
  );
});

test('an item update replaces the older version in place', () => {
  let items = history.mergeHistory([], item(1, 'prompt', { kind: 'prompt' }));
  items = history.mergeHistory(items, item(2, 'reply'));
  items = history.mergeHistory(
    items,
    item(4, 'reply', { text: 'final text', state: 'done' }),
  );
  assert.equal(items.length, 2);
  assert.equal(items[1].text, 'final text');
  assert.equal(items[1].state, 'done');
  assert.equal(items[0].id, 'prompt');
});

test('an older version never replaces a newer one', () => {
  const items = [item(5, 'reply', { text: 'newest', state: 'done' })];
  const after = history.mergeHistory(items, item(4, 'reply', { text: 'old' }));
  assert.equal(after, items);
});

test('a reconnect replay adds the gap and ignores what was seen', () => {
  let items = history.mergeHistory([], item(1, 'prompt', { kind: 'prompt' }));
  items = history.mergeHistory(items, item(2, 'reply'));

  // The reconnect replays from the last sequence. The retained item at 1 is
  // seen again, the item at 3 is new, and the live version at 4 updates 2.
  items = history.mergeHistory(items, item(1, 'prompt', { kind: 'prompt' }));
  items = history.mergeHistory(items, item(3, 'tool', { kind: 'tool' }));
  items = history.mergeHistory(
    items,
    item(4, 'reply', { text: 'after reconnect', state: 'done' }),
  );

  assert.deepEqual(
    items.map((entry) => entry.id),
    ['prompt', 'reply', 'tool'],
  );
  assert.equal(items[1].text, 'after reconnect');
  assert.equal(items[1].state, 'done');
});

test('only agent clients with a structured source get the message view', () => {
  assert.equal(history.historySupported('Claude Code', 'agent'), true);
  assert.equal(history.historySupported('Codex', 'agent'), true);
  assert.equal(history.historySupported('OpenCode 2', 'agent'), true);
  assert.equal(history.historySupported('Antigravity', 'agent'), false);
  assert.equal(history.historySupported('Claude Code', 'terminal'), false);
  assert.equal(history.historySupported(null, 'agent'), false);
});
