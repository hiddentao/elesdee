import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type { Account, Project } from '../api/types';
import { makeSession } from '../test/fixtures';
import { NewSessionDialog } from './NewSessionDialog';

const projects: Project[] = [
  { id: 'p1', name: 'core', folder: '/dev/core' },
  { id: 'p2', name: 'web', folder: '/dev/web' },
];

const defaultProjects: Project[] = [
  {
    id: 'p1',
    name: 'core',
    folder: '/dev/core',
    defaultAccountId: 'a1',
    defaultSourceBranch: 'feature',
  },
  { id: 'p2', name: 'web', folder: '/dev/web' },
];

const accounts: Account[] = [
  {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'Anthropic',
    auth: 'subscription',
    sample: true,
    rows: [{ label: '5-hour', used: 58 }],
    foot: 'Resets in 2h.',
    server: null,
    model: null,
  },
  {
    id: 'a2',
    name: 'Personal',
    plan: 'Plus',
    client: 'Codex',
    provider: 'OpenAI',
    auth: 'subscription',
    sample: true,
    rows: [],
    foot: '',
    server: null,
    model: null,
  },
  {
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
  },
];

const createdSession = makeSession({
  name: 'Fix login',
  short: 'fix-login',
  branch: 'elesdee/core/fix-login',
  folder: '/dev/core',
  preview: '',
});

function jsonResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => body,
  } as Response;
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function matches(text: string, query: string): boolean {
  return text.toLowerCase().includes(query.toLowerCase());
}

function pickerResponse(input: RequestInfo | URL): Promise<Response> {
  const url = new URL(String(input), 'http://localhost');
  const query = url.searchParams.get('q') ?? '';

  if (url.pathname === '/api/projects') {
    return Promise.resolve(
      jsonResponse(
        projects.filter(
          (project) =>
            matches(project.name, query) || matches(project.folder, query),
        ),
      ),
    );
  }

  if (url.pathname === '/api/accounts') {
    return Promise.resolve(
      jsonResponse(
        accounts.filter(
          (account) =>
            matches(account.name, query) ||
            matches(account.client, query) ||
            matches(account.plan, query),
        ),
      ),
    );
  }

  if (url.pathname === '/api/branches') {
    return Promise.resolve(
      jsonResponse([
        { name: 'main', current: true },
        { name: 'feature', current: false },
        { name: 'release', current: false },
      ]),
    );
  }

  return Promise.reject(new Error(`Unexpected request: ${String(input)}`));
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(pickerResponse));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function renderDialog(
  onCreate = vi.fn(),
  initialProjectId: string | null = null,
  dialogProjects: Project[] = projects,
) {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <NewSessionDialog
      projects={dialogProjects}
      accounts={accounts}
      initialProjectId={initialProjectId}
      onCreate={onCreate}
      onCreated={onCreated}
      onClose={onClose}
    />,
  );
  return { onCreate, onCreated, onClose };
}

async function chooseProject(user: ReturnType<typeof userEvent.setup>, name: string) {
  const input = screen.getByLabelText('Project');
  await user.click(input);
  await user.type(input, name);
  await user.click(
    await screen.findByRole('option', { name: new RegExp(name) }),
  );
}

describe('NewSessionDialog', () => {
  it('focuses the first field and shows the server error inline', async () => {
    const user = userEvent.setup();
    const onCreate = vi
      .fn()
      .mockRejectedValue(new ApiError(400, 'The project folder does not exist.'));
    const { onCreated } = renderDialog(onCreate);

    expect(screen.getByLabelText('Project')).toHaveFocus();

    await chooseProject(user, 'core');
    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));

    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);
    expect(
      await screen.findByRole('option', { name: /42% left/ }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('option', { name: /Work/ }));

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The project folder does not exist.',
    );
    expect(onCreated).not.toHaveBeenCalled();
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      accountId: 'a1',
      folder: '/dev/core',
    });
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('client');
  });

  it('prefills the stored account, client and source branch for a project', async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(createdSession);
    renderDialog(onCreate, 'p1', defaultProjects);

    expect(screen.getByLabelText('Repository folder')).toHaveValue('/dev/core');
    expect(screen.getByRole('radio', { name: 'Claude Code' })).toBeChecked();
    expect(screen.getByLabelText('Account')).toHaveValue('Work');
    expect(screen.getByLabelText('Source branch (optional)')).toHaveValue(
      'feature',
    );

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      accountId: 'a1',
      sourceBranch: 'feature',
      folder: '/dev/core',
    });
  });

  it('does not prefill a sample account when the project has no default', () => {
    renderDialog(vi.fn(), 'p1');

    expect(screen.getByLabelText('Account')).toHaveValue('');
    expect(
      screen.getByRole('radio', { name: 'Claude Code' }),
    ).not.toBeChecked();
    expect(screen.getByLabelText('Source branch (optional)')).toHaveValue('');
  });

  it('lets the user override the project defaults', async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(createdSession);
    renderDialog(onCreate, 'p1', defaultProjects);

    await user.click(screen.getByRole('radio', { name: 'Codex' }));
    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);
    await user.clear(accountInput);
    await user.type(accountInput, 'Personal');
    await user.click(
      await screen.findByRole('option', { name: /Personal/ }),
    );

    const branchInput = screen.getByLabelText('Source branch (optional)');
    await user.click(branchInput);
    await user.click(
      await screen.findByRole('option', { name: /^release/ }),
    );

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      accountId: 'a2',
      sourceBranch: 'release',
    });
  });

  it('shows the repository folder for an agent and sends it', async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(createdSession);
    const { onCreated } = renderDialog(onCreate, 'p1');

    expect(screen.getByLabelText('Repository folder')).toHaveValue('/dev/core');
    expect(
      screen.getByText('The folder must be inside a Git repository.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));
    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);
    await user.click(await screen.findByRole('option', { name: /Work/ }));

    expect(screen.getByText('42% left')).toBeInTheDocument();
    expect(screen.getByText('Sample')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(createdSession));
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      accountId: 'a1',
      folder: '/dev/core',
    });
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('client');
  });

  it('validates that a project is chosen before it calls the server', async () => {
    const user = userEvent.setup();
    const { onCreate, onCreated } = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(screen.getByText('Choose a project from the list.')).toBeInTheDocument();
    expect(screen.getByLabelText('Project')).toHaveFocus();
    expect(onCreate).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('offers Antigravity and its account with no usage data', async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(screen.getByRole('radio', { name: 'Antigravity' }));

    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);

    const option = await screen.findByRole('option', { name: /Google work/ });
    expect(option).toHaveTextContent('No usage data');
    await user.click(option);

    expect(screen.getByText('No usage data')).toBeInTheDocument();
    expect(screen.queryByText('Sample')).not.toBeInTheDocument();
  });

  it('labels the folder for the chosen session kind', async () => {
    const user = userEvent.setup();
    renderDialog();

    expect(screen.getByLabelText('Repository folder')).toBeInTheDocument();
    expect(
      screen.getByText('The folder must be inside a Git repository.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: 'Terminal' }));

    expect(screen.getByLabelText('Folder')).toBeInTheDocument();
    expect(
      screen.queryByText('The folder must be inside a Git repository.'),
    ).not.toBeInTheDocument();
  });

  it('requires a folder for a terminal session', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    await chooseProject(user, 'web');
    await user.click(screen.getByRole('radio', { name: 'Terminal' }));

    const folder = screen.getByLabelText('Folder');
    expect(folder).toHaveValue('/dev/web');

    await user.clear(folder);
    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(screen.getByText('Enter a folder.')).toBeInTheDocument();
    expect(onCreate).not.toHaveBeenCalled();
  });

  it('shows a loading row while the project list loads', async () => {
    const user = userEvent.setup();
    const pending = deferred<Response>();
    vi.stubGlobal('fetch', vi.fn().mockReturnValue(pending.promise));
    renderDialog();

    await user.click(screen.getByLabelText('Project'));

    expect(await screen.findByText('Loading…')).toBeInTheDocument();

    pending.resolve(jsonResponse(projects));

    expect(
      await screen.findByRole('option', { name: /core/ }),
    ).toBeInTheDocument();
    expect(screen.queryByText('Loading…')).not.toBeInTheDocument();
  });

  it('debounces typing and sends the text as the query', async () => {
    const user = userEvent.setup();
    renderDialog();
    const input = screen.getByLabelText('Project');
    const fetchMock = vi.mocked(fetch);

    await user.click(input);
    await screen.findByRole('option', { name: /core/ });
    fetchMock.mockClear();

    fireEvent.change(input, { target: { value: 'web' } });

    expect(fetchMock).not.toHaveBeenCalled();

    await waitFor(
      () =>
        expect(fetchMock).toHaveBeenCalledWith(
          '/api/projects?q=web',
          expect.anything(),
        ),
      { timeout: 1000 },
    );
  });

  it('ignores a stale project response', async () => {
    const user = userEvent.setup();
    const requests: Deferred<Response>[] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn((input: RequestInfo | URL) => {
        expect(String(input)).toContain('/api/projects?q=');
        const request = deferred<Response>();
        requests.push(request);
        return request.promise;
      }),
    );
    renderDialog();
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await waitFor(() => expect(requests).toHaveLength(1));
    requests[0].resolve(jsonResponse(projects));
    await screen.findByRole('option', { name: /web/ });

    fireEvent.change(input, { target: { value: 'web' } });
    await waitFor(() => expect(requests).toHaveLength(2), { timeout: 1000 });

    fireEvent.change(input, { target: { value: 'core' } });
    await waitFor(() => expect(requests).toHaveLength(3), { timeout: 1000 });

    requests[2].resolve(jsonResponse([projects[0]]));
    expect(
      await screen.findByRole('option', { name: /core/ }),
    ).toBeInTheDocument();

    await act(async () => {
      requests[1].resolve(jsonResponse([projects[1]]));
      await Promise.resolve();
    });

    expect(screen.getByRole('option', { name: /core/ })).toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: /web/ }),
    ).not.toBeInTheDocument();
  });

  it('shows a no-match row when no project matches the query', async () => {
    const user = userEvent.setup();
    renderDialog();
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await screen.findByRole('option', { name: /core/ });

    fireEvent.change(input, { target: { value: 'nothing' } });

    expect(await screen.findByText('No matches.')).toBeInTheDocument();
    expect(screen.queryByRole('option')).not.toBeInTheDocument();
  });

  it('keeps keyboard navigation and focus on the async list', async () => {
    const user = userEvent.setup();
    renderDialog();
    const input = screen.getByLabelText('Project');

    await user.click(input);
    await screen.findByRole('option', { name: /core/ });
    expect(input).toHaveFocus();

    await user.keyboard('{ArrowDown}{Enter}');

    expect(input).toHaveValue('web');
    expect(input).toHaveFocus();
    expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  });

  it('says when the chosen client has no account', async () => {
    const user = userEvent.setup();
    render(
      <NewSessionDialog
        projects={projects}
        accounts={[]}
        onCreate={vi.fn()}
        onCreated={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));

    expect(
      screen.getByText('No account is set up for Claude Code yet.'),
    ).toBeInTheDocument();
  });

  it('filters accounts to the chosen client and asks for a client first', async () => {
    const user = userEvent.setup();
    renderDialog();

    expect(screen.getByLabelText('Account')).toHaveAttribute(
      'placeholder',
      'Choose a client first',
    );

    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));
    await user.click(screen.getByLabelText('Account'));

    expect(
      await screen.findByRole('option', { name: /Work/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: /Personal/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: /Google work/ }),
    ).not.toBeInTheDocument();
    expect(fetch).toHaveBeenCalledWith('/api/accounts?q=', expect.anything());
  });
});
