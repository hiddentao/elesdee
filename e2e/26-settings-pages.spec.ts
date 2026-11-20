import { expect, test, type APIRequestContext } from '@playwright/test';
import { CORE_URL, openApp } from './helpers';

async function savedSettings(request: APIRequestContext) {
  const response = await request.get(`${CORE_URL}/api/bootstrap`);
  expect(response.ok()).toBeTruthy();
  const body = (await response.json()) as {
    settings: { startAtLogin: boolean; keepAwake: boolean };
  };
  return body.settings;
}

test('General settings persist', async ({
  page,
  request,
}) => {
  await openApp(page);

  const before = await savedSettings(request);
  expect(before.startAtLogin).toBe(false);

  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  let settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'General' }).click();

  const start = settings.getByRole('switch', {
    name: 'Start elesdee when the system starts',
  });
  await expect(start).toHaveAttribute('aria-checked', 'false');
  await start.click();
  await expect(start).toHaveAttribute('aria-checked', 'true');

  await expect
    .poll(async () => (await savedSettings(request)).startAtLogin)
    .toBe(true);

  await settings.getByRole('button', { name: 'Close settings' }).click();
  await page.reload();
  await expect(page.locator('.sidebar')).toBeVisible();

  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'General' }).click();
  await expect(
    settings.getByRole('switch', {
      name: 'Start elesdee when the system starts',
    }),
  ).toHaveAttribute('aria-checked', 'true');

  await settings
    .getByRole('switch', {
      name: 'Start elesdee when the system starts',
    })
    .click();
  await expect
    .poll(async () => (await savedSettings(request)).startAtLogin)
    .toBe(false);

});
