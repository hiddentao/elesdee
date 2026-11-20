import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAgentSession,
  createGitRepo,
  createProject,
  readClaudeHookUrl,
  uniqueName,
} from './helpers';

test('phone message view shows prompts, replies and tool rows from hooks', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-messages');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('phone-messages-session'),
    folder: repo,
  });

  const hookUrl = await readClaudeHookUrl(session.id);

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/phone.html#session=${encodeURIComponent(session.id)}`);

  await expect(page.locator('.phone-context')).toBeVisible();
  await expect(page.getByText('No messages yet.')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Prompt' })).toBeVisible();
  await page.getByRole('button', { name: 'Session view' }).click();
  await expect(page.locator('.xterm')).toBeVisible();
  await expect(page.locator('.phone-view-toggle')).toHaveText('Messages');
  await page.getByRole('button', { name: 'Session view' }).click();
  await expect(page.getByRole('textbox', { name: 'Prompt' })).toBeVisible();

  const prompt = uniqueName('Fix the failing test');
  await request.post(hookUrl, {
    data: {
      hook_event_name: 'UserPromptSubmit',
      prompt_id: 'prompt-e2e',
      prompt,
    },
  });

  const reply = uniqueName('The test is fixed');
  await request.post(hookUrl, {
    data: {
      hook_event_name: 'MessageDisplay',
      message_id: 'message-e2e',
      final: true,
      delta: reply,
    },
  });

  await request.post(hookUrl, {
    data: {
      hook_event_name: 'PreToolUse',
      tool_name: 'Read',
      tool_use_id: 'tool-e2e',
      tool_input: { file_path: 'README.md' },
    },
  });
  await request.post(hookUrl, {
    data: {
      hook_event_name: 'PostToolUse',
      tool_name: 'Read',
      tool_use_id: 'tool-e2e',
      tool_input: { file_path: 'README.md' },
      tool_response: { is_error: false },
    },
  });
  await request.post(hookUrl, {
    data: {
      hook_event_name: 'Stop',
      turn_id: 'turn-e2e',
      last_assistant_message: reply,
    },
  });

  const messages = page.locator('.phone-messages');
  await expect(messages).toContainText(prompt, { timeout: 20_000 });
  await expect(messages).toContainText(reply);
  const toolRow = messages.getByRole('button', { name: /Read/ });
  await expect(toolRow).toContainText('README.md');
  await expect(toolRow).toHaveAttribute('aria-expanded', 'false');

  await toolRow.click();
  await expect(toolRow).toHaveAttribute('aria-expanded', 'true');
  await expect(messages.locator('.phone-tool-detail')).toContainText('README.md');

  await expect(page.locator('.phone-context')).toContainText('Finished');

  await closeSession(request, session.id);
});
