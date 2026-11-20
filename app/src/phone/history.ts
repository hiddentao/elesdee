import type {
  Client,
  HistoryItem,
  HistoryState,
  SessionKind,
} from '../api/types';
import type { Translate } from '../i18n';

/**
 * A structured conversation comes from the client's own event interface. A
 * terminal session and Antigravity have none, so they stay terminal-first.
 */
export function historySupported(
  client: Client | null,
  kind: SessionKind,
): boolean {
  return kind === 'agent' && client !== null && client !== 'Antigravity';
}

/**
 * Keeps one item for each id. An update arrives as a new version with a
 * higher sequence, so an older version never replaces a newer one.
 */
export function mergeHistory(
  items: HistoryItem[],
  incoming: HistoryItem,
): HistoryItem[] {
  const index = items.findIndex((item) => item.id === incoming.id);
  if (index === -1) {
    return [...items, incoming];
  }
  if (items[index].seq >= incoming.seq) {
    return items;
  }
  const next = items.slice();
  next[index] = incoming;
  return next;
}

export function toolStateLabel(state: HistoryState, t: Translate): string | null {
  switch (state) {
    case 'running':
    case 'streaming':
      return t('phone.toolRunning');
    case 'failed':
      return t('phone.toolFailed');
    case 'done':
      return t('phone.toolDone');
    default:
      return null;
  }
}
