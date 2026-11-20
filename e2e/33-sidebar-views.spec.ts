import { expect, test } from '@playwright/test';
import {
  closeSession,
  createProject,
  createProjectFolder,
  createTerminalSession,
  deleteSession,
  openApp,
  uniqueName,
} from './helpers';

test('sidebar groups, collapses and filters sessions in both views', async ({
  page,
  request,
}) => {
  const labelA = uniqueName('sidebar-a');
  const labelB = uniqueName('sidebar-b');
  const projectA = await createProject(
    request,
    labelA,
    createProjectFolder(labelA),
  );
  const projectB = await createProject(
    request,
    labelB,
    createProjectFolder(labelB),
  );

  const activeA = await createTerminalSession(request, {
    projectId: projectA.id,
    name: uniqueName('sidebar-active-a'),
    folder: createProjectFolder(`${labelA}-active`),
  });
  const closedA = await createTerminalSession(request, {
    projectId: projectA.id,
    name: uniqueName('sidebar-closed-a'),
    folder: createProjectFolder(`${labelA}-closed`),
  });
  await closeSession(request, closedA.id);
  const activeB = await createTerminalSession(request, {
    projectId: projectB.id,
    name: uniqueName('sidebar-active-b'),
    folder: createProjectFolder(`${labelB}-active`),
  });

  try {
    await openApp(page);

    const groupA = page.locator(
      `.project-group[data-project-id="${projectA.id}"]`,
    );
    const groupB = page.locator(
      `.project-group[data-project-id="${projectB.id}"]`,
    );
    const rowA = groupA.locator('.session-row', { hasText: activeA.name });
    const rowB = groupB.locator('.session-row', { hasText: activeB.name });

    await expect(rowA).toBeVisible();
    await expect(rowB).toBeVisible();
    await expect(
      groupA.getByRole('button', { name: `Collapse ${labelA}` }),
    ).toBeVisible();
    await expect(
      groupA.locator('.session-row', { hasText: closedA.name }),
    ).toHaveCount(0);

    await page.getByRole('button', { name: 'Show closed' }).click();
    const closedRowA = groupA.locator('.session-row', {
      hasText: closedA.name,
    });
    await expect(closedRowA).toBeVisible();
    await expect(closedRowA).toContainText('Closed');

    const filters = page.getByRole('group', { name: 'Session filter' });
    const activeFilter = filters.getByRole('button', { name: /^Active/ });
    await activeFilter.click();
    await expect(activeFilter).toHaveAttribute('aria-pressed', 'true');
    await expect(closedRowA).toHaveCount(0);
    await expect(rowA).toBeVisible();

    await activeFilter.click();
    await expect(activeFilter).toHaveAttribute('aria-pressed', 'false');
    await expect(closedRowA).toBeVisible();

    await groupA
      .getByRole('button', { name: `Collapse ${labelA}` })
      .click();
    await expect(groupA.locator('.project-sessions')).toBeHidden();
    await expect(rowA).toBeHidden();
    await expect(rowB).toBeVisible();
    await expect(
      groupA.getByRole('group', {
        name: new RegExp(`^${labelA}, 1 active session$`),
      }),
    ).toBeVisible();

    await groupA.getByRole('button', { name: `Expand ${labelA}` }).click();
    await expect(rowA).toBeVisible();

    await page.getByRole('radio', { name: 'Flat', exact: true }).check();
    await expect(page.locator('.project-group')).toHaveCount(0);
    const flatRowA = page.locator('.session-row', { hasText: activeA.name });
    const flatRowB = page.locator('.session-row', { hasText: activeB.name });
    await expect(flatRowA.locator('.session-project')).toHaveText(labelA);
    await expect(flatRowB.locator('.session-project')).toHaveText(labelB);
  } finally {
    for (const session of [activeA, activeB]) {
      try {
        await closeSession(request, session.id);
      } catch {
        // The session may already be closed from a failed step.
      }
    }
    for (const session of [activeA, closedA, activeB]) {
      try {
        await deleteSession(request, session.id);
      } catch {
        // The session may already be removed from a failed step.
      }
    }
  }
});
