import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));

function load(source) {
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
  }).outputText;
  return import(
    `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
  );
}

const shared = await load(
  readFileSync(join(here, '..', '..', 'app', 'src', 'phone', 'files.ts'), 'utf8'),
);
const files = await load(
  readFileSync(join(here, '..', 'ui', 'src', 'files.ts'), 'utf8'),
);

test('an upload failure maps to a shell message key', () => {
  assert.equal(files.uploadNoticeFor('too_large'), 'files.uploadTooLarge');
  assert.equal(files.uploadNoticeFor('conflict'), 'files.uploadClosed');
  assert.equal(files.uploadNoticeFor('bad_request'), 'files.uploadInvalid');
  assert.equal(files.uploadNoticeFor('transport'), 'files.uploadFailed');
  assert.equal(files.uploadNoticeFor('timeout'), 'files.uploadFailed');
});

test('a cancelled picker shows no message', () => {
  assert.equal(files.uploadNoticeFor('cancelled'), null);
});

test('an image preview becomes a data URL with its type', () => {
  assert.equal(
    files.imageDataUrl('image/png', 'iVBORw0KGgo='),
    'data:image/png;base64,iVBORw0KGgo=',
  );
  assert.equal(
    files.imageDataUrl('image/jpeg', 'abc'),
    'data:image/jpeg;base64,abc',
  );
});

test('a non-image type falls back to a safe data URL type', () => {
  assert.equal(
    files.imageDataUrl('text/html', 'abc'),
    'data:application/octet-stream;base64,abc',
  );
});

test('a unified diff splits into coloured rows', () => {
  const diff = [
    'diff --git a/app.ts b/app.ts',
    '--- a/app.ts',
    '+++ b/app.ts',
    '@@ -1,2 +1,2 @@',
    ' unchanged',
    '-removed',
    '+added',
    '',
  ].join('\n');
  const rows = files.diffRows(diff, shared.diffLineKind);
  assert.deepEqual(
    rows.map((row) => row.kind),
    ['context', 'context', 'context', 'hunk', 'context', 'removed', 'added', 'context'],
  );
  assert.equal(rows[7].text, ' ');
  assert.equal(rows[6].text, '+added');
});

test('the shared image name test is available to the shell', () => {
  assert.equal(shared.isImageName('shot.PNG'), true);
  assert.equal(shared.isImageName('notes.md'), false);
});

test('an image failure maps a size code to the size message', () => {
  assert.equal(files.imageNoticeFor({ code: 'too_large' }), 'files.imageTooLarge');
  assert.equal(files.imageNoticeFor({ code: 'unsupported' }), 'files.notImage');
  assert.equal(files.imageNoticeFor({ status: 413 }), 'files.imageTooLarge');
  assert.equal(files.imageNoticeFor({ status: 415 }), 'files.notImage');
});

test('an image over the cap keeps its size message', async () => {
  const result = await files.loadPreview(
    true,
    async () => ({ content: '' }),
    async () => {
      throw { code: 'too_large' };
    },
  );
  assert.deepEqual(result, { kind: 'error', key: 'files.imageTooLarge' });
});

test('an image without an extension previews after the text read fails', async () => {
  const image = { contentType: 'image/png', data: 'iVBORw0KGgo=' };
  const result = await files.loadPreview(
    false,
    async () => {
      throw { code: 'unprocessable' };
    },
    async () => image,
  );
  assert.deepEqual(result, { kind: 'image', value: image });
});

test('a binary file that is not an image stays a text failure', async () => {
  const result = await files.loadPreview(
    false,
    async () => {
      throw { code: 'unprocessable' };
    },
    async () => {
      throw { code: 'unsupported' };
    },
  );
  assert.deepEqual(result, { kind: 'error', key: 'files.notText' });
});

test('a text file does not call the image read', async () => {
  const result = await files.loadPreview(
    false,
    async () => ({ content: '# Notes' }),
    async () => {
      throw new Error('the image read must not run');
    },
  );
  assert.deepEqual(result, { kind: 'text', value: { content: '# Notes' } });
});
