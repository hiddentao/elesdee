import fs from 'node:fs';
import path from 'node:path';
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import { dataDir, projectsDir } from './env';

export const CORE_URL = 'http://127.0.0.1:4399';

export interface Project {
  id: string;
  name: string;
  folder: string;
}

export interface Session {
  id: string;
  name: string;
  projectId: string;
  kind: 'agent' | 'terminal';
  status: 'active' | 'closed';
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: string | null;
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
  input: { projectId: string; name: string; folder: string },
): Promise<Session> {
  const response = await request.post(`${CORE_URL}/api/sessions`, {
    data: {
      projectId: input.projectId,
      kind: 'agent',
      accountId: 'max',
      name: input.name,
      folder: input.folder,
    },
  });
  if (!response.ok()) {
    throw new Error(`${response.status()}: ${await response.text()}`);
  }
  return (await response.json()) as Session;
}

export async function closeSession(
  request: APIRequestContext,
  id: string,
): Promise<Session> {
  const response = await request.post(
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
