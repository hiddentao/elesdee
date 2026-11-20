import { expect, test } from '@playwright/test';
import {
  bootstrap,
  createAgentSession,
  createGitRepo,
  createProject,
  uniqueName,
} from './helpers';

test('a long swipe only reveals Close and the user closes the tab', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-close-project');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const first = await createAgentSession(request, {
    projectId: project.id,
    name: 'Phone tab close 1',
    folder: repo,
  });
  const second = await createAgentSession(request, {
    projectId: project.id,
    name: 'Phone tab close 2',
    folder: repo,
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/phone.html#session=${encodeURIComponent(first.id)}`);
  await expect(page.locator('.phone-title')).toHaveText('Phone tab close 1');

  await page.getByRole('button', { name: /All tabs/ }).click();
  const sheet = page.getByRole('dialog', { name: 'All tabs' });
  const row = sheet.locator('.phone-sheet-row', { hasText: 'Phone tab close 1' });
  await expect(row).toBeVisible();

  const box = await row.boundingBox();
  expect(box).not.toBeNull();
  if (!box) {
    return;
  }
  await page.mouse.move(box.x + box.width - 20, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x - 200, box.y + box.height / 2, { steps: 12 });
  await page.mouse.up();

  await expect
    .poll(async () => row.evaluate((element) => getComputedStyle(element).transform))
    .not.toBe('none');
  const before = await bootstrap(request);
  expect(
    before.sessions.find((session) => session.id === first.id)?.status,
  ).toBe('active');
  expect(
    before.sessions.find((session) => session.id === second.id)?.status,
  ).toBe('active');

  await sheet
    .getByRole('button', { name: 'Close tab, Phone tab close 1' })
    .click();
  await expect(page.locator('.phone-title')).not.toHaveText(
    'Phone tab close 1',
  );
  await expect(page.locator('.phone-title')).not.toBeEmpty();

  const after = await bootstrap(request);
  expect(after.sessions.find((session) => session.id === first.id)?.status).toBe(
    'active',
  );
});
