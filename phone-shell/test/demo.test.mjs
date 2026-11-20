import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'ui', 'src', 'demo.ts'), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const demo = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function server(id, name, port, overrides = {}) {
  return {
    id,
    sessionId: 's1',
    name,
    port,
    address: `http://localhost:${port}`,
    state: 'active',
    createdAt: '2026-11-20T00:00:00Z',
    ...overrides,
  };
}

test('the list shows only the active servers of one session', () => {
  const servers = demo.sessionDemoServers(
    [
      server('a', 'Preview', 3000),
      server('b', 'Stopped', 3001, { state: 'stopped' }),
      server('c', 'Other session', 3002, { sessionId: 's2' }),
      server('d', 'Duplicate port', 3000),
      server('e', 'Docs', 4000),
      { name: 'Not a server' },
    ],
    's1',
  );
  assert.deepEqual(
    servers.map((entry) => entry.id),
    ['a', 'e'],
  );
});

test('a session keeps at most eight servers', () => {
  const servers = Array.from({ length: 10 }, (_, index) =>
    server(`s${index}`, `Server ${index}`, 3000 + index),
  );
  assert.equal(demo.sessionDemoServers(servers, 's1').length, 8);
  assert.equal(demo.sessionDemoServers(null, 's1').length, 0);
});

test('a ready forward shows its local address and state', () => {
  const forwards = new Map([
    [3000, { phase: 'ready', address: 'http://127.0.0.1:3000', sessionId: 's1' }],
    [4000, { phase: 'starting', sessionId: 's1' }],
    [5000, { phase: 'unavailable', sessionId: 's1' }],
    [6000, { phase: 'ready', address: 'http://127.0.0.1:6000', sessionId: 's2' }],
  ]);
  const rows = demo.demoServerRows(
    [
      server('a', 'Preview', 3000),
      server('b', 'Docs', 4000),
      server('c', 'Storybook', 5000),
      server('d', 'Report', 6000),
    ],
    forwards,
    's1',
  );
  assert.deepEqual(
    rows.map((row) => [row.server.port, row.address, row.state]),
    [
      [3000, 'http://127.0.0.1:3000', 'demoServer.ready'],
      [4000, null, 'demoServer.starting'],
      [5000, null, 'demoServer.unavailable'],
      [6000, null, 'demoServer.starting'],
    ],
  );
});

test('the shell error codes map to a listener state', () => {
  assert.equal(demo.forwardPhaseFor('unavailable'), 'unavailable');
  assert.equal(demo.forwardPhaseFor('not_registered'), 'unavailable');
  assert.equal(demo.forwardPhaseFor('timeout'), 'failed');
  assert.equal(demo.forwardPhaseFor('limit'), 'failed');
  assert.equal(
    demo.demoStateClass('demoServer.ready'),
    'demo-state is-ready',
  );
  assert.equal(
    demo.demoStateClass('demoServer.failed'),
    'demo-state is-failed',
  );
});
