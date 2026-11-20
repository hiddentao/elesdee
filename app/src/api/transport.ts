import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import { API_BASE } from './base';
import type { UploadedFile } from './types';

/** A failure with the HTTP-shaped status that the components already read. */
export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/** The marker that the shell writes before the package scripts run. */
export interface PhoneMarker {
  computerId: string;
  computerName: string;
  bridgeVersion: number;
  protocolVersion: number;
}

/** How the package reaches the computer. */
export type TransportKind = 'http' | 'phone';

/** One live channel that a subscribe call can open. */
export type TransportChannel = 'events' | 'aux' | 'output' | 'history';

/** The handlers for one open stream. */
export interface SubscribeHandlers {
  onEvent: (value: unknown) => void;
  onError?: () => void;
  onOpen?: () => void;
  onEnd?: () => void;
  /** True when the session is closed and no retry is needed. */
  isClosed?: () => boolean;
}

/**
 * One event that the shell may turn into a local notification.
 *
 * The values are facts from the event stream; the shell applies the notice
 * rules and the suppression rules.
 */
export type PhoneNoticeEvent =
  | {
      kind: 'session';
      sessionId: string;
      sessionName: string;
      status: string;
      waitingFor: string | null;
      previousWaitingFor: string | null;
    }
  | {
      kind: 'question';
      sessionId: string;
      sessionName: string;
      status: string;
    }
  | {
      kind: 'request';
      sessionId: string;
      sessionName: string;
      status: string;
    }
  | {
      kind: 'notification';
      sessionId: string;
      sessionName: string;
      notificationKind: string;
      message: string;
    };

/** The text that the shell picks from when a notice is due. */
export interface PhoneNoticeTexts {
  waitingTitle: string;
  requestTitle: string;
  requestBody: string;
  permissionBody: string;
  questionBody: string;
  inputBody: string;
  doneTitle: string;
  doneBody: string;
  messageTitle: string;
  messageBody: string;
}

/** One request to raise a local notification. */
export interface PhoneNotice {
  event: PhoneNoticeEvent;
  screenSessionId: string | null;
  appVisible: boolean;
  texts: PhoneNoticeTexts;
}

/** One way to reach the computer. */
export interface Transport {
  readonly kind: TransportKind;
  request<T>(
    method: string,
    params?: unknown,
    options?: { signal?: AbortSignal },
  ): Promise<T>;
  subscribe(
    channel: TransportChannel,
    params: unknown,
    handlers: SubscribeHandlers,
  ): () => void;
  upload(sessionId: string, name: string, data: Blob): Promise<UploadedFile>;
  readImage(
    sessionId: string,
    path: string,
    options?: { signal?: AbortSignal },
  ): Promise<Blob>;
  notify(notice: PhoneNotice): void;
  notificationClick(listener: (sessionId: string) => void): () => void;
}

interface EventSourceLike {
  onopen: (() => void) | null;
  onmessage: ((event: { data: string }) => void) | null;
  onerror: (() => void) | null;
  close(): void;
}

export type EventSourceFn = (url: string) => EventSourceLike;
type FetchFn = (input: string, init?: RequestInit) => Promise<Response>;
export type InvokeFn = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;
export type ListenFn = <T>(
  event: string,
  handler: (payload: T) => void,
) => Promise<() => void>;

/** The platform operations, injectable so both transports have tests. */
export interface TransportDeps {
  fetch?: FetchFn;
  eventSource?: EventSourceFn;
  invoke?: InvokeFn;
  listen?: ListenFn;
}

const MAX_UPLOAD_BYTES = 25 * 1024 * 1024;
const MAX_RETRY_MS = 5000;

const STATUS_BY_CODE: Record<string, number> = {
  bad_request: 400,
  unauthorized: 401,
  not_paired: 401,
  revoked: 401,
  forbidden: 403,
  not_found: 404,
  conflict: 409,
  too_large: 413,
  unsupported: 415,
  unprocessable: 422,
  rate_limited: 429,
  timeout: 408,
  not_connected: 503,
  unavailable: 503,
  internal: 500,
};

function defaultFetch(input: string, init?: RequestInit): Promise<Response> {
  return fetch(input, init);
}

function defaultEventSource(url: string): EventSourceLike {
  return new EventSource(url) as unknown as EventSourceLike;
}

const defaultInvoke = tauriInvoke as InvokeFn;
const defaultListen = tauriListen as ListenFn;

/** Reads the marker that the shell writes before the package scripts run. */
export function phoneMarker(): PhoneMarker | null {
  if (typeof window === 'undefined') {
    return null;
  }
  const raw = (window as Window & { __ELESDEE_PHONE__?: unknown })
    .__ELESDEE_PHONE__;
  if (!raw || typeof raw !== 'object') {
    return null;
  }
  const value = raw as Partial<PhoneMarker>;
  if (typeof value.computerId !== 'string' || value.computerId === '') {
    return null;
  }
  return {
    computerId: value.computerId,
    computerName:
      typeof value.computerName === 'string' ? value.computerName : '',
    bridgeVersion:
      typeof value.bridgeVersion === 'number' ? value.bridgeVersion : 0,
    protocolVersion:
      typeof value.protocolVersion === 'number' ? value.protocolVersion : 0,
  };
}

export async function errorMessage(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as { error?: unknown };
    if (typeof payload.error === 'string' && payload.error.trim() !== '') {
      return payload.error;
    }
  } catch {
    // The response carries no JSON body.
  }
  return `The request failed with status ${response.status}.`;
}

/** Sends one local HTTP request and unwraps its JSON body. */
export async function httpRequest<T>(
  path: string,
  init?: RequestInit,
  deps: TransportDeps = {},
): Promise<T> {
  const fetchFn = deps.fetch ?? defaultFetch;
  const response = await fetchFn(API_BASE + path, init);
  if (!response.ok) {
    throw new ApiError(response.status, await errorMessage(response));
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

function paramsObject(params: unknown): Record<string, unknown> {
  if (params === undefined || params === null) {
    return {};
  }
  if (typeof params !== 'object' || Array.isArray(params)) {
    throw new ApiError(400, 'The method parameters must be an object.');
  }
  return params as Record<string, unknown>;
}

function required(object: Record<string, unknown>, key: string): string {
  const value = object[key];
  if (typeof value !== 'string' || value === '') {
    throw new ApiError(400, `The ${key} parameter is required.`);
  }
  return value;
}

function optional(
  object: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = object[key];
  return typeof value === 'string' && value !== '' ? value : undefined;
}

function segment(value: string): string {
  if (!/^[A-Za-z0-9._-]+$/.test(value)) {
    throw new ApiError(400, 'The id is not valid.');
  }
  return value;
}

function encodeQuery(value: string): string {
  return encodeURIComponent(value).replace(/[!'()*]/g, (character) =>
    `%${character.charCodeAt(0).toString(16).toUpperCase()}`,
  );
}

function queryPath(
  base: string,
  pairs: [string, string | undefined][],
): string {
  const query = pairs
    .filter(([, value]) => value !== undefined)
    .map(([name, value]) => `${encodeQuery(name)}=${encodeQuery(value ?? '')}`)
    .join('&');
  return query === '' ? base : `${base}?${query}`;
}

interface HttpRoute {
  path: string;
  init?: RequestInit;
}

function getRoute(path: string): HttpRoute {
  return { path };
}

function postRoute(path: string, body: unknown): HttpRoute {
  return {
    path,
    init: {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    },
  };
}

/**
 * Maps one allowlisted link method to the local route that serves it.
 *
 * The mapping mirrors the core link dispatcher, so the browser layout and the
 * shell bridge share one method contract.
 */
export function routeFor(method: string, params?: unknown): HttpRoute {
  const object = paramsObject(params);
  switch (method) {
    case 'bootstrap':
      return getRoute('/api/bootstrap');
    case 'health':
      return getRoute('/api/health');
    case 'projects.list':
      return getRoute(
        queryPath('/api/projects', [['q', optional(object, 'q')]]),
      );
    case 'branches.list':
      return getRoute(
        queryPath('/api/branches', [
          ['folder', required(object, 'folder')],
          ['q', optional(object, 'q')],
        ]),
      );
    case 'providers.list':
      return getRoute('/api/providers');
    case 'accounts.list':
      return getRoute(
        queryPath('/api/accounts', [['q', optional(object, 'q')]]),
      );
    case 'sessions.create':
      return postRoute('/api/sessions', object);
    case 'sessions.close':
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/close`,
        {},
      );
    case 'sessions.input':
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/input`,
        { data: required(object, 'data') },
      );
    case 'sessions.permission': {
      const decision = required(object, 'decision');
      if (decision !== 'allow' && decision !== 'deny') {
        throw new ApiError(400, 'The permission decision is not valid.');
      }
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/permission`,
        { requestId: required(object, 'requestId'), decision },
      );
    }
    case 'sessions.account':
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/account`,
        { accountId: required(object, 'accountId') },
      );
    case 'sessions.config':
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/config`,
        { configId: required(object, 'configId') },
      );
    case 'sessions.resume':
      return postRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/resume`,
        {},
      );
    case 'questions.list':
      return getRoute('/api/questions');
    case 'questions.answer':
      return postRoute(
        `/api/questions/${segment(required(object, 'id'))}/answer`,
        { answer: required(object, 'answer') },
      );
    case 'sessionRequests.approve':
      return postRoute(
        `/api/session-requests/${segment(required(object, 'id'))}/approve`,
        {
          allowFurther:
            typeof object.allowFurther === 'boolean'
              ? object.allowFurther
              : false,
        },
      );
    case 'sessionRequests.deny':
      return postRoute(
        `/api/session-requests/${segment(required(object, 'id'))}/deny`,
        {},
      );
    case 'files.list':
      return getRoute(
        queryPath(
          `/api/sessions/${segment(required(object, 'sessionId'))}/files`,
          [['path', optional(object, 'path')]],
        ),
      );
    case 'files.read':
      return getRoute(
        queryPath(
          `/api/sessions/${segment(required(object, 'sessionId'))}/file`,
          [['path', required(object, 'path')]],
        ),
      );
    case 'diff.read':
      return getRoute(
        `/api/sessions/${segment(required(object, 'sessionId'))}/changes`,
      );
    case 'demoServers.list':
      return getRoute('/api/demo-servers');
    case 'sessions.upload':
    case 'files.image':
      throw new ApiError(400, 'Use the upload or image call for this method.');
    case 'events.subscribe':
    case 'output.subscribe':
    case 'history.subscribe':
      throw new ApiError(400, 'Use a subscription for this method.');
    default:
      throw new ApiError(403, `The method is not allowed: ${method}`);
  }
}

function parseFrame(data: string): unknown {
  try {
    return JSON.parse(data) as unknown;
  } catch {
    return null;
  }
}

function frameSeq(value: unknown): number | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const seq = (value as { seq?: unknown }).seq;
  return typeof seq === 'number' ? seq : null;
}

function streamParams(params: unknown): {
  sessionId: string;
  after: number;
} {
  const object = paramsObject(params);
  return {
    sessionId: segment(required(object, 'sessionId')),
    after:
      typeof object.after === 'number' && object.after >= 0
        ? object.after
        : 0,
  };
}

function subscribeHttpEvents(
  channel: 'events' | 'aux',
  handlers: SubscribeHandlers,
  deps: TransportDeps,
): () => void {
  const create = deps.eventSource ?? defaultEventSource;
  const path = channel === 'events' ? '/api/events' : '/api/questions/events';
  const source = create(API_BASE + path);
  source.onopen = () => {
    handlers.onOpen?.();
  };
  source.onmessage = (event) => {
    const value = parseFrame(event.data);
    if (value !== null) {
      handlers.onEvent(value);
    }
  };
  source.onerror = () => {
    handlers.onError?.();
  };
  return () => {
    source.close();
  };
}

function subscribeHttpFrames(
  channel: 'output' | 'history',
  params: unknown,
  handlers: SubscribeHandlers,
  deps: TransportDeps,
): () => void {
  const create = deps.eventSource ?? defaultEventSource;
  const { sessionId, after } = streamParams(params);
  let stopped = false;
  let lastSeq = after;
  let attempts = 0;
  let source: EventSourceLike | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const connect = () => {
    if (stopped) {
      return;
    }
    const path =
      channel === 'output'
        ? `/api/sessions/${encodeURIComponent(sessionId)}/output?after=${lastSeq}`
        : `/api/sessions/${encodeURIComponent(sessionId)}/history?after=${lastSeq}`;
    source = create(API_BASE + path);
    source.onopen = () => {
      attempts = 0;
      handlers.onOpen?.();
    };
    source.onmessage = (event) => {
      const frame = parseFrame(event.data);
      if (frame === null) {
        return;
      }
      const seq = frameSeq(frame);
      if (seq !== null && seq <= lastSeq) {
        return;
      }
      if (seq !== null) {
        lastSeq = seq;
      }
      handlers.onEvent(frame);
    };
    source.onerror = () => {
      if (stopped) {
        return;
      }
      source?.close();
      source = null;
      // A closed session has no live stream. The first connection loads the
      // log; the end of that stream is not a failure and needs no retry.
      if (handlers.isClosed?.()) {
        return;
      }
      handlers.onError?.();
      const delay = Math.min(500 * 2 ** attempts, MAX_RETRY_MS);
      attempts += 1;
      timer = setTimeout(connect, delay);
    };
  };

  connect();

  return () => {
    stopped = true;
    if (timer !== undefined) {
      clearTimeout(timer);
    }
    source?.close();
  };
}

/** Builds the browser transport: fetch and EventSource. */
export function createHttpTransport(deps: TransportDeps = {}): Transport {
  const fetchFn = deps.fetch ?? defaultFetch;
  return {
    kind: 'http',
    request<T>(
      method: string,
      params?: unknown,
      options?: { signal?: AbortSignal },
    ): Promise<T> {
      const route = routeFor(method, params);
      return httpRequest<T>(
        route.path,
        {
          ...route.init,
          ...(options?.signal ? { signal: options.signal } : {}),
        },
        deps,
      );
    },
    subscribe(channel, params, handlers) {
      return channel === 'events' || channel === 'aux'
        ? subscribeHttpEvents(channel, handlers, deps)
        : subscribeHttpFrames(channel, params, handlers, deps);
    },
    upload(sessionId, name, data) {
      return httpRequest<UploadedFile>(
        `/api/sessions/${segment(sessionId)}/upload`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/octet-stream',
            'X-El-Esdee-Filename': encodeURIComponent(name),
          },
          body: data,
        },
        deps,
      );
    },
    async readImage(sessionId, path, options) {
      const response = await fetchFn(
        `${API_BASE}/api/sessions/${segment(sessionId)}/image${queryPath('', [['path', path]])}`,
        options?.signal ? { signal: options.signal } : undefined,
      );
      if (!response.ok) {
        throw new ApiError(response.status, await errorMessage(response));
      }
      return response.blob();
    },
    notify() {
      // The browser layout raises no system notification.
    },
    notificationClick() {
      return () => {};
    },
  };
}

interface BridgeSubscriptionValue {
  id: string;
  event?: unknown;
  error?: { code?: unknown; message?: unknown };
  ended?: boolean;
}

interface BridgeSub {
  handlers: SubscribeHandlers;
  stopped: boolean;
}

function bridgeError(error: unknown): ApiError {
  const value =
    typeof error === 'object' && error !== null
      ? (error as { code?: unknown; message?: unknown })
      : {};
  const code = typeof value.code === 'string' ? value.code : 'internal';
  const message =
    typeof value.message === 'string' && value.message !== ''
      ? value.message
      : 'The request failed.';
  return new ApiError(STATUS_BY_CODE[code] ?? 500, message);
}

async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = '';
  const chunk = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk));
  }
  return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/** Builds the phone transport: the shell bridge over the link connection. */
export function createBridgeTransport(
  marker: PhoneMarker,
  deps: TransportDeps = {},
): Transport {
  const invokeFn = deps.invoke ?? defaultInvoke;
  const listenFn = deps.listen ?? defaultListen;
  const subs = new Map<string, BridgeSub>();
  const eventHandlers = new Set<SubscribeHandlers>();
  let listener: Promise<() => void> | null = null;
  let eventClose: (() => void) | null = null;
  let nextId = 0;

  const ensureListener = (): Promise<() => void> => {
    if (!listener) {
      listener = listenFn<BridgeSubscriptionValue>(
        'phone:subscription',
        (payload) => {
          const sub = subs.get(payload.id);
          if (!sub) {
            return;
          }
          if (payload.event !== undefined) {
            sub.handlers.onEvent(payload.event);
          }
          if (payload.error) {
            sub.handlers.onError?.();
          }
          if (payload.ended) {
            sub.handlers.onEnd?.();
          }
        },
      ).catch((error: unknown) => {
        listener = null;
        throw error;
      });
    }
    return listener;
  };

  const unsubscribe = (id: string): void => {
    void invokeFn('phone_unsubscribe', { id }).catch(() => {
      // The subscription may already have stopped.
    });
  };

  const open = (
    id: string,
    channel: 'events' | 'output' | 'history',
    params: unknown,
    handlers: SubscribeHandlers,
  ): (() => void) => {
    const sub: BridgeSub = { handlers, stopped: false };
    subs.set(id, sub);
    void (async () => {
      try {
        await ensureListener();
        if (sub.stopped) {
          return;
        }
        await invokeFn('phone_subscribe', {
          computerId: marker.computerId,
          id,
          channel,
          params: params ?? null,
        });
        if (sub.stopped) {
          unsubscribe(id);
          return;
        }
        handlers.onOpen?.();
      } catch {
        subs.delete(id);
        if (!sub.stopped) {
          handlers.onError?.();
        }
      }
    })();
    return () => {
      sub.stopped = true;
      subs.delete(id);
      unsubscribe(id);
    };
  };

  const subscribeEvents = (handlers: SubscribeHandlers): (() => void) => {
    eventHandlers.add(handlers);
    if (!eventClose) {
      eventClose = open('package:events', 'events', undefined, {
        onEvent: (value) => {
          eventHandlers.forEach((entry) => entry.onEvent(value));
        },
        onError: () => {
          eventHandlers.forEach((entry) => entry.onError?.());
        },
        onOpen: () => {
          eventHandlers.forEach((entry) => entry.onOpen?.());
        },
        onEnd: () => {
          eventHandlers.forEach((entry) => entry.onEnd?.());
        },
      });
    }
    return () => {
      eventHandlers.delete(handlers);
      if (eventHandlers.size === 0 && eventClose) {
        eventClose();
        eventClose = null;
      }
    };
  };

  return {
    kind: 'phone',
    async request<T>(
      method: string,
      params?: unknown,
    ): Promise<T> {
      try {
        return await invokeFn<T>('phone_request', {
          computerId: marker.computerId,
          method,
          params: params ?? null,
        });
      } catch (error) {
        throw bridgeError(error);
      }
    },
    subscribe(channel, params, handlers) {
      if (channel === 'events' || channel === 'aux') {
        return subscribeEvents(handlers);
      }
      nextId += 1;
      return open(
        `package:${channel}:${nextId}`,
        channel,
        params,
        handlers,
      );
    },
    async upload(sessionId, name, data) {
      if (data.size > MAX_UPLOAD_BYTES) {
        throw new ApiError(413, 'The file is over the upload limit.');
      }
      try {
        return await invokeFn<UploadedFile>('phone_upload', {
          computerId: marker.computerId,
          sessionId,
          name,
          data: await blobToBase64(data),
        });
      } catch (error) {
        throw bridgeError(error);
      }
    },
    async readImage(sessionId, path) {
      let payload: { contentType?: unknown; data?: unknown };
      try {
        payload = await invokeFn<{ contentType?: unknown; data?: unknown }>(
          'phone_read_image',
          { computerId: marker.computerId, sessionId, path },
        );
      } catch (error) {
        throw bridgeError(error);
      }
      if (typeof payload.data !== 'string') {
        throw new ApiError(500, 'The image bytes are missing.');
      }
      const contentType =
        typeof payload.contentType === 'string' && payload.contentType !== ''
          ? payload.contentType
          : 'application/octet-stream';
      const source = base64ToBytes(payload.data);
      const buffer = new ArrayBuffer(source.byteLength);
      new Uint8Array(buffer).set(source);
      return new Blob([buffer], { type: contentType });
    },
    notify(notice) {
      void invokeFn('phone_notification_event', {
        computerId: marker.computerId,
        event: notice.event,
        screenSessionId: notice.screenSessionId,
        appVisible: notice.appVisible,
        texts: notice.texts,
      }).catch(() => {
        // A refused or failed notification never blocks the event stream.
      });
    },
    notificationClick(listenerFn) {
      let stopped = false;
      let unlisten: (() => void) | null = null;
      void listenFn<{ computerId: string; sessionId: string }>(
        'phone:notification-click',
        (target) => {
          if (target.computerId === marker.computerId) {
            listenerFn(target.sessionId);
          }
        },
      )
        .then((stop) => {
          if (stopped) {
            stop();
          } else {
            unlisten = stop;
          }
        })
        .catch(() => {
          // Without the shell event, a tap cannot open the session.
        });
      return () => {
        stopped = true;
        unlisten?.();
      };
    },
  };
}

/** Chooses the transport from the marker that the shell writes. */
export function createTransport(
  marker: PhoneMarker | null = phoneMarker(),
  deps: TransportDeps = {},
): Transport {
  return marker
    ? createBridgeTransport(marker, deps)
    : createHttpTransport(deps);
}

/** The transport for this page: the bridge inside the shell, HTTP elsewhere. */
export const transport: Transport = createTransport();

declare global {
  interface Window {
    __ELESDEE_PHONE__?: PhoneMarker;
  }
}
