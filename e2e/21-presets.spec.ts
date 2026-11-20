import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  bootstrap,
  closeSession,
  createAccount,
  createGitRepo,
  createProject,
  openApp,
  setUsage,
  uniqueName,
} from './helpers';
import { dataDir } from './env';

test('creates an AI config in settings and starts a session from it', async ({
  page,
  request,
}) => {
  const label = uniqueName('preset-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const account = await createAccount(request, {
    name: `${label} account`,
    key: 'sk-e2e-preset',
  });
  await setUsage(request, account.id, [{ label: '5-hour', used: 58 }]);
  const configName = uniqueName('config');

  await openApp(page);

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'AI configs' }).click();
  await settings.getByRole('button', { name: 'Add AI config' }).click();

  const editor = page.getByRole('dialog', { name: 'Add AI config' });
  const providerCombo = editor.getByRole('combobox', {
    name: 'Provider',
    exact: true,
  });
  await providerCombo.click();
  await providerCombo.fill(account.name);
  await page.getByRole('option', { name: new RegExp(account.name) }).click();
  await editor.getByRole('radio', { name: 'Claude Code' }).click();
  await editor.getByLabel('Name (optional)').fill(configName);
  const modelCombo = editor.getByRole('combobox', { name: 'Model' });
  await modelCombo.click();
  await modelCombo.fill('claude-sonnet-4-5');
  await modelCombo.press('Enter');

  await editor.getByText('Additional settings').click();
  await editor.getByLabel('Environment').fill('E2E_VALUE=from-config');
  await editor
    .getByLabel('Agent config')
    .fill('permissions.defaultMode=acceptEdits');

  await editor.getByRole('button', { name: 'Add AI config' }).click();
  await expect(editor).not.toBeVisible();
  await expect(settings.getByText(configName)).toBeVisible();

  await settings.getByRole('button', { name: 'Close settings' }).click();

  await page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' })
    .click();
  const dialog = page.getByRole('dialog', { name: 'New session' });
  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();

  const configCombo = dialog.getByRole('combobox', {
    name: 'AI config',
    exact: true,
  });
  await configCombo.click();
  await configCombo.fill(configName);
  await page.getByRole('option', { name: new RegExp(configName) }).click();

  await expect(configCombo).toHaveValue(new RegExp('42% left'));
  await expect(dialog.getByText('Model claude-sonnet-4-5')).toBeVisible();
  await expect(
    dialog.getByText('1 environment variable · 1 config override'),
  ).toBeVisible();

  const name = uniqueName('config-session');
  await dialog.getByLabel('Session name (optional)').fill(name);
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-e2e-value:from-config',
    { timeout: 15_000 },
  );

  const data = await bootstrap(request);
  const session = data.sessions.find(
    (item) => item.projectId === project.id && item.name === name,
  );
  expect(session).toBeTruthy();
  const savedConfig = (await bootstrap(request)).configs?.find(
    (item) => item.name === configName,
  );
  expect(session?.configId).toBe(savedConfig?.id);

  const settingsPath = path.join(
    dataDir(),
    'claude-settings',
    `${session!.id}.json`,
  );
  await expect.poll(() => fs.existsSync(settingsPath)).toBe(true);
  const saved = JSON.parse(fs.readFileSync(settingsPath, 'utf8')) as {
    model: string;
    permissions: { defaultMode: string };
    hooks: { PreToolUse: unknown };
  };
  expect(saved.model, 'the config model reached the launch settings').toBe(
    'claude-sonnet-4-5',
  );
  expect(saved.permissions.defaultMode).toBe('acceptEdits');
  expect(saved.hooks.PreToolUse).toBeTruthy();

  await closeSession(request, session!.id);
});
