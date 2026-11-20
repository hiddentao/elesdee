import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'ui', 'src', 'messages.ts'), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const messages = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function item(overrides = {}) {
  return {
    seq: 1,
    id: 'item-1',
    kind: 'prompt',
    text: 'Fix the parser',
    state: 'done',
    ...overrides,
  };
}

test('a conversation item is read from a stream event', () => {
  assert.deepEqual(messages.historyItemFromEvent(item()), item());
  assert.deepEqual(
    messages.historyItemFromEvent(
      item({ kind: 'tool', tool: 'Bash', text: 'npm test', state: 'running' }),
    ),
    {
      seq: 1,
      id: 'item-1',
      kind: 'tool',
      tool: 'Bash',
      text: 'npm test',
      state: 'running',
    },
  );
});

test('an incomplete or unknown conversation item is refused', () => {
  assert.equal(messages.historyItemFromEvent({ text: 'no sequence' }), null);
  assert.equal(messages.historyItemFromEvent(item({ seq: Number.NaN })), null);
  assert.equal(messages.historyItemFromEvent(item({ id: '' })), null);
  assert.equal(messages.historyItemFromEvent(item({ text: undefined })), null);
  assert.equal(messages.historyItemFromEvent(item({ kind: 'other' })), null);
  assert.equal(messages.historyItemFromEvent(item({ state: 'other' })), null);
  assert.equal(messages.historyItemFromEvent('nonsense'), null);
});

test('the stream status is told apart from a conversation item', () => {
  const status = messages.historyStatusFromEvent({
    type: 'history.state',
    structured: true,
    client: 'Codex',
  });
  assert.deepEqual(status, {
    type: 'history.state',
    structured: true,
    client: 'Codex',
  });
  assert.equal(messages.historyStatusFromEvent(item()), null);
  assert.equal(
    messages.historyStatusFromEvent({ type: 'history.state' }),
    null,
  );
  assert.equal(messages.historyItemFromEvent(status), null);
});

test('a tool row opens and closes on tap', () => {
  const empty = new Set();
  const opened = messages.toggleExpanded(empty, 'tool-1');
  assert.deepEqual([...opened], ['tool-1']);
  assert.deepEqual([...empty], [], 'the previous set is not changed');
  const closed = messages.toggleExpanded(opened, 'tool-1');
  assert.deepEqual([...closed], []);
});

test('tool states map to shell catalog keys', () => {
  assert.equal(messages.toolStateKey('streaming'), 'messages.toolRunning');
  assert.equal(messages.toolStateKey('running'), 'messages.toolRunning');
  assert.equal(messages.toolStateKey('failed'), 'messages.toolFailed');
  assert.equal(messages.toolStateKey('done'), 'messages.toolDone');
});
