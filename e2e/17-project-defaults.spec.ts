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
  uniqueName,
} from './helpers';

test('project defaults prefill the launch form and start the session', async ({
  page,
  request,
}) => {
  const label = uniqueName('defaults-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const account = await createAccount(request, {
    name: `${label} account`,
    key: 'sk-e2e-defaults',
  });

  await openApp(page);

  await page
    .getByRole('button', { name: `Project settings for ${label}` })
    .click();
  const settings = page.getByRole('dialog', { name: 'Project settings' });

  const accountCombo = settings.getByRole('combobox', {
    name: 'Default account',
  });
  await accountCombo.click();
  await accountCombo.fill(account.name);
  await page
    .getByRole('option', { name: new RegExp(account.name) })
    .click();

  const branchCombo = settings.getByRole('combobox', {
    name: 'Default source branch',
  });
  await branchCombo.click();
  await branchCombo.fill('feature');
  await page.getByRole('option', { name: /^feature/ }).click();

  await settings.getByRole('button', { name: 'Save defaults' }).click();
  await expect(settings).not.toBeVisible();

  const stored = (await bootstrap(request)).projects.find(
    (item) => item.id === project.id,
  );
  expect(stored?.defaultAccountId).toBe(account.id);
  expect(stored?.defaultSourceBranch).toBe('feature');

  await page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' })
    .click();
  const dialog = page.getByRole('dialog');

  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();

  await expect(
    dialog.getByRole('radio', { name: 'Claude Code' }),
  ).toBeChecked();
  await expect(
    dialog.getByRole('combobox', { name: 'Preset' }),
  ).toHaveValue(new RegExp(account.name));
  await expect(
    dialog.getByRole('combobox', { name: 'Source branch (optional)' }),
  ).toHaveValue('feature');
  await expect(dialog.getByLabel('Repository folder')).toHaveValue(repo);

  const name = uniqueName('defaults-session');
  await dialog.getByLabel('Session name (optional)').fill(name);
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.getByRole('tab', { name: new RegExp(name) })).toBeVisible();

  const data = await bootstrap(request);
  const session = data.sessions.find(
    (item) => item.projectId === project.id && item.name === name,
  );
  expect(session).toBeTruthy();
  expect(session?.accountId).toBe(account.id);
  expect(
    fs.existsSync(path.join(session!.folder, 'feature.txt')),
    'the session started from the default source branch',
  ).toBe(true);

  await closeSession(request, session!.id);
});
