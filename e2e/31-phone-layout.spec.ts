import { expect, test, type Page } from '@playwright/test';
import type { Session } from '../app/src/api/types';

const project = { id: 'phone-layout', name: 'web', folder: '/dev/web' };

const config = {
  id: 'phone-layout-config',
  name: 'Claude Max',
  providerId: 'phone-layout-provider',
  client: 'Claude Code',
  model: null,
  effort: null,
  args: [],
  env: [],
  config: {},
  createdAt: '2026-11-19T10:00:00Z',
  updatedAt: '2026-11-19T10:00:00Z',
};

function phoneSession(overrides: Partial<Session>): Session {
  return {
    id: 'phone-layout-session',
    name: 'Session',
    short: 'session',
    projectId: project.id,
    kind: 'agent',
    status: 'active',
    workState: 'working',
    client: 'Claude Code',
    configId: config.id,
    allowedConfigIds: [config.id, 'phone-layout-second'],
    providerId: config.providerId,
    branch: 'main',
    folder: project.folder,
    preview: 'Ready',
    waitingFor: null,
    pendingPermission: null,
    demoServers: [],
    startedAt: '2026-11-19T10:00:00Z',
    shortcuts: [],
    ...overrides,
  };
}

const working = phoneSession({ id: 'phone-working', name: 'Working session' });
const waiting = phoneSession({
  id: 'phone-waiting',
  name: 'Waiting session',
  workState: 'waiting',
  waitingFor: 'input',
  preview: 'Waiting to push the fix',
});
const paused = phoneSession({
  id: 'phone-paused',
  name: 'Paused session',
  workState: 'paused',
});
const done = phoneSession({
  id: 'phone-done',
  name: 'Done session',
  workState: 'done',
});
const terminal = phoneSession({
  id: 'phone-terminal',
  name: 'Terminal session',
  kind: 'terminal',
  client: null,
  workState: null,
  configId: null,
  providerId: null,
  allowedConfigIds: [],
});

function payload(overrides: Record<string, unknown> = {}) {
  return {
    projects: [project],
    providers: [],
    configs: [config],
    sessions: [],
    questions: [],
    sessionRequests: [],
    settings: { startAtLogin: false, keepAwake: false },
    ...overrides,
  };
}

async function stubPhone(page: Page, data: unknown) {
  await page.route('**/api/bootstrap', (route) =>
    route.fulfill({ json: data }),
  );
  await page.route(/\/api\/(events|questions\/events)(\?|$)/, (route) =>
    route.fulfill({ status: 204, body: '' }),
  );
  await page.route(/\/api\/sessions\/[^/]+\/(history|output)/, (route) =>
    route.fulfill({ status: 204, body: '' }),
  );
}

test('a phone with no projects explains where to add one', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await stubPhone(page, payload({ projects: [], configs: [] }));

  await page.goto('/phone.html');

  await expect(page.getByText('No projects yet')).toBeVisible();
  await expect(
    page.getByText('Add a project on the computer to start a session.'),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'New session' })).toBeDisabled();
  await expect(
    page.getByRole('group', { name: 'Session filter' }),
  ).toHaveCount(0);
});

test('the running filter, waiting row and tab sheet agree', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await stubPhone(
    page,
    payload({ sessions: [working, waiting, paused, done] }),
  );

  await page.goto('/phone.html');
  await expect(page.getByText('Working session')).toBeVisible();

  await page.getByRole('button', { name: 'Running' }).click();
  await expect(page.getByText('Working session')).toBeVisible();
  await expect(page.getByText('Waiting session')).toBeVisible();
  await expect(page.getByText('Paused session')).toHaveCount(0);
  await expect(page.getByText('Done session')).toHaveCount(0);

  await page.getByRole('button', { name: 'All' }).click();
  const waitingRow = page.locator('.phone-row', {
    hasText: 'Waiting session',
  });
  await expect(waitingRow.locator('.phone-row-preview')).toHaveText(
    'Waiting to push the fix',
  );
  await expect(waitingRow.locator('.phone-row-state')).toHaveText('Waiting');
  await expect(waitingRow).not.toContainText('Waiting for you');

  await page.getByText('Working session').click();
  await page.getByRole('button', { name: /All tabs/ }).click();
  const activeRow = page.locator('.phone-sheet-row.is-on');
  await expect(activeRow.locator('.phone-sheet-row-meta')).toContainText(
    'Working',
  );
  await expect(activeRow.locator('.phone-sheet-row-meta')).not.toContainText(
    'Open',
  );
});

for (const scheme of ['light', 'dark'] as const) {
  test(`the key rail fits at 390 in ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 390, height: 844 });
    await stubPhone(page, payload({ sessions: [terminal] }));

    await page.goto(`/phone.html#session=${encodeURIComponent(terminal.id)}`);

    const rail = page.getByRole('toolbar', { name: 'Keyboard keys' });
    await expect(rail).toBeVisible();
    const add = rail.getByRole('button', { name: 'Add a key' });
    await expect(add).toBeVisible();

    const fits = await rail.evaluate(
      (element) => element.scrollWidth <= element.clientWidth + 1,
    );
    expect(fits).toBeTruthy();

    const box = await add.boundingBox();
    expect(box).not.toBeNull();
    expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(390);

    const pageFits = await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    );
    expect(pageFits).toBeTruthy();
  });
}
