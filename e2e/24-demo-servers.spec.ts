import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { expect, test, type APIRequestContext } from '@playwright/test';
import { getRunDir } from './env';
import {
  CORE_URL,
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  openApp,
  registerDemoServer,
  sessionRow,
  uniqueName,
} from './helpers';

async function startDemoServer(): Promise<{
  port: number;
  close: () => Promise<void>;
}> {
  const server = http.createServer((_request, response) => {
    response.writeHead(200, { 'Content-Type': 'text/plain' });
    response.end('hello from the demo server\n');
  });
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve);
  });
  const port = (server.address() as AddressInfo).port;
  return {
    port,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      }),
  };
}

async function demoServerEntry(
  request: APIRequestContext,
  name: string,
): Promise<
  | { id: string; state: 'active' | 'stopped'; funnelAddress?: string }
  | undefined
> {
  const response = await request.get(`${CORE_URL}/api/demo-servers`);
  expect(response.ok()).toBeTruthy();
  const entries = (await response.json()) as {
    id: string;
    name: string;
    state: 'active' | 'stopped';
    funnelAddress?: string;
  }[];
  return entries.find((entry) => entry.name === name);
}

function tailscaleCalls(): string[] {
  const file = path.join(getRunDir(), 'tailscale-calls.txt');
  if (!fs.existsSync(file)) {
    return [];
  }
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .filter(Boolean);
}

test('shows a registered demo server and stops it from the session panel', async ({
  page,
  request,
}) => {
  const label = uniqueName('demo-servers');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('Demo servers');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });
  const demo = await startDemoServer();
  await registerDemoServer(request, session.id, 'Preview', demo.port);

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session panel' }).click();
    await expect(
      page.getByRole('tab', { name: 'Files', exact: true }),
    ).toBeVisible();
    const filesPanel = page.getByRole('tabpanel', { name: 'Files' });

    await expect(
      filesPanel.getByText('Demo servers', { exact: true }),
    ).toBeVisible();
    await expect(filesPanel.getByText('Preview', { exact: true })).toBeVisible();
    await expect(filesPanel.getByText('Active', { exact: true })).toBeVisible();

    const address = `http://localhost:${demo.port}`;
    const open = filesPanel.getByRole('link', {
      name: `Open ${address} for Preview`,
    });
    await expect(open).toHaveAttribute('href', address);

    await filesPanel.getByRole('button', { name: 'Stop Preview' }).click();

    await expect(filesPanel.getByText('Stopped', { exact: true })).toBeVisible();
    await expect(open).toHaveCount(0);
    await expect(
      filesPanel.getByRole('button', { name: 'Stop Preview' }),
    ).toHaveCount(0);

    const bootstrap = await request.get(`${CORE_URL}/api/demo-servers`);
    expect(bootstrap.ok()).toBeTruthy();
    expect(await bootstrap.json()).toEqual([]);
  } finally {
    await demo.close();
    await closeSession(request, session.id);
  }
});

test('shows the tailnet address from the fake Tailscale CLI', async ({
  page,
  request,
}) => {
  test.skip(
    !fs.existsSync(path.join(getRunDir(), 'tailscale-bin', 'tailscale')),
    'needs the fake Tailscale CLI',
  );
  const label = uniqueName('demo-tailnet');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('Demo tailnet');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });
  const demo = await startDemoServer();
  await registerDemoServer(request, session.id, 'Tailnet', demo.port);

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session panel' }).click();
    const filesPanel = page.getByRole('tabpanel', { name: 'Files' });

    const local = `http://localhost:${demo.port}`;
    const tailnet = `https://elesdee-e2e.tailnet.ts.net:${demo.port}`;
    await expect(
      filesPanel.getByRole('link', { name: `Open ${local} for Tailnet` }),
    ).toHaveAttribute('href', local);
    await expect(
      filesPanel.getByRole('link', { name: `Open ${tailnet} for Tailnet` }),
    ).toHaveAttribute('href', tailnet);
  } finally {
    await demo.close();
    await closeSession(request, session.id);
  }
});

test('makes a demo server public after confirmation and stops the public address', async ({
  page,
  request,
}) => {
  test.skip(
    !fs.existsSync(path.join(getRunDir(), 'tailscale-bin', 'tailscale')),
    'needs the fake Tailscale CLI',
  );
  const label = uniqueName('demo-funnel');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('Demo funnel');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });
  const demo = await startDemoServer();
  await registerDemoServer(request, session.id, 'Funnel', demo.port);

  const callsBefore = tailscaleCalls();

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session panel' }).click();
    const filesPanel = page.getByRole('tabpanel', { name: 'Files' });

    await filesPanel
      .getByRole('button', { name: 'Make Funnel public on the internet' })
      .click();

    const dialog = page.getByRole('dialog', {
      name: 'Make this server public?',
    });
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(
      'Anyone on the internet can open Funnel at its public address until you stop it. The server stays local if Tailscale is not available.',
    );

    const beforeConfirm = await demoServerEntry(request, 'Funnel');
    expect(beforeConfirm?.funnelAddress).toBeUndefined();

    await dialog.getByRole('button', { name: 'Make public' }).click();

    const publicAddress = 'https://elesdee-e2e.tailnet.ts.net/';
    const publicLink = filesPanel.getByRole('link', {
      name: `Open public address ${publicAddress} for Funnel`,
    });
    await expect(publicLink).toHaveAttribute('href', publicAddress);
    await expect(filesPanel.getByText('Public', { exact: true })).toBeVisible();
    await expect(
      filesPanel.getByRole('button', {
        name: 'Stop the public address for Funnel',
      }),
    ).toBeVisible();

    const entry = await demoServerEntry(request, 'Funnel');
    expect(entry?.funnelAddress).toBe(publicAddress);
    expect(entry?.id).toBeTruthy();

    const enableCall = `funnel --bg ${demo.port}`;
    await expect
      .poll(() => tailscaleCalls().slice(callsBefore.length))
      .toContain(enableCall);

    await filesPanel
      .getByRole('button', { name: 'Stop the public address for Funnel' })
      .click();

    await expect(publicLink).toHaveCount(0);
    await expect(filesPanel.getByText(publicAddress)).toHaveCount(0);
    await expect(
      filesPanel.getByRole('button', {
        name: 'Make Funnel public on the internet',
      }),
    ).toBeVisible();

    const afterStop = await demoServerEntry(request, 'Funnel');
    expect(afterStop?.funnelAddress).toBeUndefined();
    const disableCall = `funnel --https=443 localhost:${demo.port} off`;
    await expect
      .poll(() => tailscaleCalls().slice(callsBefore.length))
      .toContain(disableCall);

    // Register the stopped port again, make it public, then close the session.
    // Close must turn Funnel off without waiting for the agent or the Git work.
    await registerDemoServer(request, session.id, 'Funnel', demo.port);
    const repeat = await request.post(
      `${CORE_URL}/api/sessions/${session.id}/demo-servers/${entry?.id}/funnel`,
      { data: { confirmed: true } },
    );
    expect(repeat.ok()).toBeTruthy();
    const repeated = await demoServerEntry(request, 'Funnel');
    expect(repeated?.funnelAddress).toBe(publicAddress);

    const closeResponse = await request.post(
      `${CORE_URL}/api/sessions/${session.id}/close`,
      { data: {} },
    );
    expect(closeResponse.ok()).toBeTruthy();
    const closed = (await closeResponse.json()) as {
      status: string;
      demoServers?: { funnelAddress?: string }[];
    };
    expect(closed.status).toBe('closed');
    expect(
      closed.demoServers?.every((server) => server.funnelAddress === undefined),
    ).toBeTruthy();

    const disableCalls = tailscaleCalls()
      .slice(callsBefore.length)
      .filter((call) => call === disableCall);
    expect(disableCalls).toHaveLength(2);
  } finally {
    await demo.close();
    await request.post(`${CORE_URL}/api/sessions/${session.id}/close`, {
      data: {},
    });
  }
});
