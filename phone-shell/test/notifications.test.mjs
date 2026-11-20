import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', 'ui', 'src', 'notifications.ts'),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const notifications = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

const VISIBLE_ELSEWHERE = { sessionId: null, appVisible: true };
const HIDDEN = { sessionId: null, appVisible: false };

function session(overrides = {}) {
  return {
    id: 's1',
    name: 'Build',
    short: 's1',
    projectId: 'web',
    kind: 'agent',
    status: 'active',
    workState: 'waiting',
    preview: '',
    waitingFor: null,
    pendingPermission: null,
    startedAt: '2026-11-20T00:00:00Z',
    ...overrides,
  };
}

function notice(event, screen = VISIBLE_ELSEWHERE) {
  return notifications.noticeForEvent(event, screen);
}

test('a session that starts waiting raises a waiting notice', () => {
  const result = notice({
    type: 'session',
    session: session({ waitingFor: 'permission' }),
    previous: session(),
  });
  assert.deepEqual(result, {
    kind: 'waiting',
    sessionId: 's1',
    sessionName: 'Build',
    waitingFor: 'permission',
  });
});

test('a repeated waiting state does not raise a second notice', () => {
  const waiting = session({ waitingFor: 'input' });
  assert.equal(
    notice({ type: 'session', session: waiting, previous: waiting }),
    null,
  );
});

test('a waiting input state raises a notice', () => {
  const result = notice({
    type: 'session',
    session: session({ waitingFor: 'input' }),
    previous: session({ workState: 'working' }),
  });
  assert.equal(result.waitingFor, 'input');
});

test('a closed session raises no notice', () => {
  assert.equal(
    notice({
      type: 'session',
      session: session({ status: 'closed', waitingFor: 'permission' }),
      previous: session(),
    }),
    null,
  );
});

test('a pending question raises a question notice', () => {
  const result = notice({
    type: 'question',
    question: {
      id: 'q1',
      sessionId: 's1',
      sessionName: 'Build',
      text: 'Which branch?',
      options: [],
      status: 'pending',
      answer: null,
      createdAt: '2026-11-20T00:00:00Z',
    },
  });
  assert.deepEqual(result, {
    kind: 'waiting',
    sessionId: 's1',
    sessionName: 'Build',
    waitingFor: 'question',
  });
});

test('an answered question raises no notice', () => {
  assert.equal(
    notice({
      type: 'question',
      question: {
        id: 'q1',
        sessionId: 's1',
        sessionName: 'Build',
        text: 'Which branch?',
        options: [],
        status: 'answered',
        answer: 'main',
        createdAt: '2026-11-20T00:00:00Z',
      },
    }),
    null,
  );
});

test('the session update before a question leaves the notice to the question', () => {
  assert.equal(
    notice({
      type: 'session',
      session: session({ waitingFor: 'question' }),
      previous: session({ workState: 'working' }),
    }),
    null,
  );
});

test('a pending session request raises an approval notice', () => {
  const result = notice({
    type: 'request',
    request: {
      id: 'r1',
      parentSessionId: 's2',
      parentSessionName: 'Planner',
      projectId: 'web',
      projectName: 'Web',
      name: 'Feature',
      folder: '/tmp/web',
      sourceBranch: null,
      firstPrompt: null,
      configId: 'a',
      providerId: 'p',
      client: 'Claude Code',
      status: 'pending',
      childSessionId: null,
      createdAt: '2026-11-20T00:00:00Z',
    },
  });
  assert.deepEqual(result, {
    kind: 'waiting',
    sessionId: 's2',
    sessionName: 'Planner',
    waitingFor: 'start',
  });
});

test('a finished session request raises no notice', () => {
  assert.equal(
    notice({
      type: 'request',
      request: {
        id: 'r1',
        parentSessionId: 's2',
        parentSessionName: 'Planner',
        status: 'approved',
      },
    }),
    null,
  );
});

test('a done report raises a done notice', () => {
  const result = notice({
    type: 'notification',
    notification: {
      id: 'n1',
      sessionId: 's1',
      sessionName: 'Build',
      message: 'Reported done.',
      kind: 'done',
      createdAt: '2026-11-20T00:00:00Z',
    },
  });
  assert.deepEqual(result, {
    kind: 'done',
    sessionId: 's1',
    sessionName: 'Build',
  });
});

test('a Notify me message carries the agent text', () => {
  const result = notice({
    type: 'notification',
    notification: {
      id: 'n2',
      sessionId: 's1',
      sessionName: 'Build',
      message: 'The tests pass.',
      kind: 'message',
      createdAt: '2026-11-20T00:00:00Z',
    },
  });
  assert.equal(result.kind, 'message');
  assert.equal(result.message, 'The tests pass.');
});

test('a done notice stays off while its session is open and in front', () => {
  const event = {
    type: 'notification',
    notification: {
      id: 'n1',
      sessionId: 's1',
      sessionName: 'Build',
      message: 'Reported done.',
      kind: 'done',
      createdAt: '2026-11-20T00:00:00Z',
    },
  };
  assert.equal(
    notice(event, { sessionId: 's1', appVisible: true }),
    null,
  );
  assert.equal(
    notice(event, { sessionId: 's1', appVisible: false }).kind,
    'done',
  );
});

test('a waiting notice appears when the same session is behind the app', () => {
  const event = {
    type: 'request',
    request: {
      id: 'r1',
      parentSessionId: 's1',
      parentSessionName: 'Build',
      status: 'pending',
    },
  };
  assert.equal(
    notice(event, { sessionId: 's1', appVisible: false }).sessionId,
    's1',
  );
});

test('a waiting notice appears when another session is on the screen', () => {
  const event = {
    type: 'session',
    session: session({ waitingFor: 'permission' }),
    previous: session({ workState: 'working' }),
  };
  assert.equal(
    notice(event, { sessionId: 's9', appVisible: true }).sessionId,
    's1',
  );
});

test('a message notice appears even while its session is on the screen', () => {
  const event = {
    type: 'notification',
    notification: {
      id: 'n2',
      sessionId: 's1',
      sessionName: 'Build',
      message: 'The tests pass.',
      kind: 'message',
      createdAt: '2026-11-20T00:00:00Z',
    },
  };
  assert.equal(
    notice(event, { sessionId: 's1', appVisible: true }).kind,
    'message',
  );
});

test('the visibility rule covers both kinds that a card can repeat', () => {
  assert.equal(
    notifications.shouldShowNotification(
      {
        kind: 'waiting',
        sessionId: 's1',
        sessionName: 'Build',
        waitingFor: 'permission',
      },
      { sessionId: 's1', appVisible: true },
    ),
    false,
  );
  assert.equal(
    notifications.shouldShowNotification(
      {
        kind: 'done',
        sessionId: 's1',
        sessionName: 'Build',
      },
      { sessionId: 's1', appVisible: true },
    ),
    false,
  );
  assert.equal(
    notifications.shouldShowNotification(
      {
        kind: 'waiting',
        sessionId: 's1',
        sessionName: 'Build',
        waitingFor: 'permission',
      },
      HIDDEN,
    ),
    true,
  );
});
