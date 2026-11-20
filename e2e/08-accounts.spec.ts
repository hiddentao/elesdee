import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { dataDir } from './env';
import {
  closeSession,
  createAgentSession,
  createConfig,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
  CORE_URL,
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
  await settings.getByRole('button', { name: 'Add provider' }).click();
  const add = page.getByRole('dialog', { name: 'Add provider' });

  await add.getByLabel('Vendor').selectOption('deepseek');
  await add.getByLabel('Provider name (optional)').fill('DeepSeek e2e');
  await add.locator('#provider-key').fill('sk-e2e-deepseek');
  await add.getByRole('button', { name: 'Add provider' }).click();

  await expect(add).not.toBeVisible();
  await expect(settings.getByText('DeepSeek e2e')).toBeVisible();
  await expect(
    settings.getByText('Claude Code · Codex · OpenCode 2 · pay as you go').first(),
  ).toBeVisible();

  await settings.getByRole('button', { name: 'AI configs' }).click();
  await settings.getByRole('button', { name: 'Add AI config' }).click();
  const editor = page.getByRole('dialog', { name: 'Add AI config' });
  await editor
    .getByRole('combobox', { name: 'Provider', exact: true })
    .click();
  await editor
    .getByRole('combobox', { name: 'Provider', exact: true })
    .fill('DeepSeek e2e');
  await page.getByRole('option', { name: /DeepSeek e2e/ }).click();
  await editor.getByRole('radio', { name: 'Claude Code' }).click();
  await editor.getByRole('button', { name: 'Add AI config' }).click();

  await settings.getByRole('button', { name: 'Close settings' }).click();

  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');
  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();
  const configCombo = dialog.getByRole('combobox', { name: 'AI config', exact: true });
  await configCombo.click();
  await configCombo.fill('DeepSeek e2e');
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

  const created = await request.post(`${CORE_URL}/api/providers`, {
    data: {
      name: 'Custom e2e',
      vendor: 'custom',
      auth: 'key',
      apiStyle: 'openai',
      key: 'sk-e2e-custom',
      server: 'https://gateway.example/v1',
    },
  });
  expect(created.ok()).toBeTruthy();
  const provider = (await created.json()) as { id: string };
  const config = await createConfig(request, {
    providerId: provider.id,
    client: 'Codex',
  });

  const session = await createAgentSession(request, {
    projectId: project.id,
    name: 'Custom provider session',
    folder: repo,
    configId: config.id,
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
