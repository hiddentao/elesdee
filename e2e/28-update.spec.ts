import { expect, test } from '@playwright/test';
import { openApp } from './helpers';

/**
 * Stubs the Tauri bridge in the browser. The real updater can only run in a
 * packaged, signed app, so this test checks the UI and the command wiring.
 */
async function stubDesktopBridge(page: import('@playwright/test').Page) {
  await page.addInitScript(() => {
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
          case 'get_update_state':
            return {
              state: 'ready',
              currentVersion: '0.1.0',
              version: '0.2.0',
              progress: null,
              error: null,
            };
          case 'get_start_at_login':
            return { enabled: false };
          case 'set_keep_awake':
            return false;
          case 'notification_permission_state':
          case 'request_notification_permission':
            return 'granted';
          case 'show_notification':
            return true;
          default:
            return null;
        }
      },
    };
  });
}

test('the About page shows the update state from the desktop shell', async ({
  page,
}) => {
  await stubDesktopBridge(page);
  await openApp(page);

  await page
    .getByRole('button', { name: 'Settings', exact: true })
    .click();
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('button', { name: 'About' }).click();

  await expect(settings.getByText('Version 0.1.0')).toBeVisible();
  await expect(
    settings.getByText('Version 0.2.0 is ready to install.'),
  ).toBeVisible();
  await expect(
    settings.getByRole('button', { name: 'Install and restart' }),
  ).toBeVisible();
});
