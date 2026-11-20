import { describe, expect, it } from 'vitest';
import type { HistoryItem } from '../api/types';
import { historySupported, mergeHistory } from './history';

function item(overrides: Partial<HistoryItem> = {}): HistoryItem {
  return {
    seq: 1,
    id: 'item-1',
    kind: 'reply',
    text: 'Hello',
    state: 'done',
    ...overrides,
  };
}

describe('history merge', () => {
  it('appends a new item', () => {
    const items = [item()];
    const merged = mergeHistory(items, item({ seq: 2, id: 'item-2', kind: 'tool' }));
    expect(merged).toHaveLength(2);
    expect(merged[1].id).toBe('item-2');
  });

  it('replaces an item with a newer version', () => {
    const merged = mergeHistory(
      [item({ seq: 1, text: 'Hel', state: 'streaming' })],
      item({ seq: 2, text: 'Hello', state: 'done' }),
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].seq).toBe(2);
    expect(merged[0].text).toBe('Hello');
    expect(merged[0].state).toBe('done');
  });

  it('ignores an older or repeated version', () => {
    const items = [item({ seq: 3, text: 'newer' })];
    expect(mergeHistory(items, item({ seq: 2, text: 'older' }))).toBe(items);
    expect(mergeHistory(items, item({ seq: 3, text: 'newer' }))).toBe(items);
  });
});

describe('structured sources', () => {
  it('covers agent clients except Antigravity', () => {
    expect(historySupported('Claude Code', 'agent')).toBe(true);
    expect(historySupported('Codex', 'agent')).toBe(true);
    expect(historySupported('OpenCode 2', 'agent')).toBe(true);
    expect(historySupported('Antigravity', 'agent')).toBe(false);
  });

  it('never covers a terminal session', () => {
    expect(historySupported(null, 'terminal')).toBe(false);
    expect(historySupported('Claude Code', 'terminal')).toBe(false);
  });
});
