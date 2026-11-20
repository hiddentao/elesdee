import { expect, test } from '@playwright/test';
import { openApp } from './helpers';

test.use({ locale: 'en-US' });

test('switches the interface language, keeps the choice and switches back', async ({
  page,
}) => {
  await openApp(page);
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');

  await page.getByRole('button', { name: 'Settings' }).click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Languages' }).click();

  await page.locator('#settings-language').selectOption('de');

  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  const german = page.getByRole('dialog', { name: 'Einstellungen' });
  await expect(german).toBeVisible();
  await expect(
    german.getByRole('button', { name: 'Sprachen' }),
  ).toHaveAttribute('aria-current', 'page');
  await expect(german.getByText('Oberflächensprache')).toBeVisible();

  await german.getByRole('button', { name: 'Einstellungen schließen' }).click();
  await expect(page.locator('.sidebar')).toContainText('Sitzungen');
  await expect(page.locator('.sidebar')).toContainText('Einstellungen');

  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await expect(page.locator('.sidebar')).toContainText('Sitzungen');

  await page.getByRole('button', { name: 'Einstellungen' }).click();
  await page
    .getByRole('dialog', { name: 'Einstellungen' })
    .getByRole('button', { name: 'Sprachen' })
    .click();
  await page.locator('#settings-language').selectOption('en');

  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  const english = page.getByRole('dialog', { name: 'Settings' });
  await expect(english).toBeVisible();
  await english.getByRole('button', { name: 'Close settings' }).click();

  await expect(page.locator('.sidebar')).toContainText('Sessions');
  await expect(page.locator('.sidebar')).toContainText('Settings');
});
