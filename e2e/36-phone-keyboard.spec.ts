import {
  expect,
  test,
  type APIRequestContext,
  type Locator,
  type Page,
} from '@playwright/test';
import {
  bootstrap,
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  uniqueName,
  type Session,
} from './helpers';

interface Fixture {
  sessions: Session[];
}

async function createFixture(
  request: APIRequestContext,
  prefix: string,
  count: number,
): Promise<Fixture> {
  const label = uniqueName(prefix);
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const sessions: Session[] = [];
  for (let index = 1; index <= count; index += 1) {
    sessions.push(
      await createTerminalSession(request, {
        projectId: project.id,
        name: uniqueName(`${prefix}-session-${index}`),
        folder: repo,
      }),
    );
  }
  return { sessions };
}

async function closeFixture(
  request: APIRequestContext,
  sessions: Session[],
): Promise<void> {
  const ids = new Set(sessions.map((session) => session.id));
  const data = await bootstrap(request);
  for (const session of data.sessions) {
    if (ids.has(session.id) && session.status === 'active') {
      await closeSession(request, session.id);
    }
  }
}

async function openSession(page: Page, session: Session): Promise<void> {
  await page.goto(`/phone.html#session=${encodeURIComponent(session.id)}`);
  await expect(page.locator('.phone-title')).toHaveText(session.name);
}

/** Presses Tab the given number of times and checks each stop stays inside. */
async function expectFocusStaysInside(
  page: Page,
  sheet: Locator,
  presses: number,
): Promise<void> {
  for (let step = 1; step <= presses; step += 1) {
    await page.keyboard.press('Tab');
    await expect(
      sheet.locator(':focus'),
      `focus left the sheet after Tab press ${step}`,
    ).toHaveCount(1);
  }
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
});

test('the all tabs sheet takes focus, traps Tab, closes on Escape and returns focus', async ({
  page,
  request,
}) => {
  const fixture = await createFixture(request, 'phone-tabs-focus', 2);
  try {
    await openSession(page, fixture.sessions[0]);

    const trigger = page.getByRole('button', { name: /All tabs/ });
    await trigger.click();

    const sheet = page.getByRole('dialog', { name: 'All tabs' });
    await expect(sheet).toBeVisible();
    await expect(sheet.locator('.phone-sheet-row.is-on')).toBeFocused();

    await expectFocusStaysInside(page, sheet, 30);

    await page.keyboard.press('Escape');
    await expect(sheet).toHaveCount(0);
    await expect(trigger).toBeFocused();
  } finally {
    await closeFixture(request, fixture.sessions);
  }
});

test('the session actions sheet is inert behind, traps Tab and closes on Escape', async ({
  page,
  request,
}) => {
  const fixture = await createFixture(request, 'phone-actions-focus', 2);
  try {
    await openSession(page, fixture.sessions[0]);

    const app = page.locator('.phone');
    await expect(app).not.toHaveAttribute('inert');

    const trigger = page.getByRole('button', { name: 'Session actions' });
    await trigger.click();

    const sheet = page.getByRole('dialog', { name: 'Session actions' });
    await expect(sheet).toBeVisible();
    await expect(app).toHaveAttribute('inert');
    await expect(
      sheet.locator('.phone-sheet-row:not(.is-danger)').first(),
    ).toBeFocused();

    // The page behind cannot take focus while the sheet is open.
    await page.getByRole('button', { name: 'Back to sessions' }).focus();
    await expect(sheet.locator(':focus')).toHaveCount(1);

    await expectFocusStaysInside(page, sheet, 30);

    await page.keyboard.press('Escape');
    await expect(sheet).toHaveCount(0);
    await expect(app).not.toHaveAttribute('inert');
    await expect(trigger).toBeFocused();
  } finally {
    await closeFixture(request, fixture.sessions);
  }
});

test('the active tab carries aria-current and the trigger points at the sheet', async ({
  page,
  request,
}) => {
  const fixture = await createFixture(request, 'phone-tab-state', 2);
  try {
    const [first, second] = fixture.sessions;
    await openSession(page, first);

    const firstTab = page.locator('.phone-tab', { hasText: first.name });
    const secondTab = page.locator('.phone-tab', { hasText: second.name });
    await expect(page.locator('.phone-tab[aria-current="page"]')).toHaveCount(1);
    await expect(firstTab).toHaveAttribute('aria-current', 'page');
    await expect(secondTab).not.toHaveAttribute('aria-current');

    const trigger = page.getByRole('button', { name: /All tabs/ });
    await expect(trigger).toHaveAttribute('aria-controls', 'phone-tabs-sheet');
    await expect(trigger).toHaveAttribute('aria-expanded', 'false');

    await trigger.click();
    const sheet = page.locator('#phone-tabs-sheet');
    await expect(sheet).toBeVisible();
    await expect(sheet).toHaveAttribute('role', 'dialog');
    await expect(trigger).toHaveAttribute('aria-expanded', 'true');

    await sheet
      .locator('.phone-sheet-row', { hasText: second.name })
      .click();
    await expect(page.locator('.phone-title')).toHaveText(second.name);
    await expect(page.locator('.phone-tab[aria-current="page"]')).toHaveCount(1);
    await expect(secondTab).toHaveAttribute('aria-current', 'page');
    await expect(firstTab).not.toHaveAttribute('aria-current');
  } finally {
    await closeFixture(request, fixture.sessions);
  }
});

test('an incomplete new session focuses the first missing field and links its error', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-start-validation');
  const repo = createGitRepo(label);
  await createProject(request, label, repo);

  await page.goto('/phone.html');
  const trigger = page.getByRole('button', { name: 'New session' });
  await expect(trigger).toBeVisible();
  await trigger.click();

  const sheet = page.getByRole('dialog', { name: 'New session' });
  await expect(sheet).toBeVisible();

  // docs/phone.md keeps the button disabled until both fields are chosen, so
  // an incomplete submit is raised through the form element itself.
  const submit = sheet.getByRole('button', { name: 'Start session' });
  await expect(submit).toBeDisabled();

  const project = sheet.getByRole('combobox', { name: 'Project' });
  const config = sheet.getByRole('combobox', { name: 'AI config' });
  await expect(project).toBeFocused();

  await config.focus();
  await expect(config).toBeFocused();

  await sheet.locator('form').evaluate((form) => {
    (form as HTMLFormElement).requestSubmit();
  });

  await expect(project).toBeFocused();
  await expect(project).toHaveAttribute('aria-invalid', 'true');
  await expect(config).not.toHaveAttribute('aria-invalid');

  const describedBy = await project.getAttribute('aria-describedby');
  expect(describedBy).toBe('phone-start-project-error');
  await expect(page.locator('#phone-start-project-error')).toHaveText(
    'Choose a project and an AI config first.',
  );
});
