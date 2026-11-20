import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { dataDir } from './env';
import { CORE_URL, openApp, uniqueName } from './helpers';

test('edits an account name and removes an unreferenced account', async ({
  page,
  request,
}) => {
  const name = uniqueName('Edit account');
  const editedName = `${name} edited`;
  const created = await request.post(`${CORE_URL}/api/accounts`, {
    data: {
      name,
      providerId: 'deepseek',
      auth: 'key',
      key: 'sk-e2e-account-edit',
    },
  });
  expect(created.ok()).toBeTruthy();
  const account = (await created.json()) as { id: string };

  await openApp(page);
  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  const settings = page.getByRole('dialog', { name: 'Settings' });

  await settings
    .getByRole('button', { name: `Edit account ${name}` })
    .click();
  const edit = page.getByRole('dialog', { name: 'Edit account' });
  await expect(edit.getByLabel('Account name')).toHaveValue(name);
  await edit.getByLabel('Account name').fill(editedName);
  await edit.getByRole('button', { name: 'Save' }).click();

  await expect(edit).not.toBeVisible();
  await expect(settings.getByText(editedName)).toBeVisible();

  await settings
    .getByRole('button', { name: `Remove account ${editedName}` })
    .click();
  const confirm = page.getByRole('dialog', { name: 'Remove account' });
  await expect(confirm.getByText(new RegExp(editedName))).toBeVisible();
  await confirm.getByRole('button', { name: 'Remove account' }).click();

  await expect(confirm).not.toBeVisible();
  await expect(settings.getByText(editedName)).not.toBeVisible();

  const bootstrap = await request.get(`${CORE_URL}/api/bootstrap`);
  expect(bootstrap.ok()).toBeTruthy();
  const data = (await bootstrap.json()) as {
    accounts: { id: string; name: string }[];
  };
  expect(data.accounts.some((item) => item.id === account.id)).toBe(false);

  const state = fs.readFileSync(path.join(dataDir(), 'state.json'), 'utf8');
  expect(state).not.toContain('sk-e2e-account-edit');
  expect(state).not.toContain(editedName);
});
