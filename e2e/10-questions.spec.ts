import { expect, test, type APIRequestContext } from '@playwright/test';
import {
  CORE_URL,
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

function mcpCall(
  request: APIRequestContext,
  sessionId: string,
  name: string,
  argumentsValue: Record<string, unknown>,
) {
  return request.post(`${CORE_URL}/internal/mcp/${sessionId}`, {
    data: {
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name, arguments: argumentsValue },
    },
  });
}

test('asks, lists, answers and notifies through the session tools', async ({
  page,
  request,
}) => {
  const label = uniqueName('question-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Question session'),
    folder: repo,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  const asked = await mcpCall(request, session.id, 'ask_me', {
    text: 'Deploy now?',
    options: ['Yes', 'No'],
  });
  expect(asked.ok()).toBeTruthy();
  const askedBody = (await asked.json()) as {
    result: { content: { text: string }[] };
  };
  const questionId = askedBody.result.content[0].text.replace(
    'Question asked: ',
    '',
  );

  const questionsChip = page.getByRole('button', { name: /Questions/ });
  await expect(questionsChip).toContainText('1');
  await expect(page.locator('.session-question')).toContainText('Deploy now?');

  const waiting = request.post(`${CORE_URL}/internal/mcp/${session.id}`, {
    data: {
      jsonrpc: '2.0',
      id: 2,
      method: 'tools/call',
      params: {
        name: 'wait_for_answer',
        arguments: { questionId, timeoutSeconds: 10 },
      },
    },
    timeout: 30_000,
  });

  await page
    .locator('.session-question')
    .getByRole('button', { name: 'Yes' })
    .click();
  await expect(page.locator('.session-question')).not.toBeVisible();
  await expect(questionsChip).not.toBeVisible();

  const waited = await waiting;
  expect(waited.ok()).toBeTruthy();
  const waitedBody = (await waited.json()) as {
    result: { content: { text: string }[] };
  };
  expect(waitedBody.result.content[0].text).toBe('Answer: Yes');

  const notified = await mcpCall(request, session.id, 'notify_me', {
    message: 'The build finished.',
  });
  expect(notified.ok()).toBeTruthy();
  await expect(page.getByText(/The build finished\./)).toBeVisible();

  await mcpCall(request, session.id, 'ask_me', {
    text: 'Ship the release?',
    options: ['Ship it', 'Later'],
  });
  await expect(questionsChip).toContainText('1');
  await questionsChip.click();
  const dialog = page.getByRole('dialog', { name: 'Questions' });
  await expect(dialog).toContainText('Ship the release?');
  await dialog.getByRole('button', { name: 'Later' }).click();
  await expect(dialog).toContainText('No pending questions.');
  await dialog.getByRole('button', { name: 'Close' }).click();

  await closeSession(request, session.id);
});
