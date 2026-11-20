import { expect, test, type Page } from '@playwright/test';
import {
  bootstrap,
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  getSession,
  uniqueName,
} from './helpers';

async function openSessionActions(page: Page) {
  await page.getByRole('button', { name: 'Session actions' }).click();
  return page.getByRole('dialog', { name: 'Session actions' });
}

test('phone layout sends terminal input and runs session actions', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-actions');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAccount(request, {
    name: uniqueName('Phone First'),
    key: 'sk-phone-first',
  });
  const second = await createAccount(request, {
    name: uniqueName('Phone Second'),
    key: 'sk-phone-second',
  });

  const terminalName = uniqueName('phone-terminal');
  const terminal = await createTerminalSession(request, {
    projectId: project.id,
    name: terminalName,
    folder: repo,
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/phone.html#session=${encodeURIComponent(terminal.id)}`);

  await expect(page.locator('.xterm')).toBeVisible();
  const token = uniqueName('phone-echo');
  const input = page.getByRole('textbox', { name: 'Terminal command' });
  await input.fill(`echo ${token}`);
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(token, {
    timeout: 20_000,
  });

  const rail = page.getByRole('toolbar', { name: 'Keyboard keys' });
  await expect(rail).toBeVisible();
  await page.getByRole('button', { name: 'Hide the key rail' }).click();
  await expect(rail).not.toBeVisible();
  await page.getByRole('button', { name: 'Show the key rail' }).click();
  await expect(rail).toBeVisible();

  const control = page.getByRole('button', { name: 'Control', exact: true });
  await control.click();
  await expect(control).toHaveAttribute('aria-pressed', 'true');
  await rail.getByRole('button', { name: 'Control C' }).click();
  await expect(control).toHaveAttribute('aria-pressed', 'false');

  await expect(rail.getByRole('button', { name: 'Escape' })).toBeVisible();
  await expect(rail.getByRole('button', { name: 'Tab', exact: true })).toBeVisible();

  const switchSession = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('phone-switch'),
    folder: repo,
    configId: first.configId,
    allowedConfigIds: [first.configId, second.configId],
  });

  await page.goto(
    `/phone.html#session=${encodeURIComponent(switchSession.id)}`,
  );
  // A hash-only navigation keeps the running page; a reload selects the hash.
  await page.reload();
  await expect(page.locator('.phone-title')).toHaveText(switchSession.name);

  const actionSheet = await openSessionActions(page);
  await actionSheet.getByRole('button', { name: 'Usage' }).click();
  const usage = page.getByRole('dialog', { name: 'Usage' });
  await expect(usage).toBeVisible();
  await expect(usage).toContainText(first.name);
  await usage.getByRole('button', { name: 'Close' }).click();
  await expect(usage).not.toBeVisible();

  await (
    await openSessionActions(page)
  )
    .getByRole('button', { name: 'Switch AI config…' })
    .click();
  const switchSheet = page.getByRole('dialog', { name: 'Switch AI config' });
  await switchSheet.getByRole('textbox', { name: 'Type to find an AI config' }).fill(second.name);
  await switchSheet.getByRole('button', { name: new RegExp(second.name) }).click();
  await expect
    .poll(async () => (await getSession(request, switchSession.id))?.configId)
    .toBe(second.configId);

  await page.goto('/phone.html');
  await expect(page.getByRole('button', { name: 'New session' })).toBeVisible();
  await page.getByRole('button', { name: 'New session' }).click();
  const start = page.getByRole('dialog', { name: 'New session' });
  const projectCombo = start.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();

  const configCombo = start.getByRole('combobox', { name: 'AI config', exact: true });
  await configCombo.click();
  await configCombo.fill(first.name);
  await page.getByRole('option', { name: new RegExp(first.name) }).click();

  const prompt = uniqueName('phone-launch');
  await start.getByLabel('First prompt (optional)').fill(prompt);
  await start.getByRole('button', { name: 'Start session' }).click();
  await expect(start).not.toBeVisible();
  await expect(page.locator('.phone-title')).toContainText(prompt, {
    timeout: 30_000,
  });

  await (
    await openSessionActions(page)
  )
    .getByRole('button', { name: 'Close…' })
    .click();
  const close = page.getByRole('dialog', { name: 'Close session?' });
  await expect(close).toContainText(
    "Closing commits the agent's work",
  );
  await close.getByRole('button', { name: 'Close session' }).click();
  await expect(close).not.toBeVisible();

  const data = await bootstrap(request);
  for (const session of data.sessions.filter(
    (item) => item.projectId === project.id,
  )) {
    if (session.status === 'active') {
      await closeSession(request, session.id);
    }
  }
});
