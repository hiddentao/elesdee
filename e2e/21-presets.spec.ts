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

test('creates a preset in settings and starts a session from it', async ({
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
  const presetName = uniqueName('preset');

  await openApp(page);

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Presets' }).click();
  await settings.getByRole('button', { name: 'Add preset' }).click();

  const editor = page.getByRole('dialog', { name: 'Add preset' });
  await editor.getByLabel('Preset name').fill(presetName);
  const accountCombo = editor.getByRole('combobox', { name: 'Account' });
  await accountCombo.click();
  await accountCombo.fill(account.name);
  await page.getByRole('option', { name: new RegExp(account.name) }).click();
  await editor.getByLabel('Model (optional)').fill('claude-sonnet-4-5');

  await editor.getByRole('button', { name: 'Add variable' }).click();
  await editor.getByLabel(/Variable name/).fill('E2E_VALUE');
  await editor.getByLabel(/Variable value/).fill('from-preset');

  await editor.getByRole('button', { name: 'Add setting' }).click();
  await editor.getByLabel(/Config key/).fill('permissions.defaultMode');
  await editor.getByLabel(/Config value/).fill('acceptEdits');

  await editor.getByRole('button', { name: 'Save preset' }).click();
  await expect(editor).not.toBeVisible();
  await expect(settings.getByText(presetName)).toBeVisible();

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
  await dialog.getByRole('radio', { name: 'Claude Code' }).click();

  const presetCombo = dialog.getByRole('combobox', { name: 'Preset' });
  await presetCombo.click();
  await presetCombo.fill(presetName);
  await page.getByRole('option', { name: new RegExp(presetName) }).click();

  await expect(presetCombo).toHaveValue(new RegExp('42% left'));
  await expect(dialog.getByText('Model claude-sonnet-4-5')).toBeVisible();
  await expect(
    dialog.getByText('1 environment variable · 1 config override'),
  ).toBeVisible();

  const name = uniqueName('preset-session');
  await dialog.getByLabel('Session name (optional)').fill(name);
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-e2e-value:from-preset',
    { timeout: 15_000 },
  );

  const data = await bootstrap(request);
  const session = data.sessions.find(
    (item) => item.projectId === project.id && item.name === name,
  );
  expect(session).toBeTruthy();
  expect(session?.accountId).toBe(account.id);

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
  expect(saved.model, 'the preset model reached the launch settings').toBe(
    'claude-sonnet-4-5',
  );
  expect(saved.permissions.defaultMode).toBe('acceptEdits');
  expect(saved.hooks.PreToolUse).toBeTruthy();

  await closeSession(request, session!.id);
});
