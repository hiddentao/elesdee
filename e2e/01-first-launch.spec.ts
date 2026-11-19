import { expect, test } from '@playwright/test';
import { createProjectFolder, openApp, uniqueName } from './helpers';

test('seeded projects appear and a new project is added and selected', async ({
  page,
}) => {
  await openApp(page);

  await expect(page.getByRole('button', { name: /^web,/ })).toBeVisible();
  await expect(page.getByRole('button', { name: /^core,/ })).toBeVisible();

  const name = uniqueName('ui-project');
  const folder = createProjectFolder(name);

  await page.getByRole('button', { name: 'Add project' }).click();

  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Name').fill(name);
  await dialog.getByLabel('Folder').fill(folder);
  await dialog.getByRole('button', { name: 'Add project' }).click();

  await expect(dialog).not.toBeVisible();

  const projectButton = page.getByRole('button', {
    name: new RegExp(`^${name},`),
  });
  await expect(projectButton).toBeVisible();
  await expect(projectButton).toHaveAttribute('aria-current', 'true');
  await expect(page.locator('.tabbar-project')).toContainText(name);

  await page.getByRole('button', { name: /New session/ }).click();
  const sessionDialog = page.getByRole('dialog');
  await expect(
    sessionDialog.getByRole('combobox', { name: 'Project' }),
  ).toHaveValue(name);
  await sessionDialog.getByRole('button', { name: 'Cancel' }).click();
});
