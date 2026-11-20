import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import {
  closeSession,
  createGitRepo,
  createProject,
  createTerminalSession,
  uniqueName,
} from './helpers';

const PNG_1X1 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';

test('phone layout shows files, an image, an upload and the diff', async ({
  page,
  request,
}) => {
  const label = uniqueName('phone-files');
  const repo = createGitRepo(label);
  const project = await createProject(request, label, repo);
  const session = await createTerminalSession(request, {
    projectId: project.id,
    name: uniqueName('phone-files-session'),
    folder: repo,
  });

  fs.writeFileSync(path.join(repo, 'shot.png'), Buffer.from(PNG_1X1, 'base64'));
  fs.writeFileSync(path.join(repo, 'note.txt'), 'A note for the phone\n');
  fs.appendFileSync(path.join(repo, 'README.md'), 'A changed line\n');

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/phone.html#session=${encodeURIComponent(session.id)}`);

  const files = page.getByRole('button', { name: 'Files', exact: true });
  await files.click();
  await expect(files).toHaveAttribute('aria-pressed', 'true');

  const list = page.locator('.phone-file-list');
  await expect(list).toContainText('shot.png');
  await expect(list).toContainText('note.txt');

  await list.getByRole('button', { name: /shot\.png/ }).click();
  const image = page.locator('.phone-image');
  await expect(image).toBeVisible();
  await expect(image).toHaveAttribute('alt', 'Image shot.png');
  await expect(image).toHaveAttribute('src', /^blob:/);

  await page.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.locator('.phone-image')).toHaveCount(0);

  await list.getByRole('button', { name: /note\.txt/ }).click();
  await expect(page.locator('.phone-file-text')).toContainText(
    'A note for the phone',
  );

  const changes = page.getByRole('button', { name: 'Changes', exact: true });
  await changes.click();
  await expect(changes).toHaveAttribute('aria-pressed', 'true');
  const changeList = page.locator('.phone-change-list');
  await expect(changeList).toContainText('README.md');
  await expect(changeList).toContainText('note.txt');

  await changeList
    .getByRole('button', { name: 'Open the diff for README.md' })
    .click();
  const diff = page.locator('.phone-diff');
  await expect(diff).toContainText('A changed line');
  await expect(diff.locator('.phone-diff-line.is-added')).not.toHaveCount(0);

  await files.click();
  await page.locator('input[type=file]').setInputFiles({
    name: 'upload.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('uploaded from the phone\n'),
  });
  await expect(page.locator('.phone-upload-notice')).toHaveText(
    'Uploaded upload.txt. The agent was told its path.',
  );
  await expect(files).toBeEnabled();

  await closeSession(request, session.id);
});
