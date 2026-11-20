import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  listBranches,
  listConfigs,
  listProjects,
  getGitStatus,
} from '../api/client';
import type {
  AiConfig,
  Branch,
  CreateSessionInput,
  Project,
  Provider,
  Session,
} from '../api/types';
import { NewSessionDialog } from './NewSessionDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return {
    ...actual,
    listProjects: vi.fn(),
    listConfigs: vi.fn(),
    listBranches: vi.fn(),
    getGitStatus: vi.fn(),
  };
});

const coreProject: Project = {
  id: 'p1',
  name: 'Core',
  folder: '/dev/core',
};

const webProject: Project = {
  id: 'p2',
  name: 'Web',
  folder: '/dev/web',
};

const provider: Provider = {
  id: 'prov1',
  name: 'Anthropic',
  vendor: 'anthropic',
  auth: 'subscription',
  server: null,
  sample: false,
  connected: true,
  plan: 'Max',
  rows: [],
  foot: '',
  supportedClients: ['Claude Code', 'Codex', 'OpenCode 2'],
};

const config: AiConfig = {
  id: 'c1',
  name: 'Anthropic Max',
  providerId: 'prov1',
  client: 'Claude Code',
  model: null,
  effort: null,
  args: [],
  env: [],
  config: {},
  createdAt: '2026-01-01T00:00:00.000Z',
  updatedAt: '2026-01-01T00:00:00.000Z',
};

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 's1',
    name: 'Earlier session',
    short: 'earlier-session',
    projectId: 'p1',
    kind: 'agent',
    status: 'closed',
    workState: null,
    client: 'Claude Code',
    configId: 'c1',
    providerId: 'prov1',
    branch: 'elesdee/core/earlier-session',
    folder: '/dev/core',
    preview: '',
    waitingFor: null,
    pendingPermission: null,
    startedAt: '2026-01-01T00:00:00.000Z',
    shortcuts: [],
    ...overrides,
  };
}

const branches: Branch[] = [
  { name: 'main', current: true },
  { name: 'dev', current: false },
];

beforeEach(() => {
  vi.mocked(listProjects).mockResolvedValue([coreProject, webProject]);
  vi.mocked(listConfigs).mockResolvedValue([config]);
  vi.mocked(listBranches).mockImplementation(async (_folder, query) =>
    branches.filter((branch) => !query || branch.name.includes(query)),
  );
  vi.mocked(getGitStatus).mockResolvedValue({ git: true, dirty: false });
});

interface RenderOptions {
  sessions?: Session[];
  initialProjectId?: string | null;
  onCreate?: (input: CreateSessionInput) => Promise<Session>;
}

function renderDialog(options: RenderOptions = {}) {
  const onCreate = vi.fn<
    (input: CreateSessionInput) => Promise<Session>
  >(async () => makeSession({ id: 'created', status: 'active' }));
  if (options.onCreate) {
    onCreate.mockImplementation(options.onCreate);
  }
  const onCreated = vi.fn();
  const onClose = vi.fn();

  render(
    <NewSessionDialog
      projects={[coreProject, webProject]}
      providers={[provider]}
      configs={[config]}
      sessions={options.sessions ?? []}
      initialProjectId={
        options.initialProjectId === undefined
          ? 'p1'
          : options.initialProjectId
      }
      onCreate={onCreate}
      onCreated={onCreated}
      onClose={onClose}
    />,
  );

  return { onCreate, onCreated, onClose };
}

function additionalSettings(): HTMLDetailsElement {
  const details = screen
    .getByText('Additional settings')
    .closest('details');
  if (!details) {
    throw new Error('Additional settings is not a details element.');
  }
  return details as HTMLDetailsElement;
}

describe('NewSessionDialog', () => {
  it('puts the first prompt under Additional settings and leaves the name empty for the fallback', async () => {
    const user = userEvent.setup();
    const { onCreate, onCreated } = renderDialog();
    const details = additionalSettings();

    expect(details).not.toHaveAttribute('open');
    expect(
      within(details).getByLabelText('First prompt (optional)'),
    ).toBeInTheDocument();
    expect(within(details).queryByLabelText('Source branch (optional)')).toBeNull();

    await user.click(screen.getByText('Additional settings'));
    expect(details).toHaveAttribute('open');

    await user.type(
      within(details).getByLabelText('First prompt (optional)'),
      'Fix the login',
    );
    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledOnce());
    const input = onCreate.mock.calls[0][0];
    expect(input).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      folder: '/dev/core',
      configId: 'c1',
      allowedConfigIds: ['c1'],
      firstPrompt: 'Fix the login',
    });
    expect(input.name).toBeUndefined();
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it('sends a typed name instead of the first prompt', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    await user.type(
      screen.getByLabelText('Session name (optional)'),
      '  My session  ',
    );
    await user.click(screen.getByText('Additional settings'));
    await user.type(
      screen.getByLabelText('First prompt (optional)'),
      'Fix the login',
    );
    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledOnce());
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      name: 'My session',
      firstPrompt: 'Fix the login',
    });
  });

  it('lists repositories from earlier sessions, newest first', async () => {
    const user = userEvent.setup();
    renderDialog({
      sessions: [
        makeSession({
          id: 'older',
          projectId: 'p1',
          folder: '/data/worktrees/a',
          startedAt: '2026-01-02T00:00:00.000Z',
        }),
        makeSession({
          id: 'web',
          projectId: 'p2',
          folder: '/data/worktrees/b',
          startedAt: '2026-01-03T00:00:00.000Z',
        }),
        makeSession({
          id: 'newer',
          projectId: 'p1',
          folder: '/data/worktrees/c',
          startedAt: '2026-01-04T00:00:00.000Z',
        }),
      ],
    });

    await user.click(screen.getByLabelText('Repository folder'));
    await screen.findAllByRole('option');

    const labels = screen
      .getAllByRole('option')
      .map((option) => option.textContent ?? '');
    expect(labels).toHaveLength(2);
    expect(labels[0]).toContain('/dev/core');
    expect(labels[1]).toContain('/dev/web');
  });

  it('prefills the repository from the list without changing other choices', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog({
      sessions: [
        makeSession({
          id: 'web',
          projectId: 'p2',
          folder: '/data/worktrees/b',
          startedAt: '2026-01-03T00:00:00.000Z',
        }),
      ],
    });

    await user.type(
      screen.getByLabelText('Session name (optional)'),
      'Keep my name',
    );
    await user.click(screen.getByText('Additional settings'));
    await user.type(
      screen.getByLabelText('First prompt (optional)'),
      'Keep my prompt',
    );

    const repository = screen.getByLabelText('Repository folder');
    await user.click(repository);
    await screen.findByRole('option', { name: /web/ });
    await user.clear(repository);
    await user.type(repository, 'web');
    await screen.findByRole('option', { name: /web/ });
    expect(
      screen.queryByRole('option', { name: /core/ }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole('option', { name: /web/ }));

    expect(repository).toHaveValue('/dev/web');
    expect(screen.getByLabelText('Project')).toHaveValue('Core');
    expect(screen.getByLabelText('Session name (optional)')).toHaveValue(
      'Keep my name',
    );
    expect(screen.getByLabelText('First prompt (optional)')).toHaveValue(
      'Keep my prompt',
    );

    await user.click(screen.getByRole('button', { name: 'Start session' }));
    await waitFor(() => expect(onCreate).toHaveBeenCalledOnce());
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      folder: '/dev/web',
      name: 'Keep my name',
      firstPrompt: 'Keep my prompt',
    });
  });
});
