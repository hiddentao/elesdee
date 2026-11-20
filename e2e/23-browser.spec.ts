import http from 'node:http';
import type { AddressInfo } from 'node:net';
import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from '@playwright/test';
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
      `<html><head><title>${title}</title><style>` +
        `body{margin:0;font-family:sans-serif}` +
        `#name{position:absolute;left:0;top:25%;width:100%;height:10%;font-size:20px}` +
        `#save{position:absolute;left:0;top:0;width:100%;height:20%;font-size:20px}` +
        `h1{position:absolute;left:0;top:45%}` +
        `a{position:absolute;left:0;top:60%}` +
        `#out{position:absolute;left:0;top:75%}` +
        `</style></head><body>` +
        `<h1>${title}</h1><a href="/two">Go to the second page</a>` +
        `<input id="name" aria-label="Name">` +
        `<button id="save" onclick="document.getElementById('out').textContent=document.getElementById('name').value">Save</button>` +
        `<p id="out"></p>` +
        `<script>` +
        `console.log('page ready', 1 + 1);` +
        `const input = document.getElementById('name');` +
        `input.addEventListener('select', () => {` +
        `document.getElementById('out').textContent = 'selected ' + input.value.slice(input.selectionStart, input.selectionEnd);` +
        `});` +
        `document.addEventListener('copy', (event) => {` +
        `const selected = input.value.slice(input.selectionStart, input.selectionEnd);` +
        `const copied = event.clipboardData ? event.clipboardData.getData('text/plain') : '';` +
        `document.getElementById('out').textContent = 'copied ' + (copied || selected);` +
        `});` +
        `</script>` +
        `</body></html>`,
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

async function tool(
  request: APIRequestContext,
  sessionId: string,
  name: string,
  args: Record<string, unknown>,
) {
  const response = await request.post(`${CORE_URL}/internal/mcp/${sessionId}`, {
    data: {
      jsonrpc: '2.0',
      id: 1,
      method: 'tools/call',
      params: { name, arguments: args },
    },
  });
  expect(response.ok()).toBeTruthy();
  return (await response.json()) as {
    result: { isError: boolean; content: { text: string }[] };
  };
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

    expect(
      (
        await tool(request, first.id, 'browser_type', {
          selector: '#name',
          text: 'Ada',
        })
      ).result.isError,
    ).toBe(false);
    expect(
      (await tool(request, first.id, 'browser_click', { selector: '#save' }))
        .result.isError,
    ).toBe(false);
    expect(
      (
        await tool(request, first.id, 'browser_wait', {
          selector: '#out',
          timeoutSeconds: 2,
        })
      ).result.isError,
    ).toBe(false);
    const read = await tool(request, first.id, 'browser_read', {});
    expect(read.result.content[0].text).toContain('Ada');

    const consoleLog = await tool(request, first.id, 'browser_console', {});
    expect(consoleLog.result.isError).toBe(false);
    expect(consoleLog.result.content[0].text).toContain('page ready 2');

    const networkLog = await tool(request, first.id, 'browser_network', {});
    expect(networkLog.result.isError).toBe(false);
    expect(networkLog.result.content[0].text).toContain(`${baseUrl}/one`);
    expect(networkLog.result.content[0].text).toContain('returned 200');

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

test('keeps arrow keys on the browser close and new-tab buttons', async ({
  page,
  request,
}) => {
  const label = uniqueName('browser-arrows');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('Browser arrows'),
    folder,
  });

  try {
    await openApp(page);
    const address = await openBrowserPanel(page, session.name);
    await address.fill(`${baseUrl}/one`);
    await address.press('Enter');
    await expect(page.getByRole('tab', { name: 'First page' })).toBeVisible();

    const close = page.getByRole('button', { name: 'Close tab First page' });
    await close.focus();
    await page.keyboard.press('ArrowRight');
    await expect(close).toBeFocused();
    await expect(page.getByRole('tab', { name: 'First page' })).toHaveAttribute(
      'aria-selected',
      'true',
    );

    const newTab = page.getByRole('button', { name: 'New tab' });
    await newTab.focus();
    await page.keyboard.press('ArrowLeft');
    await expect(newTab).toBeFocused();

    await newTab.click();
    const firstTab = page.getByRole('tab', { name: 'First page' });
    const secondTab = page.getByRole('tab', { name: 'Untitled page' });
    await expect(secondTab).toBeVisible();
    await firstTab.click();
    await expect(firstTab).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('ArrowRight');
    await expect(secondTab).toBeFocused();
    expect(
      await secondTab.evaluate((element) => {
        const style = getComputedStyle(element);
        return { offset: style.outlineOffset, shadow: style.boxShadow };
      }),
    ).toEqual({ offset: '-2px', shadow: expect.stringContaining('inset') });
  } finally {
    await closeSession(request, session.id);
  }
});

test('clicks and types in the live view', async ({ page, request }) => {
  const label = uniqueName('browser-live');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('Live browser'),
    folder,
  });

  try {
    await openApp(page);
    const address = await openBrowserPanel(page, session.name);
    await address.fill(`${baseUrl}/one`);
    await address.press('Enter');

    const live = page.getByRole('application', {
      name: 'Live browser page',
    });
    await expect(live).toBeVisible();
    const box = await live.boundingBox();
    expect(box).not.toBeNull();
    const width = box?.width ?? 0;
    const height = box?.height ?? 0;
    expect(width).toBeGreaterThan(0);
    expect(height).toBeGreaterThan(0);

    // The input sits between a quarter and a third of the page height.
    await live.click({ position: { x: width * 0.5, y: height * 0.3 } });
    await page.keyboard.type('Ada Lovelace', { delay: 20 });

    // The Save button fills the top fifth of the page.
    await live.click({ position: { x: width * 0.5, y: height * 0.1 } });

    await expect
      .poll(
        async () =>
          (await tool(request, session.id, 'browser_read', {})).result
            .content[0].text,
        { timeout: 5000 },
      )
      .toContain('Ada Lovelace');
  } finally {
    await closeSession(request, session.id);
  }
});

test('uses select-all and copy in the live view', async ({ page, request }) => {
  const label = uniqueName('browser-chords');
  const folder = createProjectFolder(label);
  const project = await createProject(request, label, folder);
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('Chord browser'),
    folder,
  });
  const command = process.platform === 'darwin' ? 'Meta' : 'Control';

  try {
    await openApp(page);
    const address = await openBrowserPanel(page, session.name);
    await address.fill(`${baseUrl}/one`);
    await address.press('Enter');

    const live = page.getByRole('application', {
      name: 'Live browser page',
    });
    await expect(live).toBeVisible();
    const box = await live.boundingBox();
    expect(box).not.toBeNull();
    const width = box?.width ?? 0;
    const height = box?.height ?? 0;

    await live.click({ position: { x: width * 0.5, y: height * 0.3 } });
    await page.keyboard.type('Ada Lovelace', { delay: 20 });

    await page.keyboard.press(`${command}+A`);
    await expect
      .poll(
        async () =>
          (await tool(request, session.id, 'browser_read', {})).result
            .content[0].text,
        { timeout: 5000 },
      )
      .toContain('selected Ada Lovelace');

    await page.keyboard.press(`${command}+C`);
    await expect
      .poll(
        async () =>
          (await tool(request, session.id, 'browser_read', {})).result
            .content[0].text,
        { timeout: 5000 },
      )
      .toContain('copied Ada Lovelace');
  } finally {
    await closeSession(request, session.id);
  }
});
