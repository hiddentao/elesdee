import { expect, test } from '@playwright/test';
import {
  createProject,
  createProjectFolder,
  deleteSession,
  getSession,
  openApp,
  openHeaderMenu,
  uniqueName,
  waitForPreviewChange,
} from './helpers';

test('a closed session replays its log without a disconnect banner', async ({
  page,
  request,
}) => {
  const label = uniqueName('closed-project');
  const folder = createProjectFolder(label);
  await createProject(request, label, folder);
  const sessionName = uniqueName('e2e-closed');

  await openApp(page);
  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');
  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();
  await dialog.getByRole('radio', { name: 'Terminal' }).click();
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

  await expect(page.locator('.xterm')).toBeVisible();
  await page.waitForTimeout(800);
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.type('echo e2e-ok');
  await page.keyboard.press('Enter');
  await waitForPreviewChange(request, created.id, created.preview);
  await expect(page.locator('.xterm-accessibility')).toContainText('e2e-ok', {
    timeout: 15_000,
  });

  await openHeaderMenu(page);
  await page.getByRole('menuitem', { name: 'Close…' }).click();
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Close session' })
    .click();
  await expect
    .poll(async () => (await getSession(request, created.id))?.status)
    .toBe('closed');

  await page.getByRole('button', { name: 'Show closed' }).click();
  const row = page.getByRole('button', { name: new RegExp(sessionName) });
  await expect(row).toBeVisible();
  await row.click();

  await expect(
    page.getByRole('tab', { name: new RegExp(sessionName) }),
  ).toBeVisible();
  await expect(page.locator('.xterm')).toBeVisible();
  await expect(page.getByText('Output is disconnected.')).toHaveCount(0);
  await expect(page.locator('.xterm-accessibility')).toContainText('e2e-ok', {
    timeout: 15_000,
  });

  await deleteSession(request, created.id);
});
