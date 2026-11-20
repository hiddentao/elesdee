import { expect, test } from '@playwright/test';

test.use({ locale: 'en-US' });

test('changes the phone language without changing the desktop language', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/phone.html');

  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('.phone-title')).toHaveText('Sessions');

  const languageButton = page.getByRole('button', { name: 'Languages' });
  await languageButton.click();

  const sheet = page.getByRole('dialog', { name: 'Languages' });
  await expect(sheet).toBeVisible();
  await expect(
    sheet.getByRole('button', { name: 'English' }),
  ).toHaveAttribute('aria-pressed', 'true');

  await page.keyboard.press('Escape');
  await expect(sheet).toBeHidden();
  await expect(languageButton).toBeFocused();

  await languageButton.click();
  await page
    .getByRole('dialog', { name: 'Languages' })
    .getByRole('button', { name: 'Deutsch' })
    .click();

  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await expect(page.locator('.phone-title')).toHaveText('Sitzungen');
  await expect(page.locator('.phone-sub')).toHaveText('Dieser Computer');
  await expect(page.getByRole('button', { name: 'Sprachen' })).toBeFocused();

  const stored = await page.evaluate(() => ({
    phone: window.localStorage.getItem('elesdee.phone-language'),
    desktop: window.localStorage.getItem('elesdee.language'),
  }));
  expect(stored.phone).toBe('de');
  expect(stored.desktop).toBeNull();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);

  await page.goto('/');
  await expect(page.locator('.sidebar')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('.sidebar')).toContainText('Sessions');
});
