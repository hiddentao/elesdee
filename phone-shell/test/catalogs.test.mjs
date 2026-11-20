import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const catalogDir = join(here, '..', 'ui', 'src', 'i18n');
const files = readdirSync(catalogDir)
  .filter((name) => name.endsWith('.json'))
  .sort();

const english = JSON.parse(
  readFileSync(join(catalogDir, 'en.json'), 'utf8'),
);
const keys = Object.keys(english).sort();

function placeholders(value) {
  return [...value.matchAll(/\{(\w+)\}/g)]
    .map((match) => match[1])
    .sort();
}

test('the shell ships twelve language catalogs', () => {
  assert.equal(files.length, 12);
  assert.deepEqual(files, [
    'de.json',
    'en.json',
    'es.json',
    'fr.json',
    'it.json',
    'ja.json',
    'ko.json',
    'nl.json',
    'pl.json',
    'pt-BR.json',
    'sv.json',
    'zh-CN.json',
  ]);
});

for (const file of files) {
  const catalog = JSON.parse(readFileSync(join(catalogDir, file), 'utf8'));

  test(`${file} has exactly the English keys`, () => {
    assert.deepEqual(Object.keys(catalog).sort(), keys);
  });

  test(`${file} keeps every placeholder`, () => {
    for (const key of keys) {
      assert.deepEqual(
        placeholders(catalog[key]),
        placeholders(english[key]),
        `${file} ${key}`,
      );
    }
  });

  test(`${file} has no empty strings`, () => {
    for (const key of keys) {
      assert.ok(catalog[key].trim().length > 0, `${file} ${key}`);
    }
  });
}
