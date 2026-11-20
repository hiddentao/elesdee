import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
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
