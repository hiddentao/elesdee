import { afterEach, describe, expect, it } from 'vitest';
import { focusSessionRow } from './sessionRowFocus';

function addRow(sessionId: string): HTMLButtonElement {
  const row = document.createElement('button');
  row.dataset.sessionId = sessionId;
  document.body.append(row);
  return row;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('focusSessionRow', () => {
  it('focuses the row for the session', () => {
    const other = addRow('other');
    const wanted = addRow('wanted');

    focusSessionRow('wanted');

    expect(wanted).toHaveFocus();
    expect(other).not.toHaveFocus();
  });

  it('focuses the fallback when the row is missing', () => {
    addRow('other');
    const fallback = document.createElement('div');
    fallback.className = 'session-list';
    fallback.tabIndex = 0;
    document.body.append(fallback);

    focusSessionRow('missing', '.session-list');

    expect(fallback).toHaveFocus();
  });

  it('leaves focus alone when the row and fallback are missing', () => {
    addRow('other');

    focusSessionRow('missing');

    expect(document.activeElement).toBe(document.body);
  });
});
