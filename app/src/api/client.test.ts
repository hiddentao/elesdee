import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  ApiError,
  bootstrap,
  closeSession,
  createProject,
  deleteSession,
  renameSession,
  reopenSession,
  resizeSession,
  respondPermission,
  sendInput,
} from './client';

function response(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => {
      if (body === undefined) {
        throw new Error('This response has no JSON body.');
      }
      return body;
    },
  } as Response;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('API client', () => {
  it('turns an error payload into an ApiError with the status and message', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(404, { error: 'The project was not found.' }));
    vi.stubGlobal('fetch', fetchMock);

    await expect(bootstrap()).rejects.toMatchObject({
      name: 'ApiError',
      status: 404,
      message: 'The project was not found.',
    });
    expect(fetchMock).toHaveBeenCalledWith('/api/bootstrap', undefined);
  });

  it('uses a status message when the error body is not JSON', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(response(500)));

    await expect(bootstrap()).rejects.toEqual(
      expect.objectContaining({
        name: 'ApiError',
        status: 500,
        message: 'The request failed with status 500.',
      }),
    );
    await expect(bootstrap()).rejects.toBeInstanceOf(ApiError);
  });

  it('sends a JSON header and an empty object when closing a session', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(200, { id: 's1' }));
    vi.stubGlobal('fetch', fetchMock);

    await expect(closeSession('s1')).resolves.toEqual({ id: 's1' });

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1/close', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    });
  });

  it('answers a permission request with the request id and decision', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(204));
    vi.stubGlobal('fetch', fetchMock);

    await expect(
      respondPermission('s1', 'req-1', 'allow'),
    ).resolves.toBeUndefined();

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1/permission', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ requestId: 'req-1', decision: 'allow' }),
    });
  });

  it('creates a project and returns it', async () => {
    const project = { id: 'p1', name: 'core', folder: '/dev/core' };
    const fetchMock = vi.fn().mockResolvedValue(response(200, project));
    vi.stubGlobal('fetch', fetchMock);

    await expect(
      createProject({ name: 'core', folder: '/dev/core' }),
    ).resolves.toEqual(project);

    expect(fetchMock).toHaveBeenCalledWith('/api/projects', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'core', folder: '/dev/core' }),
    });
  });

  it('turns a duplicate project into an ApiError', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        response(409, {
          error: 'A project with that folder already exists.',
        }),
      ),
    );

    await expect(
      createProject({ name: 'core', folder: '/dev/core' }),
    ).rejects.toMatchObject({
      name: 'ApiError',
      status: 409,
      message: 'A project with that folder already exists.',
    });
  });

  it('renames a session with PATCH', async () => {
    const session = { id: 's1', name: 'New name' };
    const fetchMock = vi.fn().mockResolvedValue(response(200, session));
    vi.stubGlobal('fetch', fetchMock);

    await expect(renameSession('s1', 'New name')).resolves.toEqual(session);

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: 'New name' }),
    });
  });

  it('reopens a session with an empty JSON object', async () => {
    const session = { id: 's1', status: 'active' };
    const fetchMock = vi.fn().mockResolvedValue(response(200, session));
    vi.stubGlobal('fetch', fetchMock);

    await expect(reopenSession('s1')).resolves.toEqual(session);

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1/reopen', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    });
  });

  it('deletes a session with DELETE', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(204));
    vi.stubGlobal('fetch', fetchMock);

    await expect(deleteSession('s1')).resolves.toBeUndefined();

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1', {
      method: 'DELETE',
    });
  });

  it('sends the terminal size as JSON', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(204));
    vi.stubGlobal('fetch', fetchMock);

    await expect(resizeSession('s1', 80, 24)).resolves.toBeUndefined();

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/s1/resize', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ cols: 80, rows: 24 }),
    });
  });

  it('sends terminal input as JSON and accepts an empty 204 response', async () => {
    const fetchMock = vi.fn().mockResolvedValue(response(204));
    vi.stubGlobal('fetch', fetchMock);

    await expect(sendInput('session/one', 'ls\n')).resolves.toBeUndefined();

    expect(fetchMock).toHaveBeenCalledWith('/api/sessions/session%2Fone/input', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ data: 'ls\n' }),
    });
  });
});
