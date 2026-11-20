import { transport } from './transport';
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
  return transport.subscribe('events', undefined, {
    onEvent: (value) => {
      const payload = value as Partial<SessionUpdatedEvent>;
      if (payload.type === 'session.updated' && payload.session) {
        onSession(payload.session);
      }
    },
    onError,
    onOpen,
  });
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
  return transport.subscribe('aux', undefined, {
    onEvent: (value) => {
      const payload = value as Partial<AuxEvent>;
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
    },
    onError,
    onOpen,
  });
}

/**
 * Streams terminal output for one session.
 *
 * The transport replays from the last sequence it has seen. The browser uses
 * EventSource and the shell bridge uses the link output subscription; both
 * send each frame once.
 */
export function subscribeOutput(
  sessionId: string,
  afterSeq: number,
  onChunk: (seq: number, data: string, gap?: OutputGap) => void,
  onError?: () => void,
  onOpen?: () => void,
  isClosed?: () => boolean,
): Unsubscribe {
  return transport.subscribe(
    'output',
    { sessionId, after: afterSeq },
    {
      onEvent: (frame) => {
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
    },
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
  return transport.subscribe(
    'history',
    { sessionId, after: afterSeq },
    {
      onEvent: (frame) => {
        if (!frame || typeof frame !== 'object') {
          return;
        }
        if ((frame as { type?: unknown }).type === 'history.state') {
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
    },
  );
}
