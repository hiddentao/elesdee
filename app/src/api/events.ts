import { API_BASE } from './base';
import type {
  AuxEvent,
  HistoryItem,
  HistoryStatus,
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
 * Streams questions and notifications. The browser reconnects on its own, so a
 * dropped stream does not lose the pending list; the app reloads it on mount.
 */
export function subscribeAux(
  onEvent: (event: AuxEvent) => void,
  onError?: () => void,
  onOpen?: () => void,
): Unsubscribe {
  const source = new EventSource(API_BASE + '/api/questions/events');

  source.onopen = () => {
    onOpen?.();
  };

  source.onmessage = (event) => {
    try {
      const payload = JSON.parse(event.data) as Partial<AuxEvent>;
      if (payload.type === 'question.updated' && payload.question) {
        onEvent({ type: 'question.updated', question: payload.question });
      } else if (payload.type === 'request.updated' && payload.request) {
        onEvent({ type: 'request.updated', request: payload.request });
      } else if (payload.type === 'sharedfile.created' && payload.file) {
        onEvent({ type: 'sharedfile.created', file: payload.file });
      } else if (payload.type === 'notification' && payload.notification) {
        onEvent({
          type: 'notification',
          notification: payload.notification,
        });
      } else if (payload.type === 'provider.updated' && payload.provider) {
        onEvent({ type: 'provider.updated', provider: payload.provider });
      } else if (
        payload.type === 'history.appended' &&
        payload.sessionId &&
        payload.item
      ) {
        onEvent({
          type: 'history.appended',
          sessionId: payload.sessionId,
          item: payload.item,
        });
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

type Frame = Record<string, unknown>;

/**
 * Streams one sequenced session log. The stream reconnects on its own and asks
 * for every frame after the last sequence it has seen, so no frame is lost.
 * A frame without a sequence, such as a status notice, leaves the position
 * unchanged.
 */
function subscribeFrames(
  url: (after: number) => string,
  startAfter: number,
  onFrame: (frame: Frame) => void,
  onError?: () => void,
  onOpen?: () => void,
  isClosed?: () => boolean,
): Unsubscribe {
  let stopped = false;
  let lastSeq = startAfter;
  let attempts = 0;
  let source: EventSource | null = null;
  let timer: number | undefined;

  const connect = () => {
    if (stopped) {
      return;
    }

    source = new EventSource(url(lastSeq));

    source.onopen = () => {
      attempts = 0;
      onOpen?.();
    };

    source.onmessage = (event) => {
      try {
        const frame = JSON.parse(event.data) as Frame;
        const seq = typeof frame.seq === 'number' ? frame.seq : null;
        if (seq !== null && seq <= lastSeq) {
          return;
        }
        if (seq !== null) {
          lastSeq = seq;
        }
        onFrame(frame);
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

      // A closed session has no live stream. The first connection loads the
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

/**
 * Streams terminal output for one session.
 */
export function subscribeOutput(
  sessionId: string,
  afterSeq: number,
  onChunk: (seq: number, data: string, gap?: OutputGap) => void,
  onError?: () => void,
  onOpen?: () => void,
  isClosed?: () => boolean,
): Unsubscribe {
  return subscribeFrames(
    (after) =>
      API_BASE +
      `/api/sessions/${encodeURIComponent(sessionId)}/output?after=${after}`,
    afterSeq,
    (frame) => {
      const chunk = frame as Partial<OutputChunk>;
      if (typeof chunk.seq !== 'number' || typeof chunk.data !== 'string') {
        return;
      }
      if (chunk.gap) {
        onChunk(chunk.seq, chunk.data, chunk.gap);
      } else {
        onChunk(chunk.seq, chunk.data);
      }
    },
    onError,
    onOpen,
    isClosed,
  );
}

/**
 * Streams the structured conversation for one session.
 */
export function subscribeHistory(
  sessionId: string,
  afterSeq: number,
  onStatus: (status: HistoryStatus) => void,
  onItem: (item: HistoryItem) => void,
  onError?: () => void,
  onOpen?: () => void,
  isClosed?: () => boolean,
): Unsubscribe {
  return subscribeFrames(
    (after) =>
      API_BASE +
      `/api/sessions/${encodeURIComponent(sessionId)}/history?after=${after}`,
    afterSeq,
    (frame) => {
      if (frame.type === 'history.state') {
        onStatus(frame as unknown as HistoryStatus);
        return;
      }
      const item = frame as unknown as HistoryItem;
      if (
        typeof item.seq !== 'number' ||
        typeof item.id !== 'string' ||
        typeof item.text !== 'string'
      ) {
        return;
      }
      onItem(item);
    },
    onError,
    onOpen,
    isClosed,
  );
}
