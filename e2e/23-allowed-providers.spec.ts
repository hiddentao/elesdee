import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  getSession,
  openApp,
  uniqueName,
} from './helpers';

test('the switch chooser offers only the allowed accounts', async ({
  page,
  request,
}) => {
  const label = uniqueName('allowed-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAccount(request, {
    name: uniqueName('Allowed A'),
    key: 'sk-allowed-a',
  });
  const second = await createAccount(request, {
    name: uniqueName('Allowed B'),
    key: 'sk-allowed-b',
  });
  const outside = await createAccount(request, {
    name: uniqueName('Outside C'),
    key: 'sk-allowed-c',
  });

  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Allowed session'),
    folder: repo,
    accountId: first.id,
    allowedAccountIds: [first.id, second.id],
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-anthropic-key:[redacted]',
    { timeout: 15_000 },
  );

  await page.locator('.session-head .provider-pill').click();
  const dialog = page.getByRole('dialog', { name: 'Switch account' });
  await expect(dialog).toBeVisible();

  const allowedList = dialog.getByRole('list', {
    name: 'Allowed for this session',
  });
  await expect(allowedList).toContainText(first.name);
  await expect(allowedList).toContainText(second.name);
  await expect(allowedList).not.toContainText(outside.name);

  const combo = dialog.getByRole('combobox', {
    name: 'Account',
    exact: true,
  });
  await combo.click();
  await combo.fill(outside.name);
  await expect(page.getByText('No matches.')).toBeVisible();
  await expect(
    page.getByRole('option', { name: new RegExp(outside.name) }),
  ).toHaveCount(0);

  await combo.fill(second.name);
  await page.getByRole('option', { name: new RegExp(second.name) }).click();
  await dialog.getByRole('button', { name: 'Switch account' }).click();

  await expect(dialog).not.toBeVisible();
  await expect
    .poll(async () => (await getSession(request, session.id))?.accountId)
    .toBe(second.id);

  await closeSession(request, session.id);
});
