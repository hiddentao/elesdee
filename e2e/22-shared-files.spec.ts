import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  CORE_URL,
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  openApp,
  sessionRow,
  uniqueName,
} from './helpers';

test('shows a file shared by a session in the Files panel', async ({
  page,
  request,
}) => {
  const label = uniqueName('shared-files');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const name = uniqueName('Shared files');
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name,
    folder: repo,
  });
  fs.writeFileSync(
    path.join(repo, 'shared-notes.txt'),
    'shared through the session tool\n',
  );

  try {
    const shared = await request.post(
      `${CORE_URL}/internal/mcp/${session.id}`,
      {
        data: {
          jsonrpc: '2.0',
          id: 1,
          method: 'tools/call',
          params: {
            name: 'share_file',
            arguments: {
              path: 'shared-notes.txt',
              caption: 'Notes for review',
            },
          },
        },
      },
    );
    expect(shared.ok()).toBeTruthy();
    const sharedBody = (await shared.json()) as {
      result: { isError: boolean; content: { text: string }[] };
    };
    expect(sharedBody.result.isError).toBe(false);

    await openApp(page);
    await sessionRow(page, name).first().click();

    await page.getByRole('button', { name: 'Session panel' }).click();
    await expect(
      page.getByRole('tab', { name: 'Files', exact: true }),
    ).toBeVisible();

    const sharedRow = page
      .locator('.shared-list')
      .getByRole('button', { name: /shared-notes\.txt/ });
    await expect(sharedRow).toBeVisible();
    await sharedRow.click();

    await expect(page.getByLabel('shared-notes.txt')).toContainText(
      'shared through the session tool',
    );
    await expect(
      page.getByRole('link', { name: 'Download' }),
    ).toBeVisible();
  } finally {
    await closeSession(request, session.id);
  }
});
