import { expect, test } from '@playwright/test';
import {
  createProject,
  createProjectFolder,
  createTerminalSession,
  getSession,
  openApp,
  openHeaderMenu,
  uniqueName,
} from './helpers';

test('session menu renames, copies, and shows the active and closed items', async ({
  page,
  request,
}) => {
  const label = uniqueName('menu-project');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const original = uniqueName('menu-session');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: original,
    folder,
  });

  await openApp(page);
  const row = page.getByRole('button', { name: new RegExp(original) });
  await row.click();

  await openHeaderMenu(page);
  await expect(page.getByRole('menuitem')).toHaveText([
    'Rename…',
    'Copy folder path',
    'Close…',
  ]);
  await page.keyboard.press('Escape');

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Rename…' }).click();
  const renameDialog = page.getByRole('dialog');
  const renameInput = renameDialog.getByLabel('Name');
  await renameInput.fill('');
  const renamed = uniqueName('renamed');
  await renameInput.fill(renamed);
  await renameDialog.getByRole('button', { name: 'Rename' }).click();
  await expect(renameDialog).not.toBeVisible();

  await expect(
    page.getByRole('heading', { name: renamed }),
  ).toBeVisible();
  await expect(
    page.getByRole('tab', { name: new RegExp(renamed) }),
  ).toBeVisible();
  const renamedRow = page.getByRole('button', { name: new RegExp(renamed) });
  await expect(renamedRow).toBeVisible();

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Copy folder path' }).click();
  await expect(page.getByText('Folder path copied.')).toBeVisible();

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Close…' }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Close session' })
    .click();
  await expect
    .poll(async () => (await getSession(request, session.id))?.status)
    .toBe('closed');
  await expect(renamedRow).not.toBeVisible();

  await page.getByRole('button', { name: 'Show closed' }).click();
  await expect(renamedRow).toBeVisible();

  await renamedRow.click({ button: 'right' });
  await expect(page.getByRole('menuitem')).toHaveText([
    'Reopen',
    'Rename…',
    'Copy folder path',
    'Delete…',
  ]);
  await page.keyboard.press('Escape');

  await renamedRow.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Delete…' }).click();
  const deleteDialog = page.getByRole('dialog');
  await deleteDialog.getByRole('button', { name: 'Delete' }).click();
  await expect(deleteDialog).not.toBeVisible();
  await expect(renamedRow).not.toBeVisible();
});
