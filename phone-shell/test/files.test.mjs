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
