import { expect, test } from '@playwright/test';
import {
  CORE_URL,
  closeSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('passes launch environment overrides to the agent', async ({
  page,
  request,
}) => {
  const label = uniqueName('env-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);

  await openApp(page);
  await page.locator('.titlebar').getByRole('button', { name: 'New session' }).click();
  const dialog = page.getByRole('dialog');
  const projectCombo = dialog.getByRole('combobox', { name: 'Project' });
  await projectCombo.click();
  await projectCombo.fill(label);
  await page.getByRole('option', { name: new RegExp(label) }).click();
  const configCombo = dialog.getByRole('combobox', {
    name: 'AI config',
    exact: true,
  });
  await configCombo.click();
  await configCombo.fill('Anthropic Max');
  await page.getByRole('option', { name: /Anthropic Max/ }).click();

  await dialog.getByText('Additional settings').click();
  await dialog
    .getByLabel('Environment variables (optional)')
    .fill('E2E_VALUE=hello');
  await dialog.getByRole('button', { name: 'Start session' }).click();

  await expect(page.locator('.xterm-accessibility')).toContainText(
    'env-e2e-value:hello',
    { timeout: 15_000 },
  );

  const sessions = await request.get(`${CORE_URL}/api/bootstrap`);
  const data = (await sessions.json()) as {
    sessions: { id: string; projectId: string }[];
  };
  const session = data.sessions.find((item) => item.projectId === project.id);
  if (session) {
    await closeSession(request, session.id);
  }
});
