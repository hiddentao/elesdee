import { expect, test } from '@playwright/test';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createConfig,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('one provider runs two configs and switching reaches the launch', async ({
  page,
  request,
}) => {
  const label = uniqueName('providers-configs');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);

  const provider = await createAccount(request, {
    name: `${label} provider`,
    providerId: 'deepseek',
    key: 'sk-e2e-provider',
    client: 'Claude Code',
  });
  const claude = await createConfig(request, {
    providerId: provider.id,
    client: 'Claude Code',
    name: `${label} Claude`,
    model: 'claude-sonnet-4-5',
    effort: 'max',
  });
  const codex = await createConfig(request, {
    providerId: provider.id,
    client: 'Codex',
    name: `${label} Codex`,
    model: 'gpt-5.2',
    effort: 'high',
  });

  const session = await createAgentSession(request, {
    projectId: project.id,
    folder: repo,
    name: `${label} session`,
    configId: claude.id,
    allowedConfigIds: [claude.id, codex.id],
  });

  await openApp(page);
  await page
    .locator('.session-list')
    .getByRole('button', { name: new RegExp(session.name) })
    .first()
    .click();

  const terminal = page.locator('.xterm-accessibility');
  await expect(terminal).toContainText('arg:claude-sonnet-4-5', {
    timeout: 15_000,
  });
  await expect(terminal).toContainText('arg:max');

  await page.locator('.provider-pill').click();
  const dialog = page.getByRole('dialog', { name: 'Switch AI config' });
  await expect(dialog).toBeVisible();

  const configCombo = dialog.getByRole('combobox', { name: 'AI config', exact: true });
  await configCombo.click();
  await page
    .getByRole('option', { name: new RegExp(codex.name) })
    .click();
  await dialog.getByRole('button', { name: 'Switch AI config' }).click();
  await expect(dialog).not.toBeVisible();

  await expect(terminal).toContainText('model_reasoning_effort="high"', {
    timeout: 15_000,
  });
  await expect(terminal).toContainText('model="gpt-5.2"');

  await closeSession(request, session.id);
});
