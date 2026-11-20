import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';

import { blake3 } from '@noble/hashes/blake3.js';
import { unzipSync } from 'fflate';

import {
  PHONE_UI_ARCHIVE,
  PHONE_UI_MANIFEST,
  buildPhoneUi,
  checkEntryName,
} from './package-phone-ui.mjs';

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'elesdee-phone-ui-'));
  const input = join(root, 'dist-phone');
  await mkdir(join(input, 'assets'), { recursive: true });
  await writeFile(
    join(input, 'phone.html'),
    '<!doctype html><div id="root"></div><script type="module" src="./assets/app.js"></script>\n',
  );
  await writeFile(join(input, 'assets', 'app.js'), 'console.log("phone");\n');
  await writeFile(join(input, 'assets', 'app.css'), 'body { margin: 0; }\n');
  return { root, input };
}

test('the package renames the entry page and writes a matching manifest', async () => {
  const { root, input } = await fixture();
  try {
    const output = join(root, 'package');
    const { archive, manifest } = await buildPhoneUi({
      inputDir: input,
      outputDir: output,
      version: '0.1.0',
    });

    const stored = JSON.parse(
      await readFile(join(output, PHONE_UI_MANIFEST), 'utf8'),
    );
    const zip = new Uint8Array(await readFile(join(output, PHONE_UI_ARCHIVE)));
    assert.deepEqual(stored, manifest);
    assert.equal(manifest.schema, 1);
    assert.equal(manifest.appVersion, '0.1.0');
    assert.equal(manifest.protocolVersion, 1);
    assert.equal(manifest.minBridgeVersion, 1);
    assert.equal(manifest.files, 3);
    assert.equal(manifest.bytes, archive.length);
    assert.equal(manifest.bytes, zip.length);
    assert.equal(
      manifest.buildId,
      `blake3:${Buffer.from(blake3(zip)).toString('hex')}`,
    );

    const entries = unzipSync(zip);
    assert.deepEqual(Object.keys(entries).sort(), [
      'assets/app.css',
      'assets/app.js',
      'index.html',
    ]);
    assert.equal(entries['phone.html'], undefined);
    assert.match(
      new TextDecoder().decode(entries['index.html']),
      /<div id="root">/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('the same input produces the same archive and hash', async () => {
  const { root, input } = await fixture();
  try {
    const first = await buildPhoneUi({
      inputDir: input,
      outputDir: join(root, 'first'),
      version: '0.1.0',
    });
    const second = await buildPhoneUi({
      inputDir: input,
      outputDir: join(root, 'second'),
      version: '0.1.0',
    });
    assert.deepEqual(first.archive, second.archive);
    assert.deepEqual(first.manifest, second.manifest);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('an input without an entry page is refused', async () => {
  const root = await mkdtemp(join(tmpdir(), 'elesdee-phone-ui-'));
  try {
    const input = join(root, 'dist-phone');
    await mkdir(input, { recursive: true });
    await writeFile(join(input, 'app.js'), 'console.log("phone");\n');
    await assert.rejects(
      buildPhoneUi({
        inputDir: input,
        outputDir: join(root, 'package'),
        version: '0.1.0',
      }),
      /entry page/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a path that leaves the package is refused', () => {
  for (const name of [
    '../evil.js',
    '/etc/passwd',
    'assets\\evil.js',
    'a//b',
    '',
  ]) {
    assert.throws(() => checkEntryName(name), undefined, name);
  }
  assert.equal(checkEntryName('assets/app.js'), 'assets/app.js');
  assert.equal(checkEntryName('index.html'), 'index.html');
});
