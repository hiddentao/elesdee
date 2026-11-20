import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'ui', 'src', 'live.ts'), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const live = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function session(id, name) {
  return {
    id,
    name,
    short: id,
    projectId: 'web',
    kind: 'terminal',
    status: 'active',
    workState: 'working',
    preview: '',
    waitingFor: null,
    pendingPermission: null,
    startedAt: '2026-11-20T00:00:00Z',
  };
}

test('a session update replaces the row in place', () => {
  const first = session('a', 'alpha');
  const second = session('b', 'beta');
  const updated = { ...first, name: 'renamed' };
  const result = live.mergeSession([first, second], updated);
  assert.equal(result.length, 2);
  assert.equal(result[0].name, 'renamed');
  assert.equal(result[1].id, 'b');
});

test('a new session is appended at the end', () => {
  const first = session('a', 'alpha');
  const second = session('b', 'beta');
  const result = live.mergeSession([first], second);
  assert.deepEqual(
    result.map((value) => value.id),
    ['a', 'b'],
  );
});

test('output events append text and track the sequence', () => {
  let state = { text: '', lastSeq: 0, dropped: false };
  state = live.appendOutput(state, { seq: 1, data: 'hello ' });
  state = live.appendOutput(state, { seq: 2, data: 'world' });
  assert.equal(state.text, 'hello world');
  assert.equal(state.lastSeq, 2);
  assert.equal(state.dropped, false);
});

test('a repeated sequence is ignored', () => {
  const state = { text: 'hello', lastSeq: 4, dropped: false };
  const after = live.appendOutput(state, { seq: 4, data: ' again' });
  assert.equal(after, state);
});

test('a gap marks that old output is missing', () => {
  const state = { text: '', lastSeq: 0, dropped: false };
  const after = live.appendOutput(state, {
    seq: 9,
    data: 'new',
    gap: { after: 0, oldestSeq: 9, newestSeq: 9 },
  });
  assert.equal(after.text, 'new');
  assert.equal(after.dropped, true);
});

test('the visible output keeps the newest text under the cap', () => {
  const state = { text: 'abcdef', lastSeq: 0, dropped: false };
  const after = live.appendOutput(state, { seq: 1, data: 'ghij' }, 5);
  assert.equal(after.text, 'fghij');
  assert.equal(after.dropped, true);
});

test('session and output events are read safely', () => {
  const update = live.sessionFromEvent({
    type: 'session.updated',
    session: session('a', 'alpha'),
  });
  assert.equal(update.id, 'a');
  assert.equal(live.sessionFromEvent({ type: 'notification' }), null);
  assert.equal(live.sessionFromEvent('nonsense'), null);

  const output = live.outputFromEvent({ seq: 3, data: 'text' });
  assert.deepEqual(output, { seq: 3, data: 'text' });
  assert.equal(live.outputFromEvent({ data: 'no sequence' }), null);
});
