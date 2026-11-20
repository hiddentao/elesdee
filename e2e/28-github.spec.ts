import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { getRunDir } from './env';
import { CORE_URL, openApp } from './helpers';

function setGitHubResult(result: 'success' | 'denied'): void {
  fs.writeFileSync(path.join(getRunDir(), 'github-mode.txt'), result);
}

test('a refused GitHub sign-in shows the reason', async ({ page }) => {
  setGitHubResult('denied');

  await openApp(page);
  await page
    .getByRole('button', { name: 'Add project' })
    .first()
    .click();
  const dialog = page.getByRole('dialog', { name: 'Add project' });

  await dialog.getByRole('radio', { name: 'From GitHub' }).click();
  await dialog.getByRole('button', { name: 'Connect GitHub' }).click();

  await expect(
    dialog.getByText('The GitHub sign-in was not approved. Try again.'),
  ).toBeVisible({ timeout: 20_000 });
});

test('connects GitHub, picks a repository and clones it', async ({
  page,
  request,
}) => {
  setGitHubResult('success');

  await openApp(page);
  await page
    .getByRole('button', { name: 'Add project' })
    .first()
    .click();
  const dialog = page.getByRole('dialog', { name: 'Add project' });

  await dialog.getByRole('radio', { name: 'From GitHub' }).click();
  await dialog.getByRole('button', { name: 'Connect GitHub' }).click();
  await expect(dialog.getByText('Signed in as @octocat')).toBeVisible({
    timeout: 20_000,
  });

  await dialog.getByLabel('Repository').click();
  const option = page.getByRole('option', { name: /acme\/demo/ });
  await expect(option).toBeVisible();
  await expect(page.getByText(/demo\.git/)).toBeVisible();
  await option.click();

  await expect(dialog.getByLabel('Name')).toHaveValue('demo');
  await expect(
    dialog.getByText(
      `The repository is cloned into ${path.join(getRunDir(), 'github-clones', 'demo')}.`,
    ),
  ).toBeVisible();

  await dialog.getByRole('button', { name: 'Clone and add project' }).click();
  await expect(dialog).not.toBeVisible({ timeout: 30_000 });

  const response = await request.get(`${CORE_URL}/api/projects`);
  expect(response.ok()).toBeTruthy();
  const projects = (await response.json()) as Array<{
    name: string;
    folder: string;
  }>;
  const project = projects.find((item) => item.name === 'demo');
  expect(project, JSON.stringify(projects)).toBeTruthy();
  expect(fs.existsSync(path.join(project?.folder ?? '', 'README.md'))).toBe(
    true,
  );
});
