import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { getRunDir } from './env';
import { openApp } from './helpers';

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
}) => {
  const folder = fs.mkdtempSync(
    path.join(getRunDir(), 'diagnostics-export-'),
  );

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
    const bundle = JSON.parse(fs.readFileSync(body.path, 'utf8')) as {
      app: { name: string; version: string };
      system: { os: string; arch: string; cpus: number };
      counts: { projects: number; sessions: number; activeSessions: number };
      settings: Record<string, unknown>;
      logs: string[];
    };
    expect(bundle.app.name).toBe('elesdee');
    expect(bundle.app.version).not.toBe('');
    expect(bundle.system.os).not.toBe('');
    expect(bundle.system.arch).not.toBe('');
    expect(bundle.system.cpus).toBeGreaterThan(0);
    expect(bundle.counts.projects).toBeGreaterThanOrEqual(0);
    expect(bundle.counts.sessions).toBeGreaterThanOrEqual(0);
    expect(bundle.counts.activeSessions).toBeGreaterThanOrEqual(0);
    expect(typeof bundle.settings).toBe('object');
    expect(Array.isArray(bundle.logs)).toBe(true);
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
});
