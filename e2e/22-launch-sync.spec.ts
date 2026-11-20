import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  bootstrap,
  cloneRepo,
  closeSession,
  createBareRepo,
  createGitRepo,
  createProject,
  gitIn,
  openApp,
  uniqueName,
} from './helpers';
import { projectsDir } from './env';

async function fillLaunchForm(
  page: Page,
  label: string,
  repo: string,
  branch?: string,
) {
  await page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' })
    .click();
  const dialog = page.getByRole('dialog');

  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();

  const configCombo = dialog.getByRole('combobox', {
    name: 'AI config',
    exact: true,
  });
  await configCombo.click();
  await configCombo.fill('Anthropic Max');
  await page.getByRole('option', { name: /Anthropic Max/ }).click();
  await expect(dialog.getByLabel('Repository folder')).toHaveValue(repo);

  if (branch) {
    const branchCombo = dialog.getByRole('combobox', {
      name: 'Source branch (optional)',
    });
    await branchCombo.click();
    await branchCombo.fill(branch);
    await page
      .getByRole('option', { name: new RegExp(`^${branch}`) })
      .first()
      .click();
    await expect(branchCombo).toHaveValue(branch);
  }

  return dialog;
}

test('a launch syncs a tracking branch from the remote and leaves the checkout alone', async ({
  page,
  request,
}) => {
  const label = uniqueName('sync');
  const remote = createBareRepo(`${label}-remote`);
  const repo = createGitRepo(label);
  gitIn(repo, ['remote', 'add', 'origin', remote]);
  gitIn(repo, ['push', '--quiet', '--set-upstream', 'origin', 'main']);
  const before = gitIn(repo, ['rev-parse', 'main']);

  const clone = cloneRepo(remote, path.join(projectsDir(), `${label}-clone`));
  fs.writeFileSync(path.join(clone, 'remote.txt'), 'from remote\n');
  gitIn(clone, ['add', '.']);
  gitIn(clone, [
    '-c',
    'user.name=E2E',
    '-c',
    'user.email=e2e@example.com',
    'commit',
    '--quiet',
    '-m',
    'remote update',
  ]);
  gitIn(clone, ['push', '--quiet', 'origin', 'main']);

  const project = await createProject(request, label, repo);
  await openApp(page);
  const dialog = await fillLaunchForm(page, label, repo, 'main');
  await dialog.getByRole('button', { name: 'Start session' }).click();
  await expect(dialog).not.toBeVisible();

  const data = await bootstrap(request);
  const session = data.sessions.find((item) => item.projectId === project.id);
  expect(session).toBeTruthy();
  expect(
    fs.readFileSync(path.join(session!.folder, 'remote.txt'), 'utf8'),
  ).toBe('from remote\n');
  expect(gitIn(repo, ['rev-parse', 'main'])).toBe(before);
  expect(gitIn(repo, ['status', '--porcelain'])).toBe('');

  await closeSession(request, session!.id);
});

test('the launch form asks what to do with uncommitted changes', async ({
  page,
  request,
}) => {
  const label = uniqueName('copy');
  const repo = createGitRepo(label);
  fs.writeFileSync(path.join(repo, 'README.md'), 'dirty change\n');
  fs.writeFileSync(path.join(repo, 'notes.txt'), 'untracked notes\n');
  const project = await createProject(request, label, repo);
  await openApp(page);

  let dialog = await fillLaunchForm(page, label, repo);
  await dialog.getByText('Additional settings').click();
  const settings = dialog.locator('details.additional-settings');
  await expect(settings.getByLabel('First prompt (optional)')).toBeVisible();
  const choice = dialog.getByRole('group', { name: 'Uncommitted changes' });
  await expect(choice).toBeVisible();
  await choice
    .getByRole('radio', { name: 'Copy them into the new session' })
    .check();
  await dialog.getByRole('button', { name: 'Start session' }).click();
  await expect(dialog).not.toBeVisible();

  let data = await bootstrap(request);
  const copied = data.sessions.find((item) => item.projectId === project.id);
  expect(copied).toBeTruthy();
  expect(
    fs.readFileSync(path.join(copied!.folder, 'README.md'), 'utf8'),
  ).toBe('dirty change\n');
  expect(fs.readFileSync(path.join(copied!.folder, 'notes.txt'), 'utf8')).toBe(
    'untracked notes\n',
  );

  dialog = await fillLaunchForm(page, label, repo);
  await dialog.getByText('Additional settings').click();
  await dialog
    .getByRole('group', { name: 'Uncommitted changes' })
    .getByRole('radio', { name: 'Leave them out' })
    .check();
  await dialog.getByRole('button', { name: 'Start session' }).click();
  await expect(dialog).not.toBeVisible();

  data = await bootstrap(request);
  const left = data.sessions.find(
    (item) => item.projectId === project.id && item.id !== copied!.id,
  );
  expect(left).toBeTruthy();
  expect(fs.readFileSync(path.join(left!.folder, 'README.md'), 'utf8')).toBe(
    'elesdee e2e\n',
  );
  expect(fs.existsSync(path.join(left!.folder, 'notes.txt'))).toBe(false);
  expect(fs.readFileSync(path.join(repo, 'README.md'), 'utf8')).toBe(
    'dirty change\n',
  );
  expect(fs.existsSync(path.join(repo, 'notes.txt'))).toBe(true);

  await closeSession(request, copied!.id);
  await closeSession(request, left!.id);
});
