import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  closeSession,
  createGitRepo,
  createProject,
  createProjectFolder,
  createTerminalSession,
  deleteSession,
  getSession,
  openApp,
  sessionRow,
  uniqueName,
} from './helpers';

test('the new session dialog holds focus, closes on Escape and gives focus back', async ({
  page,
  request,
}) => {
  const label = uniqueName('keys-new-session');
  const folder = createProjectFolder(label);
  await createProject(request, label, folder);

  await openApp(page);
  const newSession = page
    .locator('.titlebar')
    .getByRole('button', { name: 'New session' });
  await newSession.click();

  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByRole('combobox', { name: 'Project' }),
  ).toBeFocused();

  for (let index = 0; index < 30; index += 1) {
    await page.keyboard.press('Tab');
    await expect(dialog.locator(':focus')).toHaveCount(1);
  }

  await page.keyboard.press('Escape');
  await expect(dialog).not.toBeVisible();
  await expect(newSession).toBeFocused();
});

test('the session panel tabs respond to arrow keys and keep one tab stop', async ({
  page,
  request,
}) => {
  const label = uniqueName('keys-dock');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const name = uniqueName('keys-dock-session');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder,
  });

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();
    const panelToggle = page.getByRole('button', { name: 'Session panel' });
    await expect(panelToggle).not.toHaveAttribute('aria-controls');
    await panelToggle.click();
    await expect(panelToggle).toHaveAttribute(
      'aria-controls',
      'session-side-panel',
    );

    const tablist = page.getByRole('tablist', { name: 'Session panels' });
    const files = tablist.getByRole('tab', { name: 'Files', exact: true });
    const changes = tablist.getByRole('tab', { name: 'Changes', exact: true });
    const browser = tablist.getByRole('tab', { name: 'Browser', exact: true });

    await expect(tablist.getByRole('tab')).toHaveCount(3);
    await files.click();
    await expect(files).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#dock-panel-files')).toBeVisible();
    await expect(
      tablist.locator('[role="tab"][tabindex="0"]'),
    ).toHaveCount(1);

    await page.keyboard.press('ArrowRight');
    await expect(changes).toBeFocused();
    await expect(changes).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#dock-panel-changes')).toBeVisible();
    await expect(page.locator('#dock-panel-files')).toBeHidden();

    await page.keyboard.press('ArrowRight');
    await expect(browser).toBeFocused();
    await expect(browser).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#dock-panel-browser')).toBeVisible();

    await page.keyboard.press('ArrowRight');
    await expect(files).toBeFocused();
    await expect(files).toHaveAttribute('aria-selected', 'true');

    await page.keyboard.press('ArrowLeft');
    await expect(browser).toBeFocused();
    await expect(browser).toHaveAttribute('aria-selected', 'true');

    await page.keyboard.press('Home');
    await expect(files).toBeFocused();
    await expect(files).toHaveAttribute('aria-selected', 'true');

    await page.keyboard.press('End');
    await expect(browser).toBeFocused();
    await expect(browser).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#dock-panel-browser')).toBeVisible();
    await expect(
      tablist.locator('[role="tab"][tabindex="0"]'),
    ).toHaveCount(1);

    await expect(tablist.getByRole('button')).toHaveCount(0);
    await page.getByRole('button', { name: 'Close side panel' }).focus();
    await page.keyboard.press('ArrowRight');
    await expect(browser).toHaveAttribute('aria-selected', 'true');
  } finally {
    await closeSession(request, session.id);
  }
});

test('keeps focus on the session row when the side panel changes session', async ({
  page,
  request,
}) => {
  const label = uniqueName('keys-panel-switch');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const firstName = uniqueName('keys-panel-first');
  const secondName = uniqueName('keys-panel-second');
  const first = await createTerminalSession(request, {
    projectId: project.id,
    name: firstName,
    folder,
  });
  const second = await createTerminalSession(request, {
    projectId: project.id,
    name: secondName,
    folder,
  });

  try {
    await openApp(page);
    await sessionRow(page, firstName).first().click();
    const panelToggle = page.getByRole('button', { name: 'Session panel' });
    await panelToggle.click();
    await expect(
      page.getByRole('tablist', { name: 'Session panels' }),
    ).toBeVisible();

    const secondRow = sessionRow(page, secondName).first();
    await secondRow.click();

    await expect(secondRow).toBeFocused();
    await expect(panelToggle).not.toBeFocused();
  } finally {
    await closeSession(request, first.id);
    await closeSession(request, second.id);
  }
});

test('the file tree moves, expands, collapses and opens files from the keyboard', async ({
  page,
  request,
}) => {
  const label = uniqueName('keys-tree');
  const repo = createGitRepo(label);
  fs.mkdirSync(path.join(repo, 'alpha'));
  fs.writeFileSync(path.join(repo, 'alpha', 'inside.txt'), 'inside alpha\n');
  fs.writeFileSync(path.join(repo, 'beta.txt'), 'beta file\n');
  fs.writeFileSync(path.join(repo, 'gamma.md'), '# gamma\n');
  const project = await createProject(request, label, repo);
  const name = uniqueName('keys-tree-session');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();
    await page.getByRole('button', { name: 'Session panel' }).click();

    const alpha = page.locator('[data-tree-path="alpha"]');
    const inside = page.locator('[data-tree-path="alpha/inside.txt"]');
    const beta = page.locator('[data-tree-path="beta.txt"]');
    await expect(alpha).toBeVisible();
    await alpha.focus();

    await page.keyboard.press('ArrowDown');
    await expect(beta).toBeFocused();
    await page.keyboard.press('ArrowUp');
    await expect(alpha).toBeFocused();

    await page.keyboard.press('ArrowRight');
    await expect(alpha).toHaveAttribute('aria-expanded', 'true');
    await expect(inside).toBeVisible();
    await expect(alpha).toBeFocused();

    await page.keyboard.press('ArrowDown');
    await expect(inside).toBeFocused();

    await page.keyboard.press('ArrowLeft');
    await expect(alpha).toBeFocused();
    await expect(alpha).toHaveAttribute('aria-expanded', 'true');

    await page.keyboard.press('ArrowLeft');
    await expect(alpha).toHaveAttribute('aria-expanded', 'false');
    await expect(inside).toBeHidden();
    await expect(alpha).toBeFocused();

    await page.keyboard.press('ArrowDown');
    await expect(beta).toBeFocused();

    await page.keyboard.press('Enter');
    await expect(page.locator('.editor-path')).toHaveText('beta.txt');
    await expect(beta).toHaveAttribute('aria-selected', 'true');
    await expect(beta.locator('button')).not.toHaveAttribute('aria-current');
    await expect(page.locator('.cm-content')).toBeVisible();
    await expect(page.locator('.cm-content')).toContainText('beta file');
  } finally {
    await closeSession(request, session.id);
  }
});

test('moves a session to another project from the actions menu', async ({
  page,
  request,
}) => {
  const sourceLabel = uniqueName('keys-move-source');
  const sourceFolder = createProjectFolder(sourceLabel);
  const source = await createProject(request, sourceLabel, sourceFolder);
  const targetLabel = uniqueName('keys-move-target');
  const targetFolder = createProjectFolder(targetLabel);
  const target = await createProject(request, targetLabel, targetFolder);
  const name = uniqueName('keys-move-session');
  const session = await createTerminalSession(request, {
    projectId: source.id,
    name,
    folder: sourceFolder,
  });

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session actions' }).click();
    await page.getByRole('menuitem', { name: /Move to project/ }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    const projectField = dialog.getByRole('combobox', { name: 'Project' });
    await projectField.click();
    await projectField.fill(targetLabel);
    await page
      .getByRole('option', { name: new RegExp(targetLabel) })
      .click();
    await dialog.getByRole('button', { name: 'Move', exact: true }).click();

    await expect(dialog).not.toBeVisible();
    await expect(page.getByText('Session moved.')).toBeVisible();
    await expect(sessionRow(page, name).first()).toBeFocused();
    await expect
      .poll(async () => (await getSession(request, session.id))?.projectId)
      .toBe(target.id);
  } finally {
    await closeSession(request, session.id);
    await deleteSession(request, session.id);
  }
});
