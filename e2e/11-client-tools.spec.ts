import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import { dataDir } from './env';
import {
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  openApp,
  uniqueName,
} from './helpers';

test('gives Codex the elesdee MCP server at launch', async ({
  page,
  request,
}) => {
  const label = uniqueName('codex-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('Codex session'),
    folder: repo,
    accountId: 'codex',
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    `arg:mcp_servers.elesdee.url="http://127.0.0.1:4399/internal/mcp/${session.id}"`,
    { timeout: 15_000 },
  );

  await closeSession(request, session.id);
});

test('gives OpenCode the prompt and a merged MCP config', async ({
  page,
  request,
}) => {
  const label = uniqueName('opencode-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const account = await createAccount(request, {
    name: uniqueName('OpenCode account'),
    providerId: 'openrouter',
    key: 'sk-openrouter-e2e',
  });
  const session = await createAgentSession(request, {
    projectId: project.id,
    name: uniqueName('OpenCode session'),
    folder: repo,
    accountId: account.id,
    firstPrompt: 'Look at the code',
  });

  await openApp(page);
  await page.getByRole('button', { name: new RegExp(session.name) }).click();
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'arg:--prompt',
    { timeout: 15_000 },
  );
  await expect(page.locator('.xterm-accessibility')).toContainText(
    'arg:Look at the code',
  );

  const configPath = path.join(dataDir(), 'mcp', `opencode-${session.id}.json`);
  await expect
    .poll(() => fs.existsSync(configPath), { timeout: 15_000 })
    .toBe(true);
  const config = JSON.parse(fs.readFileSync(configPath, 'utf8')) as {
    mcp: { elesdee: { url: string } };
  };
  expect(config.mcp.elesdee.url).toBe(
    `http://127.0.0.1:4399/internal/mcp/${session.id}`,
  );

  await expect(page.locator('.xterm-accessibility')).toContainText(
    `env-opencode-config:${configPath}`,
  );

  await closeSession(request, session.id);
});
