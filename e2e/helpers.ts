import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import { dataDir, projectsDir } from './env';

export const CORE_URL =
  process.env.ELESDEE_E2E_CORE_URL ?? 'http://127.0.0.1:4399';

export interface Project {
  id: string;
  name: string;
  folder: string;
  defaultAccountId?: string | null;
  defaultSourceBranch?: string | null;
}

export interface Session {
  id: string;
  name: string;
  projectId: string;
  kind: 'agent' | 'terminal';
  status: 'active' | 'closed';
  workState: string | null;
  accountId: string | null;
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: string | null;
  firstPrompt: string | null;
  contextLeft?: number | null;
  demoServers?: {
    id: string;
    name: string;
    port: number;
    address: string;
    state: 'active' | 'stopped';
  }[];
  pendingPermission: {
    requestId: string;
    tool: string;
    summary: string;
  } | null;
}

interface Bootstrap {
  projects: Project[];
  accounts: { id: string; name: string; rows: unknown[] }[];
  sessions: Session[];
}

interface Health {
  agents: { name: string; installed: boolean; version: string | null }[];
}

export function uniqueName(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 6)}`;
}

export function createProjectFolder(label: string): string {
  const folder = path.join(projectsDir(), label);
  fs.mkdirSync(folder, { recursive: true });
  return fs.realpathSync(folder);
}

export function createBareRepo(label: string): string {
  const folder = path.join(projectsDir(), label);
  fs.mkdirSync(folder, { recursive: true });
  execFileSync('git', ['init', '--bare', '--quiet', folder], { stdio: 'pipe' });
  return fs.realpathSync(folder);
}

export function gitIn(folder: string, args: string[]): string {
  return execFileSync('git', args, { cwd: folder, encoding: 'utf8' }).trim();
}

export function cloneRepo(remote: string, target: string): string {
  execFileSync('git', ['clone', '--quiet', remote, target], { stdio: 'pipe' });
  return fs.realpathSync(target);
}

export function createGitRepo(label: string): string {
  const folder = path.join(projectsDir(), label);
  fs.mkdirSync(folder, { recursive: true });
  const git = (...args: string[]) =>
    execFileSync('git', args, { cwd: folder, stdio: 'pipe' });
  git('init', '-q', '-b', 'main');
  git('config', 'user.name', 'Elesdee E2E');
  git('config', 'user.email', 'e2e@example.com');
  fs.writeFileSync(path.join(folder, 'README.md'), 'elesdee e2e\n');
  git('add', '.');
  git('commit', '-qm', 'Initial commit');
  git('checkout', '-qb', 'feature');
  fs.writeFileSync(path.join(folder, 'feature.txt'), 'feature branch\n');
  git('add', '.');
  git('commit', '-qm', 'Feature commit');
  git('checkout', '-q', 'main');
  return fs.realpathSync(folder);
}

export async function openApp(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.locator('.sidebar')).toBeVisible();
}

export async function bootstrap(
  request: APIRequestContext,
): Promise<Bootstrap> {
  const response = await request.get(`${CORE_URL}/api/bootstrap`);
  expect(response.ok()).toBeTruthy();
  return (await response.json()) as Bootstrap;
}

export async function getSession(
  request: APIRequestContext,
  id: string,
): Promise<Session | undefined> {
  return (await bootstrap(request)).sessions.find(
    (session) => session.id === id,
  );
}

export async function createProject(
  request: APIRequestContext,
  name: string,
  folder: string,
): Promise<Project> {
  const response = await request.post(`${CORE_URL}/api/projects`, {
    data: { name, folder },
  });
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as Project;
}

export interface E2eAccount {
  id: string;
  name: string;
  client: string;
}

export async function createAccount(
  request: APIRequestContext,
  input: {
    name: string;
    providerId?: string;
    auth?: 'subscription' | 'key' | 'local';
    key?: string;
    server?: string;
    model?: string;
    apiStyle?: 'openai' | 'anthropic';
    client?: string;
  },
): Promise<E2eAccount> {
  const response = await request.post(`${CORE_URL}/api/accounts`, {
    data: {
      providerId: 'anthropic',
      auth: 'key',
      ...input,
    },
  });
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as E2eAccount;
}

export async function setUsage(
  request: APIRequestContext,
  accountId: string,
  rows: Array<{ label: string; used?: number; value?: string; min?: string }>,
): Promise<void> {
  const response = await request.post(
    `${CORE_URL}/api/accounts/${encodeURIComponent(accountId)}/usage`,
    { data: { rows } },
  );
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
}

export async function createTerminalSession(
  request: APIRequestContext,
  input: { projectId: string; name: string; folder: string },
): Promise<Session> {
  const response = await request.post(`${CORE_URL}/api/sessions`, {
    data: {
      projectId: input.projectId,
      kind: 'terminal',
      name: input.name,
      folder: input.folder,
    },
  });
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as Session;
}

export async function createAgentSession(
  request: APIRequestContext,
  input: {
    projectId: string;
    name?: string;
    folder: string;
    accountId?: string;
    allowedAccountIds?: string[];
    sourceBranch?: string;
    firstPrompt?: string;
  },
): Promise<Session> {
  const response = await request.post(`${CORE_URL}/api/sessions`, {
    data: {
      projectId: input.projectId,
      kind: 'agent',
      accountId: input.accountId ?? 'max',
      allowedAccountIds: input.allowedAccountIds,
      name: input.name,
      folder: input.folder,
      sourceBranch: input.sourceBranch,
      firstPrompt: input.firstPrompt,
    },
  });
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as Session;
}

export async function registerDemoServer(
  request: APIRequestContext,
  sessionId: string,
  name: string,
  port: number,
): Promise<void> {
  const response = await request.post(
    `${CORE_URL}/internal/mcp/${sessionId}`,
    {
      data: {
        jsonrpc: '2.0',
        id: 1,
        method: 'tools/call',
        params: {
          name: 'register_demo_server',
          arguments: { name, port },
        },
      },
    },
  );
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  const body = (await response.json()) as {
    result?: { isError?: boolean; content?: { text?: string }[] };
  };
  if (body.result?.isError) {
    throw new Error(body.result.content?.[0]?.text ?? 'The demo server was refused.');
  }
}

export async function closeSession(
  request: APIRequestContext,
  id: string,
): Promise<Session> {  const response = await request.post(
    `${CORE_URL}/api/sessions/${id}/close`,
    { data: {} },
  );
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as Session;
}

export async function deleteSession(
  request: APIRequestContext,
  id: string,
): Promise<void> {
  const response = await request.delete(`${CORE_URL}/api/sessions/${id}`);
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
}

export async function claudeInstalled(
  request: APIRequestContext,
): Promise<boolean> {
  const response = await request.get(`${CORE_URL}/api/health`);
  expect(response.ok()).toBeTruthy();
  const health = (await response.json()) as Health;
  return health.agents.some(
    (agent) => agent.name === 'Claude Code' && agent.installed,
  );
}

export async function readClaudeHookUrl(sessionId: string): Promise<string> {
  const settingsPath = path.join(
    dataDir(),
    'claude-settings',
    `${sessionId}.json`,
  );

  await expect
    .poll(() => fs.existsSync(settingsPath), { timeout: 15_000 })
    .toBe(true);

  const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8')) as {
    hooks: {
      PreToolUse: { hooks: { url: string }[] }[];
    };
  };

  return settings.hooks.PreToolUse[0].hooks[0].url;
}

export async function readClaudeStatusLineUrl(
  sessionId: string,
): Promise<string> {
  const hookUrl = await readClaudeHookUrl(sessionId);
  return hookUrl.replace(
    '/internal/hooks/claude/',
    '/internal/statusline/claude/',
  );
}

export async function waitForPreviewChange(
  request: APIRequestContext,
  id: string,
  previous: string,
): Promise<void> {
  await expect
    .poll(async () => (await getSession(request, id))?.preview, {
      timeout: 20_000,
    })
    .not.toBe(previous);
}

export function sessionRow(page: Page, name: string) {
  return page.getByRole('button', { name: new RegExp(name) });
}

export async function openHeaderMenu(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Session actions' }).click();
}
