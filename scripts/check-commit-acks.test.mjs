import test from 'node:test';
import assert from 'node:assert/strict';
import { missingAcks } from './check-commit-acks.mjs';

const docs = 'Docs-Checked: updated .dev/SPEC.md for the new close flow';
const translations = 'Translations-Checked: no user-facing strings changed; catalogs still match';

test('a code commit without acknowledgements is rejected', () => {
  const errors = missingAcks('feat(core): add resize', ['core/src/api.rs']);
  assert.equal(errors.length, 2, errors.join('\n'));
  assert.ok(errors.some((error) => error.includes('Docs-Checked')));
  assert.ok(errors.some((error) => error.includes('Translations-Checked')));
});

test('a code commit with only Docs-Checked still needs Translations-Checked', () => {
  const errors = missingAcks(`feat(core): add resize\n\nBody.\n\n${docs}`, ['core/src/api.rs']);
  assert.equal(errors.length, 1, errors.join('\n'));
  assert.match(errors[0], /Translations-Checked/);
});

test('a UI commit needs both acknowledgements', () => {
  const errors = missingAcks('fix(ui): align the sidebar', ['app/src/components/Sidebar.tsx']);
  assert.equal(errors.length, 2, errors.join('\n'));
  assert.ok(errors.some((error) => error.includes('Docs-Checked')));
  assert.ok(errors.some((error) => error.includes('Translations-Checked')));
});

test('a UI commit with both acknowledgements passes', () => {
  const errors = missingAcks(
    `fix(ui): align the sidebar\n\n${docs}\n${translations}`,
    ['app/src/components/Sidebar.tsx']
  );
  assert.deepEqual(errors, []);
});

test('a code commit with both acknowledgements passes', () => {
  const errors = missingAcks(
    `feat(core): add resize\n\n${docs}\n${translations}`,
    ['core/src/api.rs', 'src-tauri/src/main.rs']
  );
  assert.deepEqual(errors, []);
});

test('docs-only and test-only commits need no trailer', () => {
  assert.deepEqual(missingAcks('docs: clarify the API', ['.dev/SPEC.md']), []);
  assert.deepEqual(missingAcks('test: add a case', ['core/tests/api.rs']), []);
});

test('release and merge commits are exempt', () => {
  assert.deepEqual(missingAcks('chore(release): 0.2.0', ['app/src/App.tsx']), []);
  assert.deepEqual(missingAcks('Merge branch main', ['app/src/App.tsx']), []);
});

test('meaningless trailer values are rejected', () => {
  for (const value of ['', 'n/a', 'none', 'checked', 'ok']) {
    const errors = missingAcks(
      `feat: change\n\nDocs-Checked: ${value}\nTranslations-Checked: ${value}`,
      ['core/src/api.rs']
    );
    assert.equal(errors.length, 2, `value "${value}" should be rejected`);
  }
});

test('workflow changes need Docs-Checked only', () => {
  const errors = missingAcks('ci: add e2e', ['.github/workflows/ci.yml']);
  assert.equal(errors.length, 1);
  assert.match(errors[0], /Docs-Checked/);
});

test('the trailers can appear with other trailers', () => {
  const errors = missingAcks(
    `feat: change\n\nSigned-off-by: A <a@example.com>\nDocs-Checked: verified AGENTS.md and SPEC.md\nTranslations-Checked: updated the English and French catalogs`,
    ['core/src/api.rs']
  );
  assert.deepEqual(errors, []);
});
