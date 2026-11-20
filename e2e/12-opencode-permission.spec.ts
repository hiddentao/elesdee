import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

const OPENCODE_URL =
  process.env.ELESDEE_E2E_OPENCODE_URL ?? 'http://127.0.0.1:4398';

test('answers an OpenCode permission from the app', async ({ page, request }) => {
  await request.post(`${OPENCODE_URL}/reset`);

  const label = uniqueName('opencode-permission-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const account = await createAccount(request, {
    name: uniqueName('OpenCode permission account'),
    providerId: 'openrouter',
    key: 'sk-openrouter-permission',
  });
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('OpenCode permission'),
    folder: repo,
    accountId: account.id,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  await expect(
    page.getByRole('heading', { name: 'Permission needed' }),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page.locator('.permission-tool')).toContainText('bash');
  await expect(page.locator('.permission-summary')).toContainText(
    'rm -rf build',
  );

  await page.getByRole('button', { name: 'Allow' }).click();
  await expect(page.getByRole('heading', { name: 'Permission needed' })).not.toBeVisible();

  await expect
    .poll(
      async () => {
        const response = await request.get(`${OPENCODE_URL}/state`);
        return ((await response.json()) as { replied: string | null }).replied;
      },
      { timeout: 10_000 },
    )
    .toBe('once');

  await closeSession(request, session.id);
});
