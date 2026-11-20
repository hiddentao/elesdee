import { expect, test } from '@playwright/test';
import {
  bootstrap,
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  readClaudeHookUrl,
  uniqueName,
} from './helpers';

test('shows one session tab at a time and lists all tabs in a sheet', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const sessions = [];
  for (let index = 1; index <= 4; index += 1) {
    sessions.push(
      await createAgentSession(request, {
        projectId: project.id,
        name: `Phone session ${index}`,
        folder: repo,
      }),
    );
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/phone.html#session=${encodeURIComponent(sessions[0].id)}`);

  await expect(page.locator('.phone-title')).toHaveText('Phone session 1');
  const count = page.getByRole('button', { name: /All tabs/ });
  const countText = await count.getAttribute('aria-label');
  const total = Number(countText?.split(',').pop()?.trim() ?? '0');
  expect(total).toBeGreaterThanOrEqual(4);

  await page.locator('.phone-tabs').evaluate((element) => {
    element.scrollTo({ left: element.clientWidth, behavior: 'instant' });
  });
  await expect(page.locator('.phone-title')).toHaveText('Phone session 2');

  await count.click();
  const sheet = page.getByRole('dialog', { name: 'All tabs' });
  await expect(sheet).toContainText(`${total} tabs`);
  await expect(sheet).toContainText('Phone session 4');
  await sheet.getByRole('button', { name: /Phone session 4/ }).click();
  await expect(page.locator('.phone-title')).toHaveText('Phone session 4');

  const hookUrl = await readClaudeHookUrl(sessions[0].id);
  const hookPromise = request.post(hookUrl, {
    data: {
      hook_event_name: 'PreToolUse',
      tool_name: 'Bash',
      tool_input: { command: 'echo phone-hook' },
      tool_use_id: 'phone-request',
    },
    timeout: 60_000,
  });

  await page.locator('.phone-tabs').evaluate((element) => {
    element.scrollTo({ left: 0, behavior: 'instant' });
  });
  await expect(page.locator('.phone-title')).toHaveText('Phone session 1');
  await expect(page.locator('.phone-card')).toContainText('echo phone-hook', {
    timeout: 20_000,
  });
  await page.getByRole('button', { name: 'Allow' }).click();
  const hookResponse = await hookPromise;
  expect(hookResponse.ok()).toBeTruthy();

  const data = await bootstrap(request);
  for (const session of data.sessions.filter(
    (item) => item.projectId === project.id,
  )) {
    await closeSession(request, session.id);
  }
});
