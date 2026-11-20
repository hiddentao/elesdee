import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('answers a Codex permission from the app', async ({ page, request }) => {
  await request.post('http://127.0.0.1:4397/reset');

  const label = uniqueName('codex-permission-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Codex permission'),
    folder: repo,
    accountId: 'codex',
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  await expect(
    page.getByRole('heading', { name: 'Permission needed' }),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page.locator('.permission-tool')).toContainText('Bash');
  await expect(page.locator('.permission-summary')).toContainText(
    'rm -rf release',
  );

  await page.getByRole('button', { name: 'Allow' }).click();
  await expect(
    page.getByRole('heading', { name: 'Permission needed' }),
  ).not.toBeVisible();

  await expect
    .poll(
      async () => {
        const response = await request.get('http://127.0.0.1:4397/state');
        return ((await response.json()) as { replied: string | null }).replied;
      },
      { timeout: 10_000 },
    )
    .toBe('accept');

  await closeSession(request, session.id);
});
