import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  openApp,
  sessionRow,
  uniqueName,
} from './helpers';

test('edits and saves a file from the session panel and shows it in Changes', async ({
  page,
  request,
}) => {
  const label = uniqueName('files-panel');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('Files panel');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session panel' }).click();

    await expect(page.getByRole('tab', { name: 'Files' })).toBeVisible();
    const readme = page.getByRole('button', { name: /README\.md/ });
    await expect(readme).toBeVisible();
    await readme.click();

    const editor = page.locator('.cm-content');
    await expect(editor).toBeVisible();
    await editor.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('changed through the panel\n');

    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Saved.')).toBeVisible();

    expect(fs.readFileSync(path.join(repo, 'README.md'), 'utf8')).toBe(
      'changed through the panel\n',
    );

    await expect(page.locator('.terminal-shell')).toBeVisible();
    const terminal = await page.locator('.terminal-shell').boundingBox();
    expect(terminal?.width ?? 0).toBeGreaterThan(200);

    await page.getByRole('tab', { name: /Changes/ }).click();

    await expect(
      page.getByRole('button', { name: /README\.md/ }),
    ).toBeVisible();
    await expect(
      page.getByText('+changed through the panel'),
    ).toBeVisible();
  } finally {
    await closeSession(request, session.id);
  }
});
