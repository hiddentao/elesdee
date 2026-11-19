import { describe, expect, it } from 'vitest';
import { closeSessionTab, openSessionTab } from './tabs';

describe('session tabs', () => {
  it('opens a session once and makes it active', () => {
    let tabs = openSessionTab({}, 'p1', 'a');
    tabs = openSessionTab(tabs, 'p1', 'b');
    tabs = openSessionTab(tabs, 'p1', 'a');

    expect(tabs.p1).toEqual({ open: ['a', 'b'], active: 'a' });
  });

  it('selects the next tab when the active tab closes', () => {
    let tabs = openSessionTab({}, 'p1', 'a');
    tabs = openSessionTab(tabs, 'p1', 'b');
    tabs = openSessionTab(tabs, 'p1', 'c');
    tabs = openSessionTab(tabs, 'p1', 'b');

    const next = closeSessionTab(tabs, 'p1', 'b');

    expect(next.p1).toEqual({ open: ['a', 'c'], active: 'c' });
  });

  it('selects the previous tab when the last tab closes', () => {
    let tabs = openSessionTab({}, 'p1', 'a');
    tabs = openSessionTab(tabs, 'p1', 'b');
    tabs = openSessionTab(tabs, 'p1', 'c');

    const next = closeSessionTab(tabs, 'p1', 'c');

    expect(next.p1).toEqual({ open: ['a', 'b'], active: 'b' });
  });

  it('clears the selection when the only tab closes', () => {
    const tabs = openSessionTab({}, 'p1', 'a');

    const next = closeSessionTab(tabs, 'p1', 'a');

    expect(next.p1).toEqual({ open: [], active: null });
  });

  it('keeps the active tab when another tab closes', () => {
    let tabs = openSessionTab({}, 'p1', 'a');
    tabs = openSessionTab(tabs, 'p1', 'b');
    tabs = openSessionTab(tabs, 'p1', 'c');
    tabs = openSessionTab(tabs, 'p1', 'a');

    const next = closeSessionTab(tabs, 'p1', 'c');

    expect(next.p1).toEqual({ open: ['a', 'b'], active: 'a' });
  });
});
