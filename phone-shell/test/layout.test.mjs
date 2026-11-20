import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const browser = readFileSync(
  join(repo, 'app', 'src', 'styles', 'phone.css'),
  'utf8',
);
const shell = readFileSync(
  join(here, '..', 'ui', 'src', 'styles', 'shell.css'),
  'utf8',
);

function block(css, selector) {
  const start = css.indexOf(`${selector} {`);
  assert.ok(start >= 0, `${selector} is missing`);
  const end = css.indexOf('}', start);
  return css.slice(start, end + 1);
}

function chipBlock(css, selector) {
  const start = css.indexOf(selector);
  assert.ok(start >= 0, `${selector} key chip is missing`);
  const open = css.indexOf('{', start);
  const end = css.indexOf('}', open);
  return css.slice(open, end + 1);
}

test('the browser key rail wraps instead of hiding a sideways scroll', () => {
  const rail = block(browser, '.phone-key-rail');
  assert.match(rail, /flex-wrap:\s*wrap/);
  assert.doesNotMatch(rail, /overflow-x:\s*auto/);
  assert.doesNotMatch(rail, /scrollbar-width:\s*none/);
  assert.match(
    chipBlock(browser, '.phone-key-rail .key-chip,'),
    /min-width:\s*34px/,
  );
});

test('the shell key rail wraps instead of hiding a sideways scroll', () => {
  const rail = block(shell, '.key-rail');
  assert.match(rail, /flex-wrap:\s*wrap/);
  assert.doesNotMatch(rail, /overflow-x:\s*auto/);
  assert.doesNotMatch(rail, /scrollbar-width:\s*none/);
  assert.match(chipBlock(shell, '.key-chip {'), /min-width:\s*34px/);
});

test('both phone surfaces show a named empty state card', () => {
  assert.match(browser, /\.phone-empty-card\s*\{/);
  assert.match(browser, /\.phone-empty-title\s*\{/);
  assert.match(shell, /\.phone-empty-card\s*\{/);
  assert.match(shell, /\.phone-empty-title\s*\{/);
});

test('the stacked session action row is gone from both surfaces', () => {
  assert.doesNotMatch(browser, /\.phone-session-actions/);
  assert.doesNotMatch(shell, /\.session-actions\s*\{/);
});
