import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  getSession,
  openApp,
  setUsage,
  uniqueName,
} from './helpers';

test('switches the account of a running session', async ({ page, request }) => {
  const label = uniqueName('switch-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAccount(request, {
    name: uniqueName('Switch A'),
    key: 'sk-switch-a',
  });
  const second = await createAccount(request, {
    name: uniqueName('Switch B'),
    key: 'sk-switch-b',
  });
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Switch session'),
    folder: repo,
    accountId: first.id,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-anthropic-key:[redacted]',
    { timeout: 15_000 },
  );

  await page.getByRole('button', { name: 'Session actions' }).click();
  await page.getByRole('menuitem', { name: 'Switch account…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Switch account' });
  const combo = dialog.getByRole('combobox', { name: 'Account' });
  await combo.click();
  await combo.fill(second.name);
  await page.getByRole('option', { name: new RegExp(second.name) }).click();
  await dialog.getByRole('button', { name: 'Switch account' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.getByText('Account switched.')).toBeVisible();
  await expect
    .poll(async () => (await getSession(request, session.id))?.accountId)
    .toBe(second.id);

  await closeSession(request, session.id);
});

test('auto-switches near the limit and pauses when no account has room', async ({
  page,
  request,
}) => {
  const label = uniqueName('auto-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAccount(request, {
    name: uniqueName('Auto A'),
    key: 'sk-auto-a',
  });
  const second = await createAccount(request, {
    name: uniqueName('Auto B'),
    key: 'sk-auto-b',
  });
  await setUsage(request, second.id, [{ label: 'Weekly', used: 20 }]);

  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Auto session'),
    folder: repo,
    accountId: first.id,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  await setUsage(request, first.id, [{ label: 'Weekly', used: 95 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.accountId)
    .toBe(second.id);
  await expect(page.locator('.provider-account')).toContainText(second.name);

  await setUsage(request, second.id, [{ label: 'Weekly', used: 95 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.workState)
    .toBe('paused');
  await expect(page.locator('.status-badge')).toContainText('Paused');

  await setUsage(request, first.id, [{ label: 'Weekly', used: 30 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.workState)
    .toBe('working');
  await expect(page.locator('.status-badge')).toContainText('Working');

  await closeSession(request, session.id);
});
