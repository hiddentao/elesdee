import type { Session } from './types';

export interface OutputGap {
  after: number;
  oldestSeq: number;
  newestSeq: number;
}

export interface OutputEvent {
  seq: number;
  data: string;
  gap?: OutputGap;
}

export interface OutputState {
  text: string;
  lastSeq: number;
  dropped: boolean;
}

export const MAX_OUTPUT_CHARS = 200_000;

/**
 * Replaces a session in the list, or appends it when it is new. The order of
 * the other sessions does not change, so the list does not jump on an update.
 */
export function mergeSession(
  sessions: readonly Session[],
  session: Session,
): Session[] {
  const index = sessions.findIndex((current) => current.id === session.id);
  if (index < 0) {
    return [...sessions, session];
  }
  const next = sessions.slice();
  next[index] = session;
  return next;
}

/**
 * Adds one output event to the visible text. A sequence that was already seen
 * is ignored, so a replay after reconnect cannot duplicate text.
 */
export function appendOutput(
  state: OutputState,
  event: OutputEvent,
  limit = MAX_OUTPUT_CHARS,
): OutputState {
  if (!Number.isFinite(event.seq) || event.seq <= state.lastSeq) {
    return state;
  }
  let text = state.text + event.data;
  let dropped = state.dropped || event.gap !== undefined;
  if (text.length > limit) {
    text = text.slice(-limit);
    dropped = true;
  }
  return { text, lastSeq: event.seq, dropped };
}

/** Reads a session update from an event value, or returns null. */
export function sessionFromEvent(value: unknown): Session | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const event = value as { type?: unknown; session?: unknown };
  if (event.type !== 'session.updated') {
    return null;
  }
  return asSession(event.session);
}

/** Reads an output event value, or returns null. */
export function outputFromEvent(value: unknown): OutputEvent | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const event = value as { seq?: unknown; data?: unknown; gap?: unknown };
  if (typeof event.seq !== 'number' || typeof event.data !== 'string') {
    return null;
  }
  const gap = asGap(event.gap);
  return gap ? { seq: event.seq, data: event.data, gap } : {
    seq: event.seq,
    data: event.data,
  };
}

function asSession(value: unknown): Session | null {
  if (typeof value !== 'object' || value === null) {
    return null;
  }
  const session = value as Partial<Session>;
  if (
    typeof session.id !== 'string' ||
    typeof session.name !== 'string' ||
    typeof session.status !== 'string'
  ) {
    return null;
  }
  return session as Session;
}

function asGap(value: unknown): OutputGap | undefined {
  if (typeof value !== 'object' || value === null) {
    return undefined;
  }
  const gap = value as Partial<OutputGap>;
  if (
    typeof gap.after !== 'number' ||
    typeof gap.oldestSeq !== 'number' ||
    typeof gap.newestSeq !== 'number'
  ) {
    return undefined;
  }
  return {
    after: gap.after,
    oldestSeq: gap.oldestSeq,
    newestSeq: gap.newestSeq,
  };
}
