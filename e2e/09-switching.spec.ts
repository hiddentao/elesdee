import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  getSession,
  openApp,
  readClaudeStatusLineUrl,
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
    configId: first.configId,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-anthropic-key:[redacted]',
    { timeout: 15_000 },
  );

  await page.getByRole('button', { name: 'Session actions' }).click();
  await page.getByRole('menuitem', { name: 'Switch AI config…' }).click();
  const dialog = page.getByRole('dialog', { name: 'Switch AI config' });
  const combo = dialog.getByRole('combobox', { name: 'AI config', exact: true });
  await combo.click();
  await combo.fill(second.name);
  await page.getByRole('option', { name: new RegExp(second.name) }).click();
  await dialog.getByRole('button', { name: 'Switch AI config' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.getByText('AI config switched.')).toBeVisible();
  await expect
    .poll(async () => (await getSession(request, session.id))?.configId)
    .toBe(second.configId);

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
    configId: first.configId,
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  await setUsage(request, first.id, [{ label: 'Weekly', used: 95 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.configId)
    .toBe(second.configId);
  await expect(page.locator('.provider-account')).toContainText(second.name);

  await setUsage(request, second.id, [{ label: 'Weekly', used: 95 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.workState)
    .toBe('paused');
  await expect(page.locator('.status-badge')).toHaveAttribute(
    'data-status',
    'paused',
  );
  await expect(page.locator('.waiting-note')).toContainText(
    'The work is committed, but the branch has no remote.',
  );

  await setUsage(request, first.id, [{ label: 'Weekly', used: 30 }]);
  await expect
    .poll(async () => (await getSession(request, session.id))?.workState)
    .toBe('working');
  await expect(page.locator('.status-badge')).toHaveAttribute(
    'data-status',
    'running',
  );

  await closeSession(request, session.id);
});

test('switches from the usage pill and shows the context left', async ({
  page,
  request,
}) => {
  const label = uniqueName('pill-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAccount(request, {
    name: uniqueName('Pill A'),
    key: 'sk-pill-a',
  });
  const second = await createAccount(request, {
    name: uniqueName('Pill B'),
    key: 'sk-pill-b',
  });
  await setUsage(request, second.id, [{ label: 'Weekly', used: 5 }]);

  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Pill session'),
    folder: repo,
    configId: first.configId,
  });

  const statusLineUrl = await readClaudeStatusLineUrl(session.id);
  const report = await request.post(statusLineUrl, {
    data: {
      session_id: 'e2e-pill-context',
      context_window: { remaining_percentage: 63.4 },
    },
  });
  expect(report.ok()).toBeTruthy();
  await expect
    .poll(async () => (await getSession(request, session.id))?.contextLeft)
    .toBe(63);

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();

  const pill = page.locator('.session-head .provider-pill');
  await expect(pill).toContainText(first.name);
  await pill.hover();
  const tip = page.getByRole('tooltip');
  await expect(tip).toContainText('Context left');
  await expect(tip).toContainText('63% left');

  await pill.click();
  const dialog = page.getByRole('dialog', { name: 'Switch AI config' });
  await expect(dialog).toBeVisible();
  const combo = dialog.getByRole('combobox', { name: 'AI config', exact: true });
  await combo.click();
  await combo.fill(second.name);
  const option = page.getByRole('option', { name: new RegExp(second.name) });
  await expect(option).toContainText('95% left');
  await expect(option).toContainText('Next auto-switch');
  await option.click();
  await dialog.getByRole('button', { name: 'Switch AI config' }).click();

  await expect(dialog).not.toBeVisible();
  await expect(page.getByText('AI config switched.')).toBeVisible();
  await expect
    .poll(async () => (await getSession(request, session.id))?.configId)
    .toBe(second.configId);
  await expect
    .poll(async () => (await getSession(request, session.id))?.contextLeft)
    .toBeNull();
  await expect(page.locator('.session-head .provider-account')).toContainText(
    second.name,
  );

  await pill.hover();
  await expect(page.getByRole('tooltip')).toContainText(
    'Context left unknown',
  );

  await closeSession(request, session.id);
});
