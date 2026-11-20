import { expect, test } from '@playwright/test';
import {
  createProject,
  createProjectFolder,
  getSession,
  openApp,
  openHeaderMenu,
  uniqueName,
  waitForPreviewChange,
} from './helpers';

test('terminal session create, echo, close, reopen and delete', async ({
  page,
  request,
}) => {
  await openApp(page);

  const label = uniqueName('terminal-project');
  const folder = createProjectFolder(label);
  await createProject(request, label, folder);
  await openApp(page);

  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');

  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();
  await dialog.getByRole('radio', { name: 'Terminal' }).click();
  await expect(dialog.getByLabel('Folder')).toHaveValue(folder);

  const sessionName = uniqueName('e2e-terminal');
  await dialog.getByLabel('Session name (optional)').fill(sessionName);

  const [createResponse] = await Promise.all([
    page.waitForResponse(
      (response) =>
        response.url().endsWith('/api/sessions') &&
        response.request().method() === 'POST',
    ),
    dialog.getByRole('button', { name: 'Start session' }).click(),
  ]);
  const created = (await createResponse.json()) as {
    id: string;
    preview: string;
  };

  await expect(dialog).not.toBeVisible();
  await expect(
    page.getByRole('tab', { name: new RegExp(sessionName) }),
  ).toBeVisible();
  await expect(
    page.getByRole('heading', { name: sessionName }),
  ).toBeVisible();

  await expect(page.locator('.xterm')).toBeVisible();
  await page.waitForTimeout(800);
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('echo e2e-ok');
  await page.keyboard.press('Enter');

  await waitForPreviewChange(request, created.id, created.preview);
  await expect(page.locator('.xterm-accessibility')).toContainText('e2e-ok', {
    timeout: 15_000,
  });
  const row = page.getByRole('button', { name: new RegExp(sessionName) });
  await expect(row).not.toContainText(created.preview);

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Close…' }).click();
  const closeDialog = page.getByRole('dialog');
  await expect(closeDialog).toContainText(
    'The terminal stops. The folder stays as it is.',
  );
  await closeDialog.getByRole('button', { name: 'Close session' }).click();
  await expect(closeDialog).not.toBeVisible();

  await expect
    .poll(async () => (await getSession(request, created.id))?.status)
    .toBe('closed');
  await expect(
    page.getByRole('tab', { name: new RegExp(sessionName) }),
  ).not.toBeVisible();
  await expect(row).not.toBeVisible();

  await page.getByRole('button', { name: 'Show closed' }).click();
  await expect(row).toBeVisible();
  await expect(row).toContainText('Closed');

  await row.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Reopen' }).click();
  await expect(
    page.getByRole('tab', { name: new RegExp(sessionName) }),
  ).toBeVisible();
  await expect
    .poll(async () => (await getSession(request, created.id))?.status)
    .toBe('active');

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Close…' }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Close session' })
    .click();
  await expect
    .poll(async () => (await getSession(request, created.id))?.status)
    .toBe('closed');

  await expect(row).toBeVisible();
  await row.click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Delete…' }).click();
  const deleteDialog = page.getByRole('dialog');
  await expect(deleteDialog).toContainText(
    'The session and its log are removed from the app.',
  );
  await deleteDialog.getByRole('button', { name: 'Delete' }).click();
  await expect(deleteDialog).not.toBeVisible();
  await expect(row).not.toBeVisible();
});
