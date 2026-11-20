import { expect, test } from '@playwright/test';
import { createProjectFolder, openApp, uniqueName } from './helpers';

test('first launch welcomes the user and a new project is selected', async ({
  page,
}) => {
  await openApp(page);

  await expect(
    page.getByRole('heading', { name: 'Welcome to elesdee' }),
  ).toBeVisible();
  await expect(
    page.getByText('Add a project to start a session on this computer.'),
  ).toBeVisible();
  await expect(page.getByRole('list', { name: 'Agent check' })).toBeVisible();

  const name = uniqueName('ui-project');
  const folder = createProjectFolder(name);

  await page.getByRole('button', { name: 'Add project' }).click();

  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Name').fill(name);
  await dialog.getByRole('textbox', { name: 'Folder' }).fill(folder);
  await dialog.getByRole('button', { name: 'Add project' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole('heading', { name: 'Welcome to elesdee' }),
  ).not.toBeVisible();

  const projectButton = page.getByRole('button', {
    name: new RegExp(`^${name},`),
  });
  await expect(projectButton).toBeVisible();
  await expect(projectButton).toHaveAttribute('aria-current', 'true');
  await expect(page.locator('.tabbar-project')).toContainText(name);

  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const sessionDialog = page.getByRole('dialog');
  await expect(
    sessionDialog.getByRole('combobox', { name: 'Project' }),
  ).toHaveValue(name);
  await sessionDialog.getByRole('button', { name: 'Cancel' }).click();
});
