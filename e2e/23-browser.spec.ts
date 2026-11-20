import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test, type Page } from '@playwright/test';
import {
  CORE_URL,
  closeSession,
  createProject,
  createProjectFolder,
  createTerminalSession,
  openApp,
  sessionRow,
  uniqueName,
} from './helpers';

let server: http.Server;
let baseUrl = '';

test.beforeAll(async () => {
  server = http.createServer((request, response) => {
    const second = request.url?.includes('two') === true;
    const title = second ? 'Second page' : 'First page';
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    response.end(
      `<html><head><title>${title}</title></head><body>` +
        `<h1>${title}</h1><a href="/two">Go to the second page</a>` +
        `<input id="name" aria-label="Name">` +
        `<button id="save" onclick="document.getElementById('out').textContent=document.getElementById('name').value">Save</button>` +
        `<p id="out"></p></body></html>`,
    );
  });
  await new Promise<void>((resolve) => {
    server.listen(0, '127.0.0.1', resolve);
  });
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

test.afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

async function openBrowserPanel(page: Page, name: string) {
  await sessionRow(page, name).first().click();
  await page.getByRole('button', { name: 'Session panel' }).click();
  await page.getByRole('tab', { name: 'Browser', exact: true }).click();
  return page.getByLabel('Address');
}

test('opens a page and keeps separate tabs for two sessions', async ({
  page,
  request,
}) => {
  const label = uniqueName('browser-panel');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const first = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('First browser'),
    folder,
  });
  const second = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('Second browser'),
    folder,
  });

  try {
    await openApp(page);

    const firstAddress = await openBrowserPanel(page, first.name);
    await firstAddress.fill(`${baseUrl}/one`);
    await firstAddress.press('Enter');
    await expect(
      page.getByRole('tab', { name: 'First page' }),
    ).toBeVisible();
    await expect(
      page.getByRole('img', { name: 'Live view of First page' }),
    ).toBeVisible();

    const tool = async (name: string, args: Record<string, unknown>) => {
      const response = await request.post(
        `${CORE_URL}/internal/mcp/${first.id}`,
        {
          data: {
            jsonrpc: '2.0',
            id: 1,
            method: 'tools/call',
            params: { name, arguments: args },
          },
        },
      );
      expect(response.ok()).toBeTruthy();
      return (await response.json()) as {
        result: { isError: boolean; content: { text: string }[] };
      };
    };
    expect(
      (await tool('browser_type', { selector: '#name', text: 'Ada' })).result
        .isError,
    ).toBe(false);
    expect(
      (await tool('browser_click', { selector: '#save' })).result.isError,
    ).toBe(false);
    expect(
      (await tool('browser_wait', { selector: '#out', timeoutSeconds: 2 }))
        .result.isError,
    ).toBe(false);
    const read = await tool('browser_read', {});
    expect(read.result.content[0].text).toContain('Ada');

    const secondAddress = await openBrowserPanel(page, second.name);
    await secondAddress.fill(`${baseUrl}/two`);
    await secondAddress.press('Enter');
    await expect(
      page.getByRole('tab', { name: 'Second page' }),
    ).toBeVisible();
    await expect(page.getByRole('tab', { name: 'First page' })).toHaveCount(0);
    await expect(
      page.getByRole('img', { name: 'Live view of Second page' }),
    ).toBeVisible();

    await sessionRow(page, first.name).first().click();
    await expect(page.getByRole('tab', { name: 'First page' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Second page' })).toHaveCount(0);
    await expect(page.getByLabel('Address')).toHaveValue(`${baseUrl}/one`);

    const address = page.getByLabel('Address');
    await address.fill(`${baseUrl}/two`);
    await address.press('Enter');
    await expect(page.getByRole('tab', { name: 'Second page' })).toBeVisible();
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByRole('tab', { name: 'First page' })).toBeVisible();
    await expect(page.getByLabel('Address')).toHaveValue(`${baseUrl}/one`);

    for (const colorScheme of ['dark', 'light'] as const) {
      await page.emulateMedia({ colorScheme });
      for (const width of [1440, 1024]) {
        await page.setViewportSize({ width, height: 900 });
        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth > window.innerWidth,
        );
        expect(overflow, `${colorScheme} at ${width} scrolls sideways`).toBe(false);
        await expect(
          page.getByRole('tab', { name: 'First page' }),
        ).toBeVisible();
      }
    }
  } finally {
    await closeSession(request, first.id);
    await closeSession(request, second.id);
  }
});
