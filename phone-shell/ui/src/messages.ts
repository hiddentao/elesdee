import type {
  HistoryItem,
  HistoryKind,
  HistoryState,
  HistoryStatus,
} from '../../../app/src/api/types';
import type { MessageKey } from './i18n';

const KINDS: readonly HistoryKind[] = ['prompt', 'reply', 'tool'];
const STATES: readonly HistoryState[] = [
  'streaming',
  'running',
  'done',
  'failed',
];

/**
 * Reads one conversation item from a history stream, or returns null when the
 * value does not carry the minimal item fields.
 */
export function historyItemFromEvent(value: unknown): HistoryItem | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const item = value as Partial<HistoryItem>;
  if (
    typeof item.seq !== 'number' ||
    !Number.isFinite(item.seq) ||
    typeof item.id !== 'string' ||
    item.id.length === 0 ||
    typeof item.text !== 'string' ||
    !KINDS.includes(item.kind as HistoryKind) ||
    !STATES.includes(item.state as HistoryState)
  ) {
    return null;
  }
  return {
    seq: item.seq,
    id: item.id,
    kind: item.kind as HistoryKind,
    text: item.text,
    state: item.state as HistoryState,
    ...(typeof item.tool === 'string' ? { tool: item.tool } : {}),
  };
}

/** Reads the first event on a history stream, or returns null for an item. */
export function historyStatusFromEvent(value: unknown): HistoryStatus | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const status = value as Partial<HistoryStatus>;
  if (status.type !== 'history.state' || typeof status.structured !== 'boolean') {
    return null;
  }
  return {
    type: 'history.state',
    structured: status.structured,
    client: typeof status.client === 'string' ? status.client : null,
  };
}

/**
 * Adds or removes one tool row from the open set. The shell keeps one set for
 * the active session, and the set is cleared when the session changes.
 */
export function toggleExpanded(
  expanded: ReadonlySet<string>,
  id: string,
): Set<string> {
  const next = new Set(expanded);
  if (next.has(id)) {
    next.delete(id);
  } else {
    next.add(id);
  }
  return next;
}

/** The catalog key for a tool row's state label, or null when it has none. */
export function toolStateKey(state: HistoryState): MessageKey | null {
  switch (state) {
    case 'running':
    case 'streaming':
      return 'messages.toolRunning';
    case 'failed':
      return 'messages.toolFailed';
    case 'done':
      return 'messages.toolDone';
    default:
      return null;
  }
}
