import { expect, test } from '@playwright/test';
import {
  createProject,
  createProjectFolder,
  createTerminalSession,
  getSession,
  openApp,
  uniqueName,
} from './helpers';

test('drags a session row onto another project', async ({ page, request }) => {
  const sourceLabel = uniqueName('move-source');
  const sourceFolder = createProjectFolder(sourceLabel);
  const source = await createProject(request, sourceLabel, sourceFolder);
  const targetLabel = uniqueName('move-target');
  const targetFolder = createProjectFolder(targetLabel);
  const target = await createProject(request, targetLabel, targetFolder);
  const sessionName = uniqueName('move-session');
  const created = await createTerminalSession(request, {
    projectId: source.id,
    name: sessionName,
    folder: sourceFolder,
  });

  await openApp(page);

  const row = page.locator('.session-row').filter({ hasText: sessionName });
  const targetGroup = page
    .locator('.project-group')
    .filter({ has: page.locator('.project-name', { hasText: targetLabel }) });
  await expect(row).toBeVisible();
  await expect(targetGroup).toBeVisible();

  const sourceBox = await row.boundingBox();
  const targetBox = await targetGroup.boundingBox();
  expect(sourceBox).not.toBeNull();
  expect(targetBox).not.toBeNull();
  if (!sourceBox || !targetBox) {
    return;
  }

  const startX = sourceBox.x + sourceBox.width / 2;
  const startY = sourceBox.y + sourceBox.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + 12, startY + 12);
  await page.mouse.move(targetBox.x + 20, targetBox.y + 16, { steps: 5 });

  await expect(targetGroup).toHaveClass(/is-drop-target/);
  await page.mouse.up();

  await expect(
    targetGroup.locator('.session-row').filter({ hasText: sessionName }),
  ).toBeVisible();
  await expect
    .poll(async () => (await getSession(request, created.id))?.projectId)
    .toBe(target.id);
});
