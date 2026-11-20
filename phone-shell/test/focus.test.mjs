import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';
import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, '..', 'ui', 'src', 'focus.ts'), 'utf8');
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;
const focus = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`
);

function sheet(markup) {
  const dom = new JSDOM(`<!doctype html><html><body>${markup}</body></html>`);
  return {
    document: dom.window.document,
    element: dom.window.document.querySelector('.sheet'),
  };
}

const keysSheet = `
  <div class="sheet">
    <div class="sheet-head">
      <h2 class="sheet-title">Keys</h2>
      <button class="sheet-close" type="button">×</button>
    </div>
    <div class="sheet-body">
      <ul class="key-list">
        <li class="key-list-row">
          <span class="key-list-name">⇧Tab</span>
          <button class="sheet-row-action" type="button">Remove</button>
        </li>
      </ul>
      <form class="key-form">
        <div class="modifier-row" role="group">
          <label class="modifier">
            <input type="checkbox" id="saved-mod-shift">
            <span>Shift</span>
          </label>
        </div>
        <input class="text-input" type="text" id="saved-key-name">
        <button class="btn btn-primary" type="submit">Add</button>
      </form>
    </div>
  </div>
`;

test('the keys sheet focuses the name field, not a Remove button or checkbox', () => {
  const { document, element } = sheet(keysSheet);
  const target = focus.preferredFocusTarget(element);
  assert.equal(target.id, 'saved-key-name');
  target.focus();
  assert.equal(document.activeElement, target);
});

test('a disabled name field is skipped instead of a checkbox', () => {
  const markup = keysSheet
    .replace('id="saved-key-name"', 'id="saved-key-name" disabled')
    .replace('type="submit">Add', 'type="submit" disabled>Add');
  const { document, element } = sheet(markup);
  const target = focus.preferredFocusTarget(element);
  assert.equal(target.className, 'sheet-row-action');
  assert.equal(target.textContent, 'Remove');
  target.focus();
  assert.equal(document.activeElement, target);
});

test('the switch sheet focuses its filter before a config row', () => {
  const markup = `
    <div class="sheet">
      <div class="sheet-head"><button class="sheet-close" type="button">×</button></div>
      <div class="sheet-body">
        <input class="text-input" type="text" id="switch-filter">
        <button class="sheet-row" type="button">Claude</button>
      </div>
    </div>
  `;
  const { element } = sheet(markup);
  assert.equal(focus.preferredFocusTarget(element).id, 'switch-filter');
});

test('the actions sheet focuses its first safe row', () => {
  const markup = `
    <div class="sheet">
      <div class="sheet-head"><button class="sheet-close" type="button">×</button></div>
      <div class="sheet-body">
        <button class="sheet-row" type="button">Switch AI config</button>
        <button class="sheet-row is-danger" type="button">Close session</button>
      </div>
    </div>
  `;
  const { element } = sheet(markup);
  assert.equal(
    focus.preferredFocusTarget(element).textContent,
    'Switch AI config',
  );
});
