import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { expect, test, type APIRequestContext } from '@playwright/test';
import { getRunDir } from './env';
import {
  CORE_URL,
  closeSession,
  createAccount,
  createAgentSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  getSession,
  openApp,
  registerDemoServer,
  sessionRow,
  setUsage,
  uniqueName,
} from './helpers';

const TAILNET_NAME = 'elesdee-e2e.tailnet.ts.net';
const PUBLIC_ADDRESS = `https://${TAILNET_NAME}/`;

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
    const tailnet = `https://${TAILNET_NAME}:${demo.port}`;
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
    await expect(page.locator('.app-frame')).toHaveAttribute('inert', '');
    expect(
      await dialog.evaluate((element) => Boolean(element.closest('.dock'))),
    ).toBe(false);
    await expect(dialog).toContainText(
      'Anyone on the internet can open Funnel at its public address until you stop it. The server stays local if Tailscale is not available.',
    );

    const beforeConfirm = await demoServerEntry(request, 'Funnel');
    expect(beforeConfirm?.funnelAddress).toBeUndefined();

    await dialog.getByRole('button', { name: 'Make public' }).click();

    const publicAddress = PUBLIC_ADDRESS;
    const publicLink = filesPanel.getByRole('link', {
      name: `Open public address ${publicAddress} for Funnel`,
    });
    await expect(publicLink).toHaveAttribute('href', publicAddress);
    await expect(page.locator('.app-frame')).not.toHaveAttribute('inert');
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

test('keeps one public server and turns the first off when a second becomes public', async ({
  page,
  request,
}) => {
  test.skip(
    !fs.existsSync(path.join(getRunDir(), 'tailscale-bin', 'tailscale')),
    'needs the fake Tailscale CLI',
  );
  const label = uniqueName('demo-one-public');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('One public session');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });
  const firstDemo = await startDemoServer();
  const secondDemo = await startDemoServer();
  const firstName = uniqueName('First server');
  const secondName = uniqueName('Second server');
  await registerDemoServer(request, session.id, firstName, firstDemo.port);
  await registerDemoServer(request, session.id, secondName, secondDemo.port);
  const callsBefore = tailscaleCalls();
  const publicAddress = PUBLIC_ADDRESS;

  try {
    await openApp(page);
    await sessionRow(page, name).first().click();
    await page.getByRole('button', { name: 'Session panel' }).click();
    const filesPanel = page.getByRole('tabpanel', { name: 'Files' });
    const dialog = page.getByRole('dialog', {
      name: 'Make this server public?',
    });

    await filesPanel
      .getByRole('button', {
        name: `Make ${firstName} public on the internet`,
      })
      .click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Make public' }).click();

    const firstPublic = filesPanel.getByRole('link', {
      name: `Open public address ${publicAddress} for ${firstName}`,
    });
    await expect(firstPublic).toHaveAttribute('href', publicAddress);
    expect((await demoServerEntry(request, firstName))?.funnelAddress).toBe(
      publicAddress,
    );

    await filesPanel
      .getByRole('button', {
        name: `Make ${secondName} public on the internet`,
      })
      .click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Make public' }).click();

    await expect(
      filesPanel.getByRole('link', {
        name: `Open public address ${publicAddress} for ${secondName}`,
      }),
    ).toHaveAttribute('href', publicAddress);
    await expect(
      filesPanel.getByRole('link', {
        name: `Open public address ${publicAddress} for ${firstName}`,
      }),
    ).toHaveCount(0);
    await expect(
      filesPanel.getByRole('button', {
        name: `Make ${firstName} public on the internet`,
      }),
    ).toBeVisible();

    const response = await request.get(`${CORE_URL}/api/demo-servers`);
    expect(response.ok()).toBeTruthy();
    const ours = (
      (await response.json()) as { name: string; funnelAddress?: string }[]
    ).filter((entry) => entry.name === firstName || entry.name === secondName);
    expect(ours).toHaveLength(2);
    expect(
      ours.find((entry) => entry.name === firstName)?.funnelAddress,
    ).toBeUndefined();
    const publicOurs = ours.filter((entry) => entry.funnelAddress);
    expect(publicOurs).toHaveLength(1);
    expect(publicOurs[0]?.name).toBe(secondName);
    expect(publicOurs[0]?.funnelAddress).toBe(publicAddress);

    await expect
      .poll(() => tailscaleCalls().slice(callsBefore.length))
      .toEqual([
        `funnel --bg ${firstDemo.port}`,
        `funnel --https=443 localhost:${firstDemo.port} off`,
        `funnel --bg ${secondDemo.port}`,
      ]);
  } finally {
    await firstDemo.close();
    await secondDemo.close();
    await closeSession(request, session.id);
  }
});

test('turns a public demo server off when the session pauses', async ({
  request,
}) => {
  test.skip(
    !fs.existsSync(path.join(getRunDir(), 'tailscale-bin', 'tailscale')),
    'needs the fake Tailscale CLI',
  );
  const label = uniqueName('demo-pause');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const account = await createAccount(request, {
    name: uniqueName('Pause provider'),
    key: 'sk-demo-pause-0123456789',
  });
  const name = uniqueName('Pause session');
  const session = await createAgentSession(request, {
    projectId: project.id,
    name,
    folder: repo,
    configId: account.configId,
  });
  const demo = await startDemoServer();
  const serverName = uniqueName('Pause server');
  await registerDemoServer(request, session.id, serverName, demo.port);
  const callsBefore = tailscaleCalls();
  const publicAddress = PUBLIC_ADDRESS;

  try {
    const entry = await demoServerEntry(request, serverName);
    if (!entry) {
      throw new Error('The demo server was not registered.');
    }
    const enabled = await request.post(
      `${CORE_URL}/api/sessions/${session.id}/demo-servers/${entry.id}/funnel`,
      { data: { confirmed: true } },
    );
    expect(enabled.ok()).toBeTruthy();
    expect((await demoServerEntry(request, serverName))?.funnelAddress).toBe(
      publicAddress,
    );

    await setUsage(request, account.id, [{ label: 'Weekly', used: 95 }]);
    await expect
      .poll(async () => (await getSession(request, session.id))?.workState, {
        timeout: 30_000,
      })
      .toBe('paused');

    const paused = await getSession(request, session.id);
    expect(paused?.demoServers?.length).toBeGreaterThan(0);
    for (const server of paused?.demoServers ?? []) {
      expect(server.state).toBe('stopped');
      expect(server.funnelAddress).toBeUndefined();
    }
    expect(await demoServerEntry(request, serverName)).toBeUndefined();
    expect(tailscaleCalls().slice(callsBefore.length)).toContain(
      `funnel --https=443 localhost:${demo.port} off`,
    );
  } finally {
    await demo.close();
    await closeSession(request, session.id).catch(() => undefined);
  }
});
