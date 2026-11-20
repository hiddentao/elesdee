import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { getRunDir } from './env';
import { CORE_URL, openApp, uniqueName } from './helpers';

function setSignInResult(result: 'success' | 'refuse' | 'wait'): void {
  fs.writeFileSync(
    path.join(getRunDir(), 'sign-in-control.txt'),
    result,
  );
}

test('connects an Anthropic subscription through Claude Code', async ({
  page,
  request,
}) => {
  setSignInResult('success');
  const name = uniqueName('signin-connected');

  await openApp(page);
  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Add provider' }).click();
  const add = page.getByRole('dialog', { name: 'Add provider' });

  await add.getByRole('radio', { name: 'Subscription' }).click();
  await add.getByLabel('Provider name (optional)').fill(name);
  await add.getByRole('button', { name: 'Sign in and add' }).click();

  await expect(add).not.toBeVisible({ timeout: 20_000 });

  const response = await request.get(`${CORE_URL}/api/providers?q=`);
  expect(response.ok()).toBeTruthy();
  const providers = (await response.json()) as Array<{
    name: string;
    vendor: string;
    auth: string;
    connected: boolean;
  }>;
  const provider = providers.find((item) => item.name === name);
  expect(provider, JSON.stringify(providers)).toBeTruthy();
  expect(provider?.vendor).toBe('anthropic');
  expect(provider?.auth).toBe('subscription');
  expect(provider?.connected).toBe(true);

  await expect(settings.getByText(name)).toBeVisible();
  await expect(
    settings.locator('.connection-tag.is-connected'),
  ).toHaveText('Connected');
});

test('a refused sign-in leaves the provider unconnected with a reason', async ({
  page,
}) => {
  setSignInResult('refuse');
  const name = uniqueName('signin-refused');

  await openApp(page);
  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Add provider' }).click();
  const add = page.getByRole('dialog', { name: 'Add provider' });

  await add.getByRole('radio', { name: 'Subscription' }).click();
  await add.getByLabel('Provider name (optional)').fill(name);
  await add.getByRole('button', { name: 'Sign in and add' }).click();

  await expect(
    add.getByText('The sign-in did not complete. Try again.'),
  ).toBeVisible({ timeout: 20_000 });
  await add.getByRole('button', { name: 'Cancel' }).click();
  await expect(add).not.toBeVisible();
  await expect(settings.getByText(name)).toHaveCount(0);
});
