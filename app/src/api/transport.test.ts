import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource } from '../test/fakeEventSource';
import {
  ApiError,
  createBridgeTransport,
  createHttpTransport,
  createTransport,
  type EventSourceFn,
  type InvokeFn,
  type ListenFn,
  type PhoneMarker,
  type PhoneNotice,
} from './transport';

const MARKER: PhoneMarker = {
  computerId: 'computer-1',
  computerName: 'Studio',
  bridgeVersion: 1,
  protocolVersion: 1,
};

interface FetchCall {
  url: string;
  init?: RequestInit;
}

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function fetchStub(response: Response | (() => Response)) {
  const calls: FetchCall[] = [];
  const fetch = (url: string, init?: RequestInit): Promise<Response> => {
    calls.push({ url, init });
    return Promise.resolve(
      typeof response === 'function' ? response() : response.clone(),
    );
  };
  return { fetch, calls };
}

function invokeStub(result: unknown = null) {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const invoke: InvokeFn = (command, args) => {
    calls.push({ command, args });
    return Promise.resolve(result as never);
  };
  return { invoke, calls };
}

function listenStub() {
  const handlers = new Map<string, (payload: unknown) => void>();
  const listen: ListenFn = (event, handler) => {
    handlers.set(event, handler as (payload: unknown) => void);
    return Promise.resolve(() => {
      handlers.delete(event);
    });
  };
  return { listen, handlers };
}

function dispatch(
  handlers: Map<string, (payload: unknown) => void>,
  event: string,
  payload: unknown,
): void {
  handlers.get(event)?.(payload);
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  FakeEventSource.reset();
  vi.useRealTimers();
});

describe('HTTP transport', () => {
  it('maps a session input method to its local route', async () => {
    const { fetch, calls } = fetchStub(jsonResponse({ ok: true }));
    const transport = createHttpTransport({ fetch });

    await transport.request('sessions.input', {
      sessionId: 's1',
      data: 'ls\n',
    });

    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe('/api/sessions/s1/input');
    expect(calls[0].init?.method).toBe('POST');
    expect(calls[0].init?.body).toBe('{"data":"ls\\n"}');
  });

  it('maps the bootstrap and branch list to their routes', async () => {
    const { fetch, calls } = fetchStub(jsonResponse([]));
    const transport = createHttpTransport({ fetch });

    await transport.request('bootstrap');
    await transport.request('branches.list', { folder: 'a b', q: 'main' });

    expect(calls[0].url).toBe('/api/bootstrap');
    expect(calls[1].url).toBe('/api/branches?folder=a%20b&q=main');
  });

  it('maps every phone method to its local route', async () => {
    const { fetch, calls } = fetchStub(() => jsonResponse({}));
    const transport = createHttpTransport({ fetch });
    const cases: [string, unknown, string, string][] = [
      ['health', undefined, 'GET', '/api/health'],
      ['projects.list', { q: 'web' }, 'GET', '/api/projects?q=web'],
      ['providers.list', undefined, 'GET', '/api/providers'],
      ['accounts.list', { q: '' }, 'GET', '/api/accounts'],
      ['sessions.create', { projectId: 'p', kind: 'terminal' }, 'POST', '/api/sessions'],
      ['sessions.close', { sessionId: 's1' }, 'POST', '/api/sessions/s1/close'],
      ['sessions.input', { sessionId: 's1', data: 'x' }, 'POST', '/api/sessions/s1/input'],
      [
        'sessions.permission',
        { sessionId: 's1', requestId: 'r1', decision: 'allow' },
        'POST',
        '/api/sessions/s1/permission',
      ],
      [
        'sessions.account',
        { sessionId: 's1', accountId: 'a1' },
        'POST',
        '/api/sessions/s1/account',
      ],
      [
        'sessions.config',
        { sessionId: 's1', configId: 'c1' },
        'POST',
        '/api/sessions/s1/config',
      ],
      ['sessions.resume', { sessionId: 's1' }, 'POST', '/api/sessions/s1/resume'],
      ['questions.list', undefined, 'GET', '/api/questions'],
      [
        'questions.answer',
        { id: 'q1', answer: 'yes' },
        'POST',
        '/api/questions/q1/answer',
      ],
      [
        'sessionRequests.approve',
        { id: 'r1', allowFurther: true },
        'POST',
        '/api/session-requests/r1/approve',
      ],
      [
        'sessionRequests.deny',
        { id: 'r1' },
        'POST',
        '/api/session-requests/r1/deny',
      ],
      [
        'files.list',
        { sessionId: 's1', path: 'a/b' },
        'GET',
        '/api/sessions/s1/files?path=a%2Fb',
      ],
      [
        'files.read',
        { sessionId: 's1', path: 'a.txt' },
        'GET',
        '/api/sessions/s1/file?path=a.txt',
      ],
      ['diff.read', { sessionId: 's1' }, 'GET', '/api/sessions/s1/changes'],
      ['demoServers.list', undefined, 'GET', '/api/demo-servers'],
    ];

    for (const [method, params] of cases) {
      await transport.request(method, params);
    }

    expect(calls.map((call) => call.url)).toEqual(
      cases.map(([, , , path]) => path),
    );
    expect(calls.map((call) => call.init?.method ?? 'GET')).toEqual(
      cases.map(([, , verb]) => verb),
    );
  });

  it('turns a link method error into an ApiError', async () => {
    const { fetch } = fetchStub(
      jsonResponse({ error: 'The session is closed.' }, 409),
    );
    const transport = createHttpTransport({ fetch });

    await expect(
      transport.request('sessions.close', { sessionId: 's1' }),
    ).rejects.toMatchObject({ status: 409, message: 'The session is closed.' });
  });

  it('refuses a method outside the allowlist', () => {
    const transport = createHttpTransport({ fetch: fetchStub(jsonResponse({})).fetch });
    expect(() => transport.request('projects.create')).toThrowError(ApiError);
  });

  it('uploads a binary body and reads an image blob', async () => {
    const upload = fetchStub(jsonResponse({ name: 'a.txt', path: '/x', size: 5 }));
    const imageBytes = new Uint8Array([1, 2, 3]);
    const image = fetchStub(
      new Response(imageBytes, {
        status: 200,
        headers: { 'Content-Type': 'image/png' },
      }),
    );

    const transport = createHttpTransport({
      fetch: (url, init) =>
        url.endsWith('/upload') ? upload.fetch(url, init) : image.fetch(url, init),
    });

    await transport.upload('s1', 'a b.txt', new Blob([new Uint8Array([1])]));
    expect(upload.calls[0].url).toBe('/api/sessions/s1/upload');
    expect(upload.calls[0].init?.headers).toMatchObject({
      'X-El-Esdee-Filename': 'a%20b.txt',
    });

    const blob = await transport.readImage('s1', 'shot one.png');
    expect(image.calls[0].url).toBe(
      '/api/sessions/s1/image?path=shot%20one.png',
    );
    expect(blob.type).toBe('image/png');
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(imageBytes);
  });

  it('streams an event source and passes each parsed value', () => {
    const source = new FakeEventSource('/api/events');
    const eventSource: EventSourceFn = () => source;
    const transport = createHttpTransport({ eventSource });
    const onEvent = vi.fn();
    const onOpen = vi.fn();

    const stop = transport.subscribe('events', undefined, { onEvent, onOpen });
    source.open();
    source.message({ type: 'session.updated', session: { id: 's1' } });

    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith({
      type: 'session.updated',
      session: { id: 's1' },
    });

    stop();
    expect(source.closed).toBe(true);
  });

  it('reconnects an output stream from the last sequence', () => {
    vi.useFakeTimers();
    const sources: FakeEventSource[] = [];
    const eventSource: EventSourceFn = (url) => {
      const source = new FakeEventSource(url);
      sources.push(source);
      return source;
    };
    const transport = createHttpTransport({ eventSource });
    const onEvent = vi.fn();
    const onError = vi.fn();

    const stop = transport.subscribe(
      'output',
      { sessionId: 's1', after: 0 },
      { onEvent, onError },
    );
    expect(sources[0].url).toBe('/api/sessions/s1/output?after=0');

    sources[0].message({ seq: 3, data: 'hello' });
    expect(onEvent).toHaveBeenCalledWith({ seq: 3, data: 'hello' });

    sources[0].fail();
    expect(onError).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(500);

    expect(sources).toHaveLength(2);
    expect(sources[1].url).toBe('/api/sessions/s1/output?after=3');

    stop();
  });

  it('does not retry a closed session stream', () => {
    vi.useFakeTimers();
    const sources: FakeEventSource[] = [];
    const eventSource: EventSourceFn = (url) => {
      const source = new FakeEventSource(url);
      sources.push(source);
      return source;
    };
    const transport = createHttpTransport({ eventSource });
    const onError = vi.fn();

    const stop = transport.subscribe(
      'output',
      { sessionId: 's2', after: 0 },
      { onEvent: vi.fn(), onError, isClosed: () => true },
    );
    sources[0].fail();
    vi.advanceTimersByTime(10000);

    expect(onError).not.toHaveBeenCalled();
    expect(sources).toHaveLength(1);

    stop();
  });
});

describe('bridge transport', () => {
  it('sends a unary request against the allowlist', async () => {
    const { invoke, calls } = invokeStub({ requests: [] });
    const transport = createBridgeTransport(MARKER, { invoke });

    await expect(transport.request('questions.list')).resolves.toEqual({
      requests: [],
    });
    expect(calls[0]).toEqual({
      command: 'phone_request',
      args: {
        computerId: 'computer-1',
        method: 'questions.list',
        params: null,
      },
    });
  });

  it('maps a bridge error code to an ApiError status', async () => {
    const invoke: InvokeFn = () => {
      return Promise.reject({ code: 'conflict', message: 'Already closed.' });
    };
    const transport = createBridgeTransport(MARKER, { invoke });

    await expect(
      transport.request('sessions.close', { sessionId: 's1' }),
    ).rejects.toMatchObject({ status: 409, message: 'Already closed.' });
  });

  it('opens one event stream for both event channels', async () => {
    const { invoke, calls } = invokeStub({ id: 'package:events' });
    const { listen, handlers } = listenStub();
    const transport = createBridgeTransport(MARKER, { invoke, listen });
    const onSession = vi.fn();
    const onAux = vi.fn();

    const stopEvents = transport.subscribe('events', undefined, {
      onEvent: onSession,
    });
    const stopAux = transport.subscribe('aux', undefined, { onEvent: onAux });
    await flush();

    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({
      command: 'phone_subscribe',
      args: {
        computerId: 'computer-1',
        id: 'package:events',
        channel: 'events',
      },
    });

    dispatch(handlers, 'phone:subscription', {
      id: 'package:events',
      event: { type: 'session.updated', session: { id: 's1' } },
    });
    expect(onSession).toHaveBeenCalledTimes(1);
    expect(onAux).toHaveBeenCalledTimes(1);

    stopEvents();
    stopAux();
    await flush();
    expect(calls.some((call) => call.command === 'phone_unsubscribe')).toBe(
      true,
    );
  });

  it('opens an output stream from the requested sequence', async () => {
    const { invoke, calls } = invokeStub({ id: 'package:output:1' });
    const { listen, handlers } = listenStub();
    const transport = createBridgeTransport(MARKER, { invoke, listen });
    const onEvent = vi.fn();
    const onOpen = vi.fn();
    const onEnd = vi.fn();

    const stop = transport.subscribe(
      'output',
      { sessionId: 's1', after: 7 },
      { onEvent, onOpen, onEnd },
    );
    await flush();

    expect(calls[0]).toMatchObject({
      command: 'phone_subscribe',
      args: {
        id: 'package:output:1',
        channel: 'output',
        params: { sessionId: 's1', after: 7 },
      },
    });
    expect(onOpen).toHaveBeenCalledTimes(1);

    dispatch(handlers, 'phone:subscription', {
      id: 'package:output:1',
      event: { seq: 8, data: 'tail' },
    });
    expect(onEvent).toHaveBeenCalledWith({ seq: 8, data: 'tail' });

    dispatch(handlers, 'phone:subscription', {
      id: 'package:output:1',
      ended: true,
    });
    expect(onEnd).toHaveBeenCalledTimes(1);

    stop();
    await flush();
    expect(calls.some((call) => call.command === 'phone_unsubscribe')).toBe(
      true,
    );
  });

  it('uploads base64 bytes and reads an image payload', async () => {
    const { invoke, calls } = invokeStub({});
    const transport = createBridgeTransport(MARKER, { invoke });

    await transport.upload(
      's1',
      'hello.txt',
      new Blob([new Uint8Array([104, 101, 108, 108, 111])]),
    );
    expect(calls[0]).toEqual({
      command: 'phone_upload',
      args: {
        computerId: 'computer-1',
        sessionId: 's1',
        name: 'hello.txt',
        data: 'aGVsbG8=',
      },
    });

    const image = createBridgeTransport(MARKER, {
      invoke: invokeStub({
        contentType: 'image/png',
        data: 'AQID',
      }).invoke,
    });
    const blob = await image.readImage('s1', 'shot.png');
    expect(blob.type).toBe('image/png');
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(
      new Uint8Array([1, 2, 3]),
    );
  });

  it('refuses an upload over the link cap before the bridge call', async () => {
    const { invoke, calls } = invokeStub({});
    const transport = createBridgeTransport(MARKER, { invoke });
    const big = new Blob([new Uint8Array(25 * 1024 * 1024 + 1)]);

    await expect(transport.upload('s1', 'big.bin', big)).rejects.toMatchObject({
      status: 413,
    });
    expect(calls).toHaveLength(0);
  });

  it('asks the shell for a notice only from package facts', () => {
    const { invoke, calls } = invokeStub(false);
    const transport = createBridgeTransport(MARKER, { invoke });
    const notice: PhoneNotice = {
      event: {
        kind: 'session',
        sessionId: 's1',
        sessionName: 'Build',
        status: 'active',
        waitingFor: 'input',
        previousWaitingFor: null,
      },
      screenSessionId: null,
      appVisible: true,
      texts: {
        waitingTitle: 'Waiting',
        requestTitle: 'Request',
        requestBody: 'Approve',
        permissionBody: 'Permission',
        questionBody: 'Question',
        inputBody: 'Input',
        doneTitle: 'Done',
        doneBody: 'Complete',
        messageTitle: 'Message',
        messageBody: 'Hello',
      },
    };

    transport.notify(notice);
    expect(calls[0]).toEqual({
      command: 'phone_notification_event',
      args: {
        computerId: 'computer-1',
        event: notice.event,
        screenSessionId: null,
        appVisible: true,
        texts: notice.texts,
      },
    });
  });

  it('routes a notification click for this computer', async () => {
    const { listen, handlers } = listenStub();
    const transport = createBridgeTransport(MARKER, {
      invoke: invokeStub().invoke,
      listen,
    });
    const clicked = vi.fn();

    const stop = transport.notificationClick(clicked);
    await flush();
    dispatch(handlers, 'phone:notification-click', {
      computerId: 'computer-1',
      sessionId: 's1',
    });
    dispatch(handlers, 'phone:notification-click', {
      computerId: 'computer-2',
      sessionId: 's2',
    });

    expect(clicked).toHaveBeenCalledTimes(1);
    expect(clicked).toHaveBeenCalledWith('s1');

    stop();
  });
});

describe('transport selection', () => {
  it('uses HTTP without the shell marker and the bridge with it', () => {
    expect(createTransport(null, { fetch: vi.fn() }).kind).toBe('http');
    expect(
      createTransport(MARKER, {
        invoke: invokeStub().invoke,
        listen: listenStub().listen,
      }).kind,
    ).toBe('phone');
  });
});
