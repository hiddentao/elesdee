import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', '..', 'app', 'src', 'phone', 'sessionActions.ts'),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const actions = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

const configs = [
  { id: 'a', name: 'Alpha', providerId: 'max', client: 'Claude Code' },
  { id: 'b', name: 'Beta', providerId: 'deepseek', client: 'Claude Code' },
  { id: 'c', name: 'Gamma', providerId: 'codex', client: 'Codex' },
];

test('a config label names the config and its client', () => {
  assert.equal(actions.configChoiceLabel(configs[0]), 'Alpha · Claude Code');
});

test('the switch lists only allowed configs except the current one', () => {
  const session = {
    id: 's1',
    name: 'Session',
    kind: 'agent',
    status: 'active',
    configId: 'a',
    allowedConfigIds: ['a', 'b'],
  };
  assert.deepEqual(
    actions.switchableConfigs(configs, session).map((config) => config.id),
    ['b'],
  );
  assert.deepEqual(
    actions.switchableConfigs(configs, { ...session, allowedConfigIds: ['a'] }),
    [],
  );
});

test('a terminal session cannot switch', () => {
  assert.equal(
    actions.canSwitch({
      id: 't',
      name: 'Terminal',
      kind: 'terminal',
      status: 'active',
    }),
    false,
  );
  assert.equal(
    actions.canSwitch({
      id: 'a',
      name: 'Agent',
      kind: 'agent',
      status: 'active',
      allowedConfigIds: ['a', 'b'],
    }),
    true,
  );
});

test('the picker filters on the label and the id', () => {
  assert.equal(
    actions.filterChoices(configs, 'bet', (config) => config.name).length,
    1,
  );
  assert.equal(
    actions.filterChoices(configs, 'GAMMA', (config) => config.name)[0].id,
    'c',
  );
  assert.equal(actions.filterChoices(configs, '  ', (config) => config.name).length, 3);
});

test('the start request leaves out empty optional values', () => {
  assert.deepEqual(
    actions.buildStartInput({
      projectId: 'web',
      configId: 'a',
      sourceBranch: '',
      name: '  ',
      firstPrompt: '',
    }),
    { projectId: 'web', configId: 'a', kind: 'agent' },
  );
  assert.deepEqual(
    actions.buildStartInput({
      projectId: 'web',
      configId: 'a',
      sourceBranch: ' feature ',
      name: ' Build ',
      firstPrompt: ' Fix it ',
    }),
    {
      projectId: 'web',
      configId: 'a',
      kind: 'agent',
      sourceBranch: 'feature',
      name: 'Build',
      firstPrompt: 'Fix it',
    },
  );
  assert.equal(
    actions.buildStartInput({ projectId: null, configId: 'a' }),
    null,
  );
});

test('close uses the same copy for the session kind', () => {
  assert.equal(actions.closeBodyKey('agent'), 'close.agentBody');
  assert.equal(actions.closeBodyKey('terminal'), 'close.terminalBody');
});

test('the newest pending question and request belong to their session', () => {
  const questions = [
    { id: 'q1', sessionId: 's1', status: 'answered' },
    { id: 'q2', sessionId: 's1', status: 'pending' },
    { id: 'q3', sessionId: 's2', status: 'pending' },
  ];
  assert.equal(actions.pendingQuestion(questions, 's1').id, 'q2');
  assert.equal(actions.pendingQuestion(questions, 's3'), null);

  const requests = [
    { id: 'r1', parentSessionId: 's1', status: 'pending' },
    { id: 'r2', parentSessionId: 's1', status: 'denied' },
  ];
  assert.equal(actions.pendingRequest(requests, 's1').id, 'r1');
  assert.equal(actions.pendingRequest(requests, 's2'), null);
});

test('usage text names each reported window', () => {
  assert.equal(
    actions.usageSummary({
      id: 'max',
      name: 'Max',
      rows: [
        { label: '5-hour', used: 40 },
        { label: 'Weekly', used: 70 },
      ],
    }),
    '5-hour 40% · Weekly 70%',
  );
  assert.equal(
    actions.usageSummary({
      id: 'deepseek',
      name: 'DeepSeek',
      rows: [{ label: 'Balance', value: '$12.40' }],
    }),
    'Balance $12.40',
  );
  assert.equal(actions.usageSummary({ id: 'x', name: 'X', rows: [] }), null);
});

test('a permission card appears only with a pending permission', () => {
  const base = {
    id: 's1',
    name: 'Session',
    kind: 'agent',
    status: 'active',
  };
  assert.equal(actions.hasPermission(base), false);
  assert.equal(
    actions.hasPermission({
      ...base,
      pendingPermission: { requestId: 'r', tool: 'Bash', summary: 'ls' },
    }),
    true,
  );
});
