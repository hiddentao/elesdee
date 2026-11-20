import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  bootstrap,
  closeSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('launches from a chosen branch and names the session from the prompt', async ({
  page,
  request,
}) => {
  const label = uniqueName('launch-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);

  await openApp(page);
  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');

  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();

  await dialog.getByRole('radio', { name: 'Claude Code' }).click();
  const accountCombo = dialog.getByRole('combobox', { name: 'Account' });
  await accountCombo.click();
  await accountCombo.fill('Anthropic Max');
  await page.getByRole('option', { name: /Anthropic Max/ }).click();

  await expect(dialog.getByLabel('Repository folder')).toHaveValue(repo);

  const branchCombo = dialog.getByRole('combobox', {
    name: 'Source branch (optional)',
  });
  await branchCombo.click();
  await branchCombo.fill('feature');
  await expect(
    page.getByRole('option', { name: /^feature/ }),
  ).toBeVisible();
  await page.getByRole('option', { name: /^feature/ }).click();
  await expect(branchCombo).toHaveValue('feature');

  const prompt = 'Fix the sky courier';
  await dialog.getByLabel('First prompt (optional)').fill(prompt);
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole('tab', { name: new RegExp(prompt) }),
  ).toBeVisible();
  await expect(page.getByRole('heading', { name: prompt })).toBeVisible();

  await expect(page.locator('.xterm-accessibility')).toContainText(
    `arg:${prompt}`,
    { timeout: 15_000 },
  );

  const data = await bootstrap(request);
  const session = data.sessions.find(
    (item) => item.projectId === project.id && item.name === prompt,
  );
  expect(session).toBeTruthy();
  expect(session?.branch).toMatch(/^elesdee\//);
  expect(session?.branch).toContain('fix-the-sky-courier');
  expect(fs.existsSync(path.join(session!.folder, 'feature.txt'))).toBe(true);
  expect(session?.firstPrompt).toBe(prompt);

  await closeSession(request, session!.id);
});
