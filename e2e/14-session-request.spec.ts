import { expect, test, type APIRequestContext } from '@playwright/test';
import {
  CORE_URL,
  bootstrap,
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

function startChild(
  request: APIRequestContext,
  parentId: string,
  projectId: string,
  name: string,
) {
  return request.post(`${CORE_URL}/internal/mcp/${parentId}`, {
    data: {
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: {
        name: 'start_session',
        arguments: {
          projectId,
          name,
          firstPrompt: 'Do the work',
        },
      },
    },
  });
}

test('asks before starting a child session and remembers permission', async ({
  page,
  request,
}) => {
  const label = uniqueName('request-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const parent = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Coordinator'),
    folder: repo,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(parent.name) }).click();

  const asked = await startChild(request, parent.id, project.id, 'Child session');
  expect(asked.ok()).toBeTruthy();
  const askedBody = (await asked.json()) as {
    result: { content: { text: string }[] };
  };
  expect(askedBody.result.content[0].text).toContain('Session request:');

  const requestsChip = page.getByRole('button', { name: /Requests/ });
  await expect(requestsChip).toContainText('1');
  await expect(page.locator('.request-card')).toContainText(
    'wants to start a session',
  );

  await page
    .locator('.request-card')
    .getByLabel('Allow further sessions from this session')
    .check();
  await page
    .locator('.request-card')
    .getByRole('button', { name: 'Approve' })
    .click();

  await expect(
    page.getByRole('tab', { name: /Child session/ }),
  ).toBeVisible({ timeout: 20_000 });
  let data = await bootstrap(request);
  const firstChild = data.sessions.find(
    (session) => session.name === 'Child session',
  );
  expect(firstChild).toBeTruthy();

  const second = await startChild(request, parent.id, project.id, 'Second child');
  const secondBody = (await second.json()) as {
    result: { content: { text: string }[] };
  };
  expect(secondBody.result.content[0].text).toContain('Session started:');
  await expect
    .poll(async () => {
      data = await bootstrap(request);
      return data.sessions.some((session) => session.name === 'Second child');
    })
    .toBe(true);

  for (const session of data.sessions.filter(
    (item) => item.projectId === project.id,
  )) {
    await closeSession(request, session.id);
  }
});
