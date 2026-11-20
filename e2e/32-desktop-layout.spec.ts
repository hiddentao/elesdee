import fs from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  openApp,
  sessionRow,
  uniqueName,
} from './helpers';

const LONG_SESSION_NAME =
  'Pricing page token migration for every provider';

async function expectNoPageOverflow(page: Page) {
  expect(
    await page.evaluate(
      () =>
        document.documentElement.scrollWidth -
        document.documentElement.clientWidth,
    ),
  ).toBeLessThanOrEqual(0);
}

async function hasScrollbarRule(page: Page, selector: string) {
  return page.evaluate((target) => {
    for (const sheet of Array.from(document.styleSheets)) {
      let rules: CSSRuleList;
      try {
        rules = sheet.cssRules;
      } catch {
        continue;
      }
      for (const rule of Array.from(rules)) {
        if (
          rule instanceof CSSStyleRule &&
          rule.selectorText.includes(target) &&
          (rule.style.width === '11px' || rule.style.height === '11px')
        ) {
          return true;
        }
      }
    }
    return false;
  }, selector);
}

async function emulateWidth(
  page: Page,
  width: number,
  colorScheme: 'light' | 'dark',
) {
  await page.emulateMedia({ colorScheme });
  await page.setViewportSize({ width, height: width === 1024 ? 768 : 900 });
}

test('desktop layout stays readable, scrollable and free of console errors', async ({
  page,
  request,
}) => {
  const errors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(message.text());
    }
  });
  page.on('pageerror', (error) => errors.push(error.message));

  await page.route('**/api/bootstrap', async (route) => {
    const response = await route.fetch();
    const payload = (await response.json()) as {
      providers?: Array<Record<string, unknown>>;
    };
    for (const provider of payload.providers ?? []) {
      if (provider.id === 'max') {
        provider.rows = [
          { label: '5-hour', window: 'five_hour', used: 58 },
          { label: 'Weekly', window: 'seven_day', used: 42 },
        ];
        provider.usageStatus = 'fresh';
        provider.usageReadAt = new Date().toISOString();
      }
    }
    await route.fulfill({ response, json: payload });
  });

  const label = uniqueName('desktop-layout');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: `${LONG_SESSION_NAME} ${label}`,
    folder: repo,
  });
  fs.writeFileSync(
    path.join(repo, 'README.md'),
    `elesdee e2e\n${'x'.repeat(360)}\n`,
  );

  try {
    await openApp(page);
    await sessionRow(page, LONG_SESSION_NAME).first().click();
    await expect(page.getByRole('tab', { name: new RegExp(LONG_SESSION_NAME) }))
      .toHaveAttribute('aria-selected', 'true');

    for (const colorScheme of ['light', 'dark'] as const) {
      for (const width of [1440, 1024]) {
        await emulateWidth(page, width, colorScheme);
        await expectNoPageOverflow(page);

        const row = sessionRow(page, LONG_SESSION_NAME).first();
        await expect(row).toHaveAttribute(
          'title',
          new RegExp(LONG_SESSION_NAME),
        );
        await expect(row.locator('.session-status-label')).toBeVisible();

        await page.getByRole('button', { name: 'Session panel' }).click();
        await expectNoPageOverflow(page);
        const dock = page.locator('.dock');
        const dockBox = await dock.boundingBox();
        expect(dockBox?.width ?? 0).toBeGreaterThanOrEqual(320);
        expect(dockBox?.width ?? 0).toBeLessThanOrEqual(380);

        const dockName = dock.locator('.dock-context-name');
        await expect(dockName).toHaveAttribute('title', new RegExp(
          LONG_SESSION_NAME,
        ));
        const nameBox = await dockName.boundingBox();
        const branchBox = await dock
          .locator('.dock-context-branch')
          .boundingBox();
        if (width === 1024) {
          expect(branchBox?.y ?? 0).toBeGreaterThanOrEqual(
            (nameBox?.y ?? 0) + (nameBox?.height ?? 0) - 1,
          );
        }

        await dock.getByRole('tab', { name: 'Changes', exact: true }).click();
        const diff = dock.locator('.diff-snippet');
        await expect(diff).toBeVisible();
        expect(
          await diff.evaluate(
            (element) => element.scrollWidth > element.clientWidth,
          ),
        ).toBe(true);
        expect(
          await hasScrollbarRule(page, '.diff-snippet::-webkit-scrollbar'),
        ).toBe(true);

        await dock.getByRole('button', { name: 'Close side panel' }).click();

        await page.locator('.usage-summary-button').click();
        await expectNoPageOverflow(page);
        const usage = page.getByRole('dialog', { name: 'Usage' });
        await expect(
          usage.getByText('5-hour limit', { exact: true }),
        ).toBeVisible();
        await expect(
          usage.getByText('Weekly limit', { exact: true }),
        ).toBeVisible();
        for (const text of ['5-hour limit', 'Weekly limit']) {
          expect(
            await usage.getByText(text, { exact: true }).evaluate(
              (element) => element.scrollWidth <= element.clientWidth + 1,
            ),
          ).toBe(true);
        }
        await expect(
          usage.getByText('work · anthropic', { exact: true }),
        ).toHaveCount(0);
        await usage.getByRole('button', { name: 'Close usage' }).click();

        await page
          .locator('.titlebar')
          .getByRole('button', { name: 'New session' })
          .click();
        const dialog = page.getByRole('dialog');
        await expectNoPageOverflow(page);
        const body = dialog.locator('.dialog-body');
        expect(
          await body.evaluate(
            (element) => getComputedStyle(element).overflowY,
          ),
        ).toBe('auto');
        expect(
          await hasScrollbarRule(page, '.dialog-body::-webkit-scrollbar'),
        ).toBe(true);

        const nameInput = dialog.getByLabel('Session name (optional)');
        const branch = dialog.getByLabel('Source branch (optional)');
        if (width === 1024) {
          const bodyRect = await body.boundingBox();
          const branchRect = await branch.boundingBox();
          expect(
            (branchRect?.y ?? 0) + (branchRect?.height ?? 0),
          ).toBeLessThanOrEqual(
            (bodyRect?.y ?? 0) + (bodyRect?.height ?? 0) + 1,
          );
        }
        await nameInput.scrollIntoViewIfNeeded();
        await expect(nameInput).toBeInViewport();
        await branch.scrollIntoViewIfNeeded();
        await expect(branch).toBeInViewport();
        const additional = dialog.getByText('Additional settings');
        await additional.scrollIntoViewIfNeeded();
        await expect(additional).toBeInViewport();

        const footerBox = await dialog.locator('.dialog-foot').boundingBox();
        const viewport = page.viewportSize()!;
        expect((footerBox?.y ?? 0) + (footerBox?.height ?? 0)).toBeLessThanOrEqual(
          viewport.height + 1,
        );
        expect(
          await dialog.evaluate(
            (element) => element.scrollWidth - element.clientWidth,
          ),
        ).toBeLessThanOrEqual(0);
        await dialog.getByRole('button', { name: 'Cancel' }).click();
      }
    }

    await emulateWidth(page, 1440, 'light');
    const lastTab = page.locator('.tab').last();
    const add = page.locator('.tab-add');
    const lastTabBox = await lastTab.boundingBox();
    const addBox = await add.boundingBox();
    expect(addBox?.x ?? 0).toBeLessThanOrEqual(
      (lastTabBox?.x ?? 0) + (lastTabBox?.width ?? 0) + 2,
    );

    expect(errors.filter((message) => message.includes('same key'))).toEqual(
      [],
    );
    expect(errors).toEqual([]);
  } finally {
    await closeSession(request, session.id);
  }
});
