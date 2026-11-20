import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { dataDir, getRunDir } from './env';
import { CORE_URL, openApp } from './helpers';

/**
 * Stubs the Tauri bridge so the folder picker answers with a known folder.
 * The real picker needs the packaged desktop shell.
 */
async function stubFolderPicker(page: Page, folder: string): Promise<void> {
  await page.addInitScript((exportFolder) => {
    const scope = window as unknown as Record<string, unknown>;
    let nextId = 1;
    scope.isTauri = true;
    scope.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    scope.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: 'main' },
        currentWebview: { windowLabel: 'main', label: 'main' },
      },
      transformCallback: () => nextId++,
      invoke: async (command: string) => {
        switch (command) {
          case 'plugin:dialog|open':
            return exportFolder;
          case 'plugin:event|listen':
            return 1;
          case 'notification_permission_state':
            return 'granted';
          default:
            return null;
        }
      },
    };
  }, folder);
}

test('exports a redacted diagnostics bundle into the chosen folder', async ({
  page,
  request,
}) => {
  const folder = fs.mkdtempSync(
    path.join(getRunDir(), 'diagnostics-export-'),
  );
  const secret = {
    key: 'sk-live-diagnostics-0123456789',
    output: 'TOP_SECRET_DIAGNOSTICS_OUTPUT',
    ticket: 'ticket-diagnostics-probe',
    note: 'api_key=sk-value-diagnostics-probe',
  };

  try {
    await stubFolderPicker(page, folder);
    await openApp(page);

    await page
      .getByRole('button', { name: 'Settings', exact: true })
      .click();
    const settings = page.getByRole('dialog', { name: 'Settings' });
    await settings.getByRole('button', { name: 'Diagnostics' }).click();

    await expect(
      settings.getByText(/The bundle holds the app version/),
    ).toBeVisible();
    const exportButton = settings.getByRole('button', {
      name: 'Export diagnostics',
    });
    await expect(exportButton).toBeVisible();

    // The core writes its own log beside the data directory. Append one raw
    // record so the export must redact it on read.
    const logPath = path.join(dataDir(), 'logs', 'core.log');
    await expect.poll(() => fs.existsSync(logPath), {
      timeout: 15_000,
    }).toBe(true);
    fs.appendFileSync(
      logPath,
      `${JSON.stringify({
        fields: {
          message: 'diagnostics e2e probe',
          apiKey: secret.key,
          output: secret.output,
          ticket: secret.ticket,
          note: secret.note,
        },
      })}\n`,
    );

    const bootstrapResponse = await request.get(`${CORE_URL}/api/bootstrap`);
    expect(bootstrapResponse.ok()).toBeTruthy();
    const expected = (await bootstrapResponse.json()) as {
      projects: unknown[];
      providers: unknown[];
      configs: unknown[];
      sessions: { status: 'active' | 'closed' }[];
      settings: Record<string, unknown>;
    };
    const devicesResponse = await request.get(`${CORE_URL}/api/devices`);
    expect(devicesResponse.ok()).toBeTruthy();
    const devices = (await devicesResponse.json()) as unknown[];

    const [response] = await Promise.all([
      page.waitForResponse(
        (result) =>
          result.url().endsWith('/api/diagnostics/export') &&
          result.request().method() === 'POST',
      ),
      exportButton.click(),
    ]);
    expect(response.ok()).toBeTruthy();
    const body = (await response.json()) as { path: string };

    expect(path.dirname(body.path)).toBe(folder);
    expect(path.basename(body.path)).toMatch(
      /^elesdee-diagnostics-\d{8}-\d{6}\.json$/,
    );
    await expect(
      settings.getByText(`Saved to ${body.path}`),
    ).toBeVisible();

    await expect.poll(() => fs.existsSync(body.path)).toBe(true);
    const raw = fs.readFileSync(body.path, 'utf8');
    const bundle = JSON.parse(raw) as {
      generatedAt: string;
      app: { name: string; version: string };
      system: { os: string; family: string; arch: string; cpus: number };
      counts: {
        projects: number;
        providers: number;
        configs: number;
        sessions: number;
        activeSessions: number;
        pairedDevices: number;
      };
      settings: Record<string, unknown>;
      logs: string[];
    };
    expect(Number.isNaN(Date.parse(bundle.generatedAt))).toBe(false);
    expect(bundle.app.name).toBe('elesdee');
    expect(bundle.app.version).not.toBe('');
    expect(bundle.system.os).not.toBe('');
    expect(bundle.system.family).not.toBe('');
    expect(bundle.system.arch).not.toBe('');
    expect(bundle.system.cpus).toBeGreaterThan(0);
    expect(bundle.counts).toEqual({
      projects: expected.projects.length,
      providers: expected.providers.length,
      configs: expected.configs.length,
      sessions: expected.sessions.length,
      activeSessions: expected.sessions.filter(
        (session) => session.status === 'active',
      ).length,
      pairedDevices: devices.length,
    });
    expect(bundle.settings).toEqual(expected.settings);

    expect(bundle.logs.length).toBeGreaterThan(0);
    expect(
      bundle.logs.some((line) => line.includes('diagnostics e2e probe')),
    ).toBe(true);
    expect(raw).toContain('[redacted]');
    for (const hidden of Object.values(secret)) {
      expect(raw).not.toContain(hidden);
    }
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
});
