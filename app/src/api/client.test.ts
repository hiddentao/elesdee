import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  ApiError,
  bootstrap,
  closeSession,
  createAccount,
  createProject,
  deleteSession,
  listAccounts,
  listProjects,
  renameSession,
  reopenSession,
  resizeSession,
  respondPermission,
  sendInput,
  updateProjectDefaults,
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

  it('creates a Google account with the key and model', async () => {
    const account = {
      id: 'google-1',
      name: 'Google work',
      plan: 'Google AI',
      client: 'Antigravity',
      provider: 'google',
      auth: 'key',
      sample: false,
      rows: [],
      foot: 'No official usage source.',
      server: null,
      model: 'gemini-3.8-flash-medium',
    };
    const fetchMock = vi.fn().mockResolvedValue(response(200, account));
    vi.stubGlobal('fetch', fetchMock);

    const input = {
      name: 'Google work',
      providerId: 'google' as const,
      client: 'Antigravity' as const,
      auth: 'key' as const,
      key: 'secret-key',
      model: 'gemini-3.8-flash-medium',
    };

    await expect(createAccount(input)).resolves.toEqual(account);
    expect(fetchMock).toHaveBeenCalledWith('/api/accounts', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(input),
    });
  });

  it('loads projects with a query and an abort signal', async () => {
    const projects = [{ id: 'p1', name: 'core', folder: '/dev/core' }];
    const fetchMock = vi.fn().mockResolvedValue(response(200, projects));
    vi.stubGlobal('fetch', fetchMock);
    const controller = new AbortController();

    await expect(listProjects('core', controller.signal)).resolves.toEqual(
      projects,
    );

    expect(fetchMock).toHaveBeenCalledWith('/api/projects?q=core', {
      signal: controller.signal,
    });
  });

  it('loads accounts with a query and an abort signal', async () => {
    const accounts = [{ id: 'a1', name: 'Work' }];
    const fetchMock = vi.fn().mockResolvedValue(response(200, accounts));
    vi.stubGlobal('fetch', fetchMock);
    const controller = new AbortController();

    await expect(listAccounts('work account', controller.signal)).resolves.toEqual(
      accounts,
    );

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/accounts?q=work+account',
      { signal: controller.signal },
    );
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

  it('updates project defaults with a patch request', async () => {
    const project = {
      id: 'p1',
      name: 'core',
      folder: '/dev/core',
      defaultAccountId: 'a1',
      defaultSourceBranch: 'main',
    };
    const fetchMock = vi.fn().mockResolvedValue(response(200, project));
    vi.stubGlobal('fetch', fetchMock);
    const input = {
      defaultAccountId: 'a1',
      defaultSourceBranch: 'main',
    };

    await expect(updateProjectDefaults('p1', input)).resolves.toEqual(project);

    expect(fetchMock).toHaveBeenCalledWith('/api/projects/p1', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(input),
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
