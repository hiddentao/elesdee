import { API_BASE } from './base';
import type {
  OutputChunk,
  OutputGap,
  Session,
  SessionUpdatedEvent,
} from './types';

export type Unsubscribe = () => void;

export function subscribeEvents(
  onSession: (session: Session) => void,
  onError?: () => void,
  onOpen?: () => void,
): Unsubscribe {
  const source = new EventSource(API_BASE + '/api/events');

  source.onopen = () => {
    onOpen?.();
  };

  source.onmessage = (event) => {
    try {
      const payload = JSON.parse(event.data) as Partial<SessionUpdatedEvent>;
      if (payload.type === 'session.updated' && payload.session) {
        onSession(payload.session);
      }
    } catch {
      // Ignore malformed events and keep the stream open.
    }
  };

  source.onerror = () => {
    onError?.();
  };

  return () => {
    source.close();
  };
}

/**
 * Streams terminal output for one session. The stream reconnects on its own and
 * asks for every chunk after the last sequence it has seen, so no text is lost.
 */
export function subscribeOutput(
  sessionId: string,
  afterSeq: number,
  onChunk: (seq: number, data: string, gap?: OutputGap) => void,
  onError?: () => void,
  onOpen?: () => void,
  isClosed?: () => boolean,
): Unsubscribe {
  let stopped = false;
  let lastSeq = afterSeq;
  let attempts = 0;
  let source: EventSource | null = null;
  let timer: number | undefined;

  const connect = () => {
    if (stopped) {
      return;
    }

    source = new EventSource(
      API_BASE +
        `/api/sessions/${encodeURIComponent(sessionId)}/output?after=${lastSeq}`,
    );

    source.onopen = () => {
      attempts = 0;
      onOpen?.();
    };

    source.onmessage = (event) => {
      try {
        const chunk = JSON.parse(event.data) as Partial<OutputChunk>;
        if (typeof chunk.seq !== 'number' || typeof chunk.data !== 'string') {
          return;
        }
        if (chunk.seq <= lastSeq) {
          return;
        }
        lastSeq = chunk.seq;
        if (chunk.gap) {
          onChunk(chunk.seq, chunk.data, chunk.gap);
        } else {
          onChunk(chunk.seq, chunk.data);
        }
      } catch {
        // Ignore malformed events and keep the stream open.
      }
    };

    source.onerror = () => {
      if (stopped) {
        return;
      }
      source?.close();
      source = null;

      // A closed session has no live output. The first connection loads the
      // log; the end of that stream is not a failure and needs no retry.
      if (isClosed?.()) {
        return;
      }

      onError?.();
      const delay = Math.min(500 * 2 ** attempts, 5000);
      attempts += 1;
      timer = window.setTimeout(connect, delay);
    };
  };

  connect();

  return () => {
    stopped = true;
    if (timer !== undefined) {
      window.clearTimeout(timer);
    }
    source?.close();
  };
}
