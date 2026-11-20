import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { dataDir } from './env';
import {
  CORE_URL,
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('adds an API-key provider and gives the session its environment', async ({
  page,
  request,
}) => {
  const label = uniqueName('account-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);

  await openApp(page);
  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Add account' }).click();
  const add = page.getByRole('dialog', { name: 'Add account' });

  await add.getByLabel('Provider').selectOption('deepseek');
  await add.getByLabel('Account name').fill('DeepSeek e2e');
  await add.locator('#account-key').fill('sk-e2e-deepseek');
  await add.getByRole('button', { name: 'Add account' }).click();

  await expect(add).not.toBeVisible();
  await expect(settings.getByText('DeepSeek e2e')).toBeVisible();
  await expect(
    settings.getByText('Claude Code · pay as you go').first(),
  ).toBeVisible();

  await settings.getByRole('button', { name: 'Close settings' }).click();

  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');
  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();
  await dialog.getByRole('radio', { name: 'Claude Code' }).click();
  const accountCombo = dialog.getByRole('combobox', { name: 'Preset' });
  await accountCombo.click();
  await accountCombo.fill('DeepSeek e2e');
  await page.getByRole('option', { name: /DeepSeek e2e/ }).click();
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-anthropic-base:https://api.deepseek.com/anthropic',
    { timeout: 15_000 },
  );
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-anthropic-token:[redacted]',
  );

  const state = fs.readFileSync(path.join(dataDir(), 'state.json'), 'utf8');
  expect(state).not.toContain('sk-e2e-deepseek');

  const response = await request.get(`${CORE_URL}/api/bootstrap`);
  expect(response.ok()).toBeTruthy();
  const data = (await response.json()) as {
    sessions: { id: string; projectId: string }[];
  };
  const session = data.sessions.find((item) => item.projectId === project.id);
  if (session) {
    await closeSession(request, session.id);
  }
});

test('runs a custom OpenAI-compatible provider on Codex', async ({
  page,
  request,
}) => {
  const label = uniqueName('custom-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);

  const created = await request.post(`${CORE_URL}/api/accounts`, {
    data: {
      name: 'Custom e2e',
      providerId: 'custom',
      client: 'Codex',
      auth: 'key',
      apiStyle: 'openai',
      key: 'sk-e2e-custom',
      server: 'https://gateway.example/v1',
    },
  });
  expect(created.ok()).toBeTruthy();
  const account = (await created.json()) as { id: string };

  const session = await createAgentSession(request, {
    projectId: project.id,
    name: 'Custom provider session',
    folder: repo,
    accountId: account.id,
  });

  await openApp(page);
  await page
    .getByRole('button', { name: /Custom provider session/ })
    .click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-openai-base:https://gateway.example/v1',
    { timeout: 15_000 },
  );
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-openai-key:[redacted]',
  );

  await closeSession(request, session.id);
});
