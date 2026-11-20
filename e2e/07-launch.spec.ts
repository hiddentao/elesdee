import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Locator } from '@playwright/test';
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

  const configCombo = dialog.getByRole('combobox', { name: 'AI config', exact: true });
  await configCombo.click();
  await configCombo.fill('Anthropic Max');
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
  const promptField = dialog.getByLabel('First prompt (optional)');
  await expect(promptField).toBeHidden();
  await dialog.getByText('Additional settings').click();
  await expect(promptField).toBeVisible();
  await promptField.fill(prompt);
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

test('picks a recent repository from an earlier session', async ({
  page,
  request,
}) => {
  const label = uniqueName('recent-project');
  const sourceLabel = uniqueName('recent-source');
  const baseRepo = createGitRepo(`${label}-base`);
  const sourceRepo = createGitRepo(sourceLabel);
  await createProject(request, label, baseRepo);
  const sourceProject = await createProject(request, sourceLabel, sourceRepo);

  async function chooseProjectAndConfig(dialog: Locator, projectName: string) {
    const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
    await projectCombo.click();
    await projectCombo.fill(projectName);
    await page
      .getByRole('option', { name: new RegExp(projectName) })
      .click();

    const configCombo = dialog.getByRole('combobox', {
      name: 'AI config',
      exact: true,
    });
    await configCombo.click();
    await configCombo.fill('Anthropic Max');
    await page.getByRole('option', { name: /Anthropic Max/ }).click();
  }

  await openApp(page);
  await page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' })
    .click();
  let dialog = page.getByRole('dialog');
  await chooseProjectAndConfig(dialog, sourceLabel);
  await expect(
    dialog.getByRole('combobox', { name: 'Repository folder' }),
  ).toHaveValue(sourceRepo);
  await dialog.getByRole('button', { name: 'Start session' }).click();
  await expect(dialog).not.toBeVisible();

  let data = await bootstrap(request);
  const earlier = data.sessions.find(
    (item) => item.projectId === sourceProject.id,
  );
  expect(earlier).toBeTruthy();
  await closeSession(request, earlier!.id);

  await page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' })
    .click();
  dialog = page.getByRole('dialog');
  await chooseProjectAndConfig(dialog, label);

  const repository = dialog.getByRole('combobox', {
    name: 'Repository folder',
  });
  await expect(repository).toHaveValue(baseRepo);
  await repository.click();
  const recent = page.getByRole('option', {
    name: new RegExp(sourceRepo.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
  });
  await expect(recent).toBeVisible();
  await recent.click();

  await expect(repository).toHaveValue(sourceRepo);
  await expect(
    dialog.getByRole('combobox', { name: 'Project' }),
  ).toHaveValue(label);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
});

test.describe('at 1024 pixels', () => {
  test.use({ viewport: { width: 1024, height: 800 } });

  test('the launch form fits without horizontal scroll', async ({ page }) => {
    await openApp(page);
    await page
      .locator('.titlebar')
      .getByRole('button', { name: 'New session' })
      .click();
    const dialog = page.getByRole('dialog');

    await dialog.getByText('Additional settings').click();
    await expect(dialog.getByLabel('Repository folder')).toBeVisible();
    await expect(dialog.getByLabel('First prompt (optional)')).toBeVisible();

    const pageOverflow = await page.evaluate(
      () =>
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    );
    expect(pageOverflow).toBeLessThanOrEqual(0);
    const dialogOverflow = await dialog.evaluate(
      (element) => element.scrollWidth - element.clientWidth,
    );
    expect(dialogOverflow).toBeLessThanOrEqual(0);
  });
});
