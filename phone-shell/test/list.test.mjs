import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', '..', 'app', 'src', 'phone', 'list.ts'),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const list = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function session(overrides = {}) {
  return {
    status: 'active',
    workState: 'working',
    waitingFor: null,
    preview: '',
    projectId: 'web',
    ...overrides,
  };
}

test('the running filter keeps working and waiting sessions', () => {
  assert.equal(list.matchesPhoneFilter(session(), 'running'), true);
  assert.equal(
    list.matchesPhoneFilter(
      session({ workState: 'waiting', waitingFor: 'input' }),
      'running',
    ),
    true,
  );
});

test('the running filter drops paused, done and closed sessions', () => {
  assert.equal(
    list.matchesPhoneFilter(session({ workState: 'paused' }), 'running'),
    false,
  );
  assert.equal(
    list.matchesPhoneFilter(session({ workState: 'done' }), 'running'),
    false,
  );
  assert.equal(
    list.matchesPhoneFilter(
      session({ status: 'closed', workState: null }),
      'running',
    ),
    false,
  );
});

test('the waiting filter keeps only sessions that need me', () => {
  assert.equal(
    list.matchesPhoneFilter(session({ waitingFor: 'question' }), 'waiting'),
    true,
  );
  assert.equal(list.matchesPhoneFilter(session(), 'waiting'), false);
  assert.equal(list.matchesPhoneFilter(session(), 'all'), true);
});

test('a waiting row keeps its preview instead of repeating the heading', () => {
  assert.equal(
    list.rowPreview(
      session({ waitingFor: 'input', preview: 'Waiting to push the fix' }),
    ),
    'Waiting to push the fix',
  );
  assert.equal(list.rowPreview(session({ preview: '' })), 'web');
});

test('no projects blocks a new session before any field is chosen', () => {
  assert.equal(list.startBlocker([], null, null), 'noProjects');
  assert.equal(list.startBlocker([], 'p1', 'c1'), 'noProjects');
});

test('a project and an AI config are both required to start', () => {
  assert.equal(list.startBlocker([{ id: 'p1' }], null, null), 'incomplete');
  assert.equal(list.startBlocker([{ id: 'p1' }], 'p1', null), 'incomplete');
  assert.equal(list.startBlocker([{ id: 'p1' }], null, 'c1'), 'incomplete');
  assert.equal(list.startBlocker([{ id: 'p1' }], 'p1', 'c1'), null);
});
