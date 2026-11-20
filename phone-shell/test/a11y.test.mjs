import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(
  join(here, '..', 'ui', 'src', 'app.ts'),
  'utf8',
);

test('a sheet is a modal dialog that takes focus and is keyboard trapped', () => {
  assert.match(source, /role: 'dialog',\s*'aria-modal': 'true',\s*tabindex: '-1'/);
  assert.match(source, /private focusSheet\(/);
  assert.match(source, /this\.focusSheet\(layer\)/);
  assert.match(source, /event\.key === 'Escape'/);
  assert.match(source, /if \(event\.defaultPrevented\)/);
  assert.match(source, /event\.key !== 'Tab'/);
  assert.match(source, /focusableIn\(sheet\)/);
});

test('the rest of the screen is inert while a sheet is open', () => {
  assert.match(source, /shell\.setAttribute\('inert', ''\)/);
  assert.match(source, /insertBefore\(shell, this\.liveRegion\)/);
  assert.match(source, /insertBefore\(layer, this\.liveRegion\)/);
});

test('closing a sheet returns focus to its trigger', () => {
  assert.match(source, /this\.focusAfterRenderId = this\.overlayReturnId/);
  assert.match(source, /document\.getElementById\(this\.focusAfterRenderId\)/);
  assert.match(source, /'session-actions-trigger'/);
});

test('each screen title is a real H1', () => {
  assert.match(source, /function titleEl\(text: string\): HTMLElement \{\s*return el\('h1'/);
  assert.doesNotMatch(source, /el\('span', \{ class: 'phone-title'/);
});

test('a new card is announced once through a polite live region', () => {
  assert.match(source, /role: 'status', 'aria-live': 'polite'/);
  assert.match(source, /if \(key === this\.announcedCard\)/);
  assert.match(source, /this\.announcedCard = key/);
});

test('an incomplete launch moves focus to the first invalid field', () => {
  assert.match(source, /setAttribute\('aria-invalid', 'true'\)/);
  assert.match(source, /setAttribute\('aria-describedby', 'new-session-error'\)/);
  assert.match(source, /document\.getElementById\(fieldId\)\?\.focus\(\)/);
});
