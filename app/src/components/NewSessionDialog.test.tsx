import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  getGitHubStatus,
  listBranches,
  listConfigs,
  listProjects,
  getGitStatus,
} from '../api/client';
import type {
  AiConfig,
  Branch,
  CreateSessionInput,
  GitHubRepo,
  GitHubStatus,
  Project,
  Provider,
  Session,
} from '../api/types';
import {
  NewSessionDialog,
  type RepositoryGitHubActions,
} from './NewSessionDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return {
    ...actual,
    listProjects: vi.fn(),
    listConfigs: vi.fn(),
    listBranches: vi.fn(),
    getGitStatus: vi.fn(),
    getGitHubStatus: vi.fn(),
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

const disconnectedGitHub: GitHubStatus = {
  connected: false,
  login: null,
  name: null,
  avatarUrl: null,
  defaultCloneFolder: '/dev/projects',
};

const connectedGitHub: GitHubStatus = {
  ...disconnectedGitHub,
  connected: true,
  login: 'octocat',
  name: 'The Octocat',
};

function githubActions(
  overrides: Partial<RepositoryGitHubActions> = {},
): RepositoryGitHubActions {
  return {
    status: vi.fn(async () => connectedGitHub),
    listRepos: vi.fn(async () => []),
    clone: vi.fn(async () => webProject),
    ...overrides,
  };
}

beforeEach(() => {
  vi.mocked(listProjects).mockResolvedValue([coreProject, webProject]);
  vi.mocked(listConfigs).mockResolvedValue([config]);
  vi.mocked(listBranches).mockImplementation(async (_folder, query) =>
    branches.filter((branch) => !query || branch.name.includes(query)),
  );
  vi.mocked(getGitStatus).mockResolvedValue({ git: true, dirty: false });
  vi.mocked(getGitHubStatus).mockResolvedValue(disconnectedGitHub);
});

interface RenderOptions {
  sessions?: Session[];
  projects?: Project[];
  initialProjectId?: string | null;
  onCreate?: (input: CreateSessionInput) => Promise<Session>;
  github?: RepositoryGitHubActions;
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
      projects={options.projects ?? [coreProject, webProject]}
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
      github={options.github}
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

  it('lists the saved source repository of a moved session', async () => {
    const user = userEvent.setup();
    renderDialog({
      sessions: [
        makeSession({
          id: 'moved',
          projectId: 'p2',
          folder: '/data/worktrees/moved',
          sourceRepository: '/dev/source-repo',
          startedAt: '2026-01-05T00:00:00.000Z',
        }),
      ],
    });

    await user.click(screen.getByLabelText('Repository folder'));
    const option = await screen.findByRole('option', {
      name: /source-repo/,
    });
    expect(option).toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: /\/dev\/web/ }),
    ).not.toBeInTheDocument();
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

  it('keeps the name and additional settings in the scrollable dialog body', () => {
    renderDialog();

    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveClass('new-session-dialog');
    const body = dialog.querySelector('.dialog-body');
    expect(body).not.toBeNull();
    expect(body).toContainElement(
      screen.getByLabelText('Session name (optional)'),
    );
    expect(body).toContainElement(
      screen.getByLabelText('Source branch (optional)'),
    );
    expect(body).toContainElement(screen.getByText('Additional settings'));
    expect(dialog.querySelector('.dialog-foot')).not.toBeNull();
  });

  it('merges recent and GitHub repositories without duplicating', async () => {
    const user = userEvent.setup();
    const repos: GitHubRepo[] = [
      {
        id: 1,
        fullName: 'acme/web',
        name: 'web',
        owner: 'acme',
        private: false,
        cloneUrl: 'https://github.com/acme/web.git',
        htmlUrl: 'https://github.com/acme/web',
        description: '',
        updatedAt: null,
        added: true,
        localFolder: '/dev/web',
      },
      {
        id: 2,
        fullName: 'acme/api',
        name: 'api',
        owner: 'acme',
        private: true,
        cloneUrl: 'https://github.com/acme/api.git',
        htmlUrl: 'https://github.com/acme/api',
        description: '',
        updatedAt: null,
        added: false,
        localFolder: null,
      },
    ];
    const github = githubActions({
      listRepos: vi.fn(async () => repos),
    });
    renderDialog({
      sessions: [
        makeSession({
          id: 'web',
          projectId: 'p2',
          folder: '/data/worktrees/web',
          sourceRepository: '/dev/web',
          startedAt: '2026-01-03T00:00:00.000Z',
        }),
      ],
      github,
    });

    await user.click(screen.getByLabelText('Repository folder'));
    await screen.findByRole('option', { name: /acme\/api/ });

    const options = screen.getAllByRole('option').map((option) => option.textContent ?? '');
    expect(options).toHaveLength(2);
    expect(
      screen.getAllByRole('option', { name: /web/ }),
    ).toHaveLength(1);
    expect(screen.getByRole('option', { name: /web/ })).toHaveTextContent(
      'Added',
    );
    expect(
      screen.getByRole('option', { name: /acme\/api/ }),
    ).toBeInTheDocument();
  });

  it('finds an added repository by owner and name when its folder is renamed', async () => {
    const user = userEvent.setup();
    const repo: GitHubRepo = {
      id: 5,
      fullName: 'acme/web',
      name: 'web',
      owner: 'acme',
      private: false,
      cloneUrl: 'https://github.com/acme/web.git',
      htmlUrl: 'https://github.com/acme/web',
      description: '',
      updatedAt: null,
      added: true,
      localFolder: '/work/client-app',
    };
    renderDialog({
      github: githubActions({ listRepos: vi.fn(async () => [repo]) }),
    });
    const repository = screen.getByLabelText('Repository folder');

    await user.clear(repository);
    await user.type(repository, 'acme');

    const option = await screen.findByRole('option', { name: /client-app/ });
    expect(option).toHaveTextContent('Added');
    await option.click();
    expect(repository).toHaveValue('/work/client-app');
  });

  it('clears a saved default branch that no longer exists', async () => {
    renderDialog({
      projects: [
        { ...coreProject, defaultSourceBranch: 'removed-branch' },
        webProject,
      ],
    });

    const branch = screen.getByLabelText('Source branch (optional)');
    await waitFor(() => expect(branch).toHaveValue(''));
    expect(
      screen.getByText(
        'The saved default branch no longer exists. Pick another branch.',
      ),
    ).toBeInTheDocument();
  });

  it('uses the folder the core matched when two repositories share a name', async () => {
    const user = userEvent.setup();
    const repos: GitHubRepo[] = [
      {
        id: 11,
        fullName: 'acme/web',
        name: 'web',
        owner: 'acme',
        private: false,
        cloneUrl: 'https://github.com/acme/web.git',
        htmlUrl: 'https://github.com/acme/web',
        description: '',
        updatedAt: null,
        added: true,
        localFolder: '/work/acme/web',
      },
      {
        id: 12,
        fullName: 'other/web',
        name: 'web',
        owner: 'other',
        private: false,
        cloneUrl: 'https://github.com/other/web.git',
        htmlUrl: 'https://github.com/other/web',
        description: '',
        updatedAt: null,
        added: true,
        localFolder: '/work/other/web',
      },
    ];
    renderDialog({
      github: githubActions({ listRepos: vi.fn(async () => repos) }),
    });
    const repository = screen.getByLabelText('Repository folder');

    await user.click(repository);
    await (
      await screen.findByRole('option', { name: /\/work\/acme\/web/ })
    ).click();
    expect(repository).toHaveValue('/work/acme/web');

    await user.clear(repository);
    await user.click(repository);
    await (
      await screen.findByRole('option', { name: /\/work\/other\/web/ })
    ).click();
    expect(repository).toHaveValue('/work/other/web');
  });

  it('clones a remote repository into the chosen parent and starts there', async () => {
    const user = userEvent.setup();
    const repo: GitHubRepo = {
      id: 2,
      fullName: 'acme/api',
      name: 'api',
      owner: 'acme',
      private: true,
      cloneUrl: 'https://github.com/acme/api.git',
      htmlUrl: 'https://github.com/acme/api',
      description: '',
      updatedAt: null,
      added: false,
      localFolder: null,
    };
    const clone = vi.fn(async () => ({
      id: 'api',
      name: 'api',
      folder: '/dev/clones/api',
    }));
    const github = githubActions({
      listRepos: vi.fn(async () => [repo]),
      clone,
    });
    const { onCreate } = renderDialog({ github });

    await user.click(screen.getByLabelText('Repository folder'));
    const option = await screen.findByRole('option', { name: /acme\/api/ });
    await option.click();

    const cloneFolder = screen.getByLabelText('Clone folder');
    expect(cloneFolder).toHaveValue('/dev/projects');
    await user.clear(cloneFolder);
    await user.type(cloneFolder, '/dev/clones');
    expect(
      screen.getByText(
        'The repository is cloned into /dev/clones/api, then the session starts there.',
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() =>
      expect(clone).toHaveBeenCalledWith({
        fullName: 'acme/api',
        parentFolder: '/dev/clones',
      }),
    );
    await waitFor(() => expect(onCreate).toHaveBeenCalledOnce());
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      folder: '/dev/clones/api',
    });
  });
});
