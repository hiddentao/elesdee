import { expect, test } from '@playwright/test';
import { openApp } from './helpers';

test('the usage footer opens the panel with every seeded account', async ({
  page,
}) => {
  await openApp(page);

  await page.locator('.usage-summary-button').click();
  const dialog = page.getByRole('dialog');

  await expect(dialog.getByRole('heading', { name: 'Usage' })).toBeVisible();

  for (const name of [
    'Anthropic Max 20×',
    'ChatGPT Pro',
    'DeepSeek API',
    'LiteLLM gateway',
  ]) {
    await expect(
      dialog.getByRole('region', { name }),
    ).toBeVisible();
  }

  await expect(
    dialog.getByRole('region', { name: 'LiteLLM gateway' }),
  ).toContainText('No usage data');

  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(dialog).not.toBeVisible();
});
