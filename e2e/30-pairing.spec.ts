import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { expect, test, type Page } from '@playwright/test';
import { CORE_URL, openApp } from './helpers';

interface FakePhone {
  child: ChildProcessWithoutNullStreams;
  code: Promise<string>;
  exited: Promise<number>;
}

function startFakePhone(ticket: string, name: string): FakePhone {
  const child = spawn(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      'core/Cargo.toml',
      '--example',
      'fake_phone',
      '--',
      ticket,
      name,
    ],
    {
      cwd: process.cwd(),
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );

  let output = '';
  let found = false;
  let resolveCode: (code: string) => void = () => undefined;
  let rejectCode: (error: Error) => void = () => undefined;
  const code = new Promise<string>((resolve, reject) => {
    resolveCode = resolve;
    rejectCode = reject;
  });

  child.stdout.on('data', (chunk: Buffer) => {
    output += chunk.toString();
    const match = output.match(/CODE (\d{6})/);
    if (match && !found) {
      found = true;
      resolveCode(match[1]);
    }
  });
  child.stderr.on('data', (chunk: Buffer) => {
    output += chunk.toString();
  });
  child.on('error', (error) => {
    if (!found) {
      rejectCode(error);
    }
  });

  const exited = new Promise<number>((resolve) => {
    child.on('exit', (exitCode) => {
      if (!found) {
        rejectCode(new Error(`The fake phone stopped: ${output}`));
      }
      resolve(exitCode ?? -1);
    });
  });

  return { child, code, exited };
}

async function openSettings(page: Page) {
  await openApp(page);
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'Devices' }).click();
  return settings;
}

test('pairs a phone that reaches the core and shows the new device', async ({
  page,
  request,
}) => {
  const settings = await openSettings(page);

  const started = page.waitForResponse(
    (response) =>
      response.request().method() === 'POST' &&
      response.url().endsWith('/api/pairing'),
  );
  await settings.getByRole('button', { name: 'Pair a phone' }).click();
  const response = await started;
  expect(response.ok()).toBeTruthy();
  const created = (await response.json()) as {
    relayReady: boolean;
    ticket?: string;
  };
  expect(created.ticket).toBeTruthy();

  const dialog = page.getByRole('dialog', { name: 'Pair a phone' });
  await expect(
    dialog.getByRole('img', { name: 'Pairing QR code' }),
  ).toBeVisible();

  const phone = startFakePhone(created.ticket as string, 'E2E Phone');
  try {
    const code = await phone.code;
    expect(code).toMatch(/^\d{6}$/);

    await expect(
      dialog.getByText('The phone E2E Phone wants to pair.'),
    ).toBeVisible();
    await expect(dialog.getByText(code, { exact: true })).toBeVisible();

    await dialog.getByRole('button', { name: 'Approve' }).click();

    await expect(
      page.getByRole('dialog', { name: 'Pair a phone' }),
    ).toHaveCount(0);
    await expect(settings.getByText('E2E Phone')).toBeVisible();
    expect(await phone.exited).toBe(0);

    const devices = await request.get(`${CORE_URL}/api/devices`);
    const list = (await devices.json()) as {
      endpointId: string;
      name: string;
    }[];
    const paired = list.find((device) => device.name === 'E2E Phone');
    expect(paired).toBeTruthy();
    await request.delete(
      `${CORE_URL}/api/devices/${encodeURIComponent(paired?.endpointId ?? '')}`,
    );
  } finally {
    phone.child.kill();
  }
});

test('fits the pairing dialog at both desktop widths and themes', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(message.text());
    }
  });
  page.on('pageerror', (error) => errors.push(error.message));

  const settings = await openSettings(page);
  const dialog = page.getByRole('dialog', { name: 'Pair a phone' });
  await settings.getByRole('button', { name: 'Pair a phone' }).click();
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByRole('img', { name: 'Pairing QR code' }),
  ).toBeVisible();

  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme });
    for (const width of [1440, 1024]) {
      await page.setViewportSize({ width, height: 900 });
      expect(
        await page.evaluate(
          () =>
            document.documentElement.scrollWidth -
            document.documentElement.clientWidth,
        ),
      ).toBeLessThanOrEqual(0);
    }
  }

  await dialog.getByRole('button', { name: 'Cancel pairing' }).click();
  await expect(dialog).toHaveCount(0);
  expect(errors).toEqual([]);
});

test('shows the ticket when the QR image fails and cancels pairing', async ({
  page,
  request,
}) => {
  await page.route('**/api/pairing/qr*', (route) => route.abort());
  const settings = await openSettings(page);

  const started = page.waitForResponse(
    (response) =>
      response.request().method() === 'POST' &&
      response.url().endsWith('/api/pairing'),
  );
  await settings.getByRole('button', { name: 'Pair a phone' }).click();
  const created = (await (await started).json()) as { ticket?: string };
  expect(created.ticket).toBeTruthy();

  const dialog = page.getByRole('dialog', { name: 'Pair a phone' });
  await expect(dialog.getByLabel('Pairing ticket')).toHaveValue(
    created.ticket as string,
  );
  await expect(
    dialog.getByText(/paste this ticket in the phone app/),
  ).toBeVisible();

  await dialog.getByRole('button', { name: 'Cancel pairing' }).click();
  await expect(
    page.getByRole('dialog', { name: 'Pair a phone' }),
  ).toHaveCount(0);

  await expect
    .poll(async () => {
      const status = await request.get(`${CORE_URL}/api/pairing`);
      const body = (await status.json()) as { state: string };
      return body.state;
    })
    .toBe('cancelled');
});
