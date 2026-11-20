#!/usr/bin/env node
// Packs the phone UI build into one archive and writes its manifest.
//
// Entries are sorted, stored without compression and stamped with one fixed
// time, so the same input produces the same bytes. The manifest hash covers
// the archive bytes. The shell checks both before it keeps a download.

import { readFile, readdir, rm, mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { blake3 } from '@noble/hashes/blake3.js';
import { zipSync } from 'fflate';

export const PHONE_UI_SCHEMA = 1;
export const PHONE_UI_MANIFEST = 'phone-ui.json';
export const PHONE_UI_ARCHIVE = 'phone-ui.zip';
export const MAX_ENTRY_BYTES = 512;

const FIXED_TIME = new Date('1980-01-01T00:00:00Z');
const INDEX_NAME = 'index.html';
const BUILT_ENTRY = 'phone.html';

/** Returns the relative entry name for one file. */
function entryName(root, full) {
  return relative(root, full).split(sep).join('/');
}

/** Refuses a name that is not a plain relative path. */
export function checkEntryName(name) {
  if (typeof name !== 'string' || name.length === 0) {
    throw new Error('the package has an empty entry name');
  }
  if (new TextEncoder().encode(name).length > MAX_ENTRY_BYTES) {
    throw new Error(`the package entry name is too long: ${name}`);
  }
  if (
    name.startsWith('/') ||
    name.includes('\\') ||
    name.includes('\0') ||
    name.split('/').some((part) => part === '' || part === '.' || part === '..')
  ) {
    throw new Error(`the package entry name is not a relative path: ${name}`);
  }
  return name;
}

async function collectFiles(root, directory = root) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const full = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectFiles(root, full)));
    } else if (entry.isFile()) {
      files.push({ name: entryName(root, full), full });
    }
  }
  return files;
}

function toHex(bytes) {
  return Buffer.from(bytes).toString('hex');
}

/**
 * Builds the package from a Vite output folder.
 *
 * The built `phone.html` becomes `index.html`, so the shell can load the
 * package folder as a site. A missing entry page is an error.
 */
export async function buildPhoneUi({
  inputDir,
  outputDir,
  version,
  schema = PHONE_UI_SCHEMA,
  protocolVersion = 1,
  minBridgeVersion = 1,
}) {
  if (!inputDir || !outputDir) {
    throw new Error('the input and output folders are required');
  }
  if (typeof version !== 'string' || version.trim() === '') {
    throw new Error('the app version is required');
  }

  const files = await collectFiles(inputDir);
  const entries = new Map();
  for (const file of files) {
    const name = checkEntryName(
      file.name === BUILT_ENTRY ? INDEX_NAME : file.name,
    );
    const key = name.toLowerCase();
    if (entries.has(key)) {
      throw new Error(`the package has duplicate entries: ${name}`);
    }
    entries.set(key, { name, full: file.full });
  }
  if (!entries.has(INDEX_NAME)) {
    throw new Error(
      `the build has no ${BUILT_ENTRY} or ${INDEX_NAME} entry page`,
    );
  }

  const names = [...entries.values()]
    .map((entry) => entry.name)
    .sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));

  const data = {};
  for (const name of names) {
    const bytes = new Uint8Array(await readFile(entries.get(name.toLowerCase()).full));
    data[name] = [bytes, { level: 0, mtime: FIXED_TIME }];
  }
  const archive = zipSync(data, { level: 0, mtime: FIXED_TIME });
  const manifest = {
    schema,
    buildId: `blake3:${toHex(blake3(archive))}`,
    appVersion: version,
    protocolVersion,
    minBridgeVersion,
    bytes: archive.length,
    files: names.length,
  };

  await rm(outputDir, { recursive: true, force: true });
  await mkdir(outputDir, { recursive: true });
  await writeFile(join(outputDir, PHONE_UI_ARCHIVE), archive);
  await writeFile(
    join(outputDir, PHONE_UI_MANIFEST),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );

  return { archive, manifest };
}

async function main() {
  const here = dirname(fileURLToPath(import.meta.url));
  const root = resolve(here, '..');
  const inputDir = join(root, 'app', 'dist-phone');
  const outputDir = join(root, 'app', 'dist-phone-package');
  const packageJson = JSON.parse(
    await readFile(join(root, 'package.json'), 'utf8'),
  );

  const { manifest } = await buildPhoneUi({
    inputDir,
    outputDir,
    version: packageJson.version,
  });
  console.log(
    `Packed the phone UI: ${manifest.files} files, ${manifest.bytes} bytes, ${manifest.buildId}`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(`Could not pack the phone UI: ${error.message}`);
    process.exit(1);
  });
}
