import { expect, test } from '@playwright/test';
import { repoDir } from './env';
import {
  claudeInstalled,
  closeSession,
  createAgentSession,
  createProject,
  deleteSession,
  openApp,
  readClaudeHookUrl,
  uniqueName,
} from './helpers';

test('answers a Claude Code permission prompt', async ({ page, request }) => {
  test.skip(
    !(await claudeInstalled(request)),
    'Claude Code is not installed.',
  );

  const label = uniqueName('permission-project');
  const project = await createProject(request, label, repoDir());
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('e2e-agent'),
    folder: repoDir(),
  });
  const hookUrl = await readClaudeHookUrl(session.id);

  await openApp(page);
  const row = page.getByRole('button', {
    name: new RegExp(session.name),
  });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.locator('.xterm')).toBeVisible();

  const hookPromise = request.post(hookUrl, {
    data: {
      hook_event_name: 'PreToolUse',
      tool_name: 'Bash',
      tool_input: { command: 'echo e2e-hook' },
      tool_use_id: 'e2e-request',
    },
    timeout: 60_000,
  });

  await expect(
    page.getByRole('heading', { name: 'Permission needed' }),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page.locator('.permission-tool')).toContainText('Bash');
  await expect(page.locator('.permission-summary')).toContainText(
    'echo e2e-hook',
  );

  await page.locator('.branch-pill').click();
  await expect(
    page.getByRole('menuitem', { name: 'Copy branch name' }),
  ).toBeVisible();
  await page.getByRole('menuitem', { name: 'Copy branch name' }).click();
  await expect(page.getByText('Branch name copied.')).toBeVisible();

  await page.getByRole('button', { name: 'Allow' }).click();

  const hookResponse = await hookPromise;
  expect(hookResponse.ok()).toBeTruthy();
  const decision = (await hookResponse.json()) as {
    hookSpecificOutput: { permissionDecision: string };
  };
  expect(decision.hookSpecificOutput.permissionDecision).toBe('allow');

  await closeSession(request, session.id);
  await deleteSession(request, session.id);
});
