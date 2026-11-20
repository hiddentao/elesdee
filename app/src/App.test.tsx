import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App';
import { makeSession } from './test/fixtures';
import {
  ApiError,
  bootstrap,
  closeSession,
  createAccount,
  createProject,
  deleteSession,
  health,
  listAccounts,
  listProjects,
  listProviders,
  renameSession,
  reopenSession,
  respondPermission,
} from './api/client';
import { subscribeEvents } from './api/events';
import { isTauri, openFolder, openTerminal } from './api/shell';
import type { Account } from './api/types';

vi.mock('./api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api/client')>();
  return {
    ...actual,
    bootstrap: vi.fn(),
    createAccount: vi.fn(),
    createSession: vi.fn(),
    closeSession: vi.fn(),
    createProject: vi.fn(),
    deleteSession: vi.fn(),
    health: vi.fn(),
    listAccounts: vi.fn(),
    listProjects: vi.fn(),
    listProviders: vi.fn(),
    renameSession: vi.fn(),
    reopenSession: vi.fn(),
    respondPermission: vi.fn(),
    sendInput: vi.fn(),
  };
});

vi.mock('./api/events', () => ({
  subscribeEvents: vi.fn(),
}));

vi.mock('./api/shell', () => ({
  isTauri: vi.fn(() => false),
  openFolder: vi.fn(),
  openTerminal: vi.fn(),
}));

vi.mock('./components/TerminalPane', () => ({
  TerminalPane: () => null,
}));

afterEach(() => {
  vi.unstubAllGlobals();
});


function topNewSessionButton() {
  const titlebar = document.querySelector('.titlebar');
  if (!titlebar) {
    throw new Error('The titlebar is missing.');
  }
  return within(titlebar as HTMLElement).getByRole('button', {
    name: 'New session',
  });
}

const activeSession = makeSession({
  name: 'Check close',
  short: 'check-close',
  kind: 'terminal',
  client: null,
  accountId: null,
});

beforeEach(() => {
  vi.mocked(bootstrap).mockResolvedValue({
    projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
    accounts: [],
    sessions: [activeSession],
  });
  vi.mocked(closeSession).mockResolvedValue({
    ...activeSession,
    status: 'closed',
    workState: null,
  });
  vi.mocked(health).mockResolvedValue({
    ok: true,
    version: '0.1.0',
    agents: [],
  });
  vi.mocked(listProjects).mockResolvedValue([
    { id: 'p1', name: 'web', folder: '/dev/web' },
  ]);
  vi.mocked(listAccounts).mockResolvedValue([]);
  vi.mocked(listProviders).mockResolvedValue([
    {
      id: 'anthropic',
      label: 'Anthropic',
      client: 'Claude Code',
      auth: ['subscription', 'key'],
      defaultModel: null,
      custom: false,
    },
    {
      id: 'google',
      label: 'Google',
      client: 'Antigravity',
      auth: ['key'],
      defaultModel: 'gemini-3.8-flash-medium',
      custom: false,
    },
  ]);
  vi.mocked(createProject).mockResolvedValue({
    id: 'p2',
    name: 'added',
    folder: '/dev/added',
  });
  vi.mocked(renameSession).mockResolvedValue({
    ...activeSession,
    name: 'Renamed task',
  });
  vi.mocked(reopenSession).mockResolvedValue({
    ...activeSession,
    status: 'active',
    workState: 'working',
  });
  vi.mocked(deleteSession).mockResolvedValue(undefined);
  vi.mocked(subscribeEvents).mockReturnValue(() => {});
  vi.mocked(isTauri).mockReturnValue(false);
  vi.mocked(openFolder).mockResolvedValue(undefined);
  vi.mocked(openTerminal).mockResolvedValue(undefined);
});

describe('first launch', () => {
  it('shows the agent check and adds the first project', async () => {
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [],
      accounts: [],
      sessions: [],
    });
    vi.mocked(health).mockResolvedValue({
      ok: true,
      version: '0.1.0',
      agents: [
        { name: 'Claude Code', installed: true, version: '2.1.328' },
        { name: 'Codex', installed: false, version: null },
        { name: 'OpenCode 2', installed: false, version: null },
      ],
    });
    const added = { id: 'p1', name: 'core', folder: '/dev/core' };
    vi.mocked(createProject).mockResolvedValue(added);

    const user = userEvent.setup();
    render(<App />);

    expect(
      await screen.findByRole('heading', { name: 'Welcome to elesdee' }),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Installed · 2.1.328'),
    ).toBeInTheDocument();
    expect(screen.getAllByText('Not installed')).toHaveLength(2);
    expect(
      screen.getByText('Install Codex to start sessions with it.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Install OpenCode 2 to start sessions with it.'),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /^Active/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Show closed' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /^Usage/ }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Add project' }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText('Name'), 'core');
    await user.type(within(dialog).getByLabelText('Folder'), '/dev/core');
    await user.click(within(dialog).getByRole('button', { name: 'Add project' }));

    await waitFor(() =>
      expect(createProject).toHaveBeenCalledWith({
        name: 'core',
        folder: '/dev/core',
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole('heading', { name: 'Choose a session' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Add project' }),
    ).toBeInTheDocument();
    expect(
      topNewSessionButton(),
    ).toBeEnabled();
    expect(screen.getByRole('main')).toHaveFocus();

    await user.click(topNewSessionButton());
    expect(
      within(screen.getByRole('dialog')).getByLabelText('Project'),
    ).toHaveValue('core');
  });
});

describe('live updates', () => {
  it('shows a paused banner and reconnects on Retry', async () => {
    const user = userEvent.setup();
    let calls = 0;
    vi.mocked(subscribeEvents).mockImplementation((_onSession, onError) => {
      calls += 1;
      if (calls === 1) {
        onError?.();
      }
      return () => {};
    });

    render(<App />);

    expect(await screen.findByText('Live updates paused.')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(subscribeEvents).toHaveBeenCalledTimes(2));
    expect(
      screen.queryByText('Live updates paused.'),
    ).not.toBeInTheDocument();
  });
});

describe('session actions', () => {
  it('renames from the header menu', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: /Check close/ }),
    );
    const actions = screen.getByRole('button', { name: 'Session actions' });
    expect(actions).toHaveAttribute('aria-haspopup', 'menu');

    await user.click(actions);
    expect(actions).toHaveAttribute('aria-expanded', 'true');
    await user.click(screen.getByRole('menuitem', { name: 'Rename…' }));

    const dialog = screen.getByRole('dialog');
    const input = within(dialog).getByLabelText('Name');
    expect(input).toHaveValue('Check close');

    await user.clear(input);
    await user.type(input, 'Renamed task');
    await user.click(within(dialog).getByRole('button', { name: 'Rename' }));

    await waitFor(() =>
      expect(renameSession).toHaveBeenCalledWith('s1', 'Renamed task'),
    );
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );

    expect(
      screen.getByRole('heading', { name: 'Renamed task' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('tab', { name: /Renamed task/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Renamed task/ }),
    ).toBeInTheDocument();
  });

  it('copies the folder path and shows a toast', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const fakeNavigator = Object.create(navigator) as Navigator;
    Object.defineProperty(fakeNavigator, 'clipboard', {
      value: { writeText },
    });
    vi.stubGlobal('navigator', fakeNavigator);

    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: /Check close/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy folder path' }));

    expect(writeText).toHaveBeenCalledWith('/dev/web');
    expect(await screen.findByText('Folder path copied.')).toBeInTheDocument();
  });

  it('shows the branch and copies the worktree folder and branch', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const fakeNavigator = Object.create(navigator) as Navigator;
    Object.defineProperty(fakeNavigator, 'clipboard', {
      value: { writeText },
    });
    vi.stubGlobal('navigator', fakeNavigator);

    const branchSession = makeSession({
      id: 's1',
      name: 'Branch task',
      short: 'branch-task',
      branch: 'elesdee/web/branch-task',
      folder: '/data/worktrees/s1',
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [branchSession],
    });

    render(<App />);
    fireEvent.click(
      await screen.findByRole('button', { name: /Branch task/ }),
    );

    expect(
      screen.getByText('elesdee/web/branch-task'),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy folder path' }));
    expect(writeText).toHaveBeenCalledWith('/data/worktrees/s1');

    fireEvent.click(screen.getByRole('button', { name: 'Session actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy branch name' }));
    expect(writeText).toHaveBeenCalledWith('elesdee/web/branch-task');
  });

  it('reopens a closed session and opens its tab', async () => {
    const closed = makeSession({
      id: 's2',
      name: 'Old task',
      short: 'old-task',
      status: 'closed',
      workState: null,
      kind: 'terminal',
      client: null,
      accountId: null,
      branch: 'elesdee/web/old-task',
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [closed],
    });
    vi.mocked(reopenSession).mockResolvedValue({
      ...closed,
      status: 'active',
      workState: 'working',
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: 'Show closed' }));
    fireEvent.contextMenu(screen.getByRole('button', { name: /Old task/ }));
    await user.click(screen.getByRole('menuitem', { name: 'Reopen' }));

    await waitFor(() => expect(reopenSession).toHaveBeenCalledWith('s2'));
    expect(
      await screen.findByRole('tab', { name: /Old task/ }),
    ).toBeInTheDocument();
    expect(await screen.findByText('Session reopened.')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Old task/ })).toHaveFocus();
  });

  it('shows the server message when reopen fails', async () => {
    const closed = makeSession({
      id: 's2',
      name: 'Old task',
      status: 'closed',
      workState: null,
      kind: 'terminal',
      client: null,
      accountId: null,
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [closed],
    });
    vi.mocked(reopenSession).mockRejectedValue(
      new ApiError(422, 'The folder no longer exists.'),
    );

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: 'Show closed' }));
    fireEvent.contextMenu(screen.getByRole('button', { name: /Old task/ }));
    await user.click(screen.getByRole('menuitem', { name: 'Reopen' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The folder no longer exists.',
    );
    expect(
      screen.queryByRole('tab', { name: /Old task/ }),
    ).not.toBeInTheDocument();
  });

  it('deletes a closed session and removes its tab and row', async () => {
    const closed = makeSession({
      id: 's2',
      name: 'Old task',
      short: 'old-task',
      status: 'closed',
      workState: null,
      kind: 'terminal',
      client: null,
      accountId: null,
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [activeSession, closed],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: /Check close/ }),
    );
    await user.click(screen.getByRole('button', { name: 'Show closed' }));
    await user.click(screen.getByRole('button', { name: /Old task/ }));

    const tab = screen.getByRole('tab', { name: /Old task/ });
    fireEvent.contextMenu(tab);
    await user.click(screen.getByRole('menuitem', { name: 'Delete…' }));

    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).getByText(
        'The session and its log are removed from the app. The branch stays.',
      ),
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(deleteSession).toHaveBeenCalledWith('s2'));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );

    expect(
      screen.queryByRole('tab', { name: /Old task/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /Old task/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Check close/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByRole('tab', { name: /Check close/ })).toHaveFocus();
  });
});

describe('closing a session', () => {
  it('removes the tab and keeps the session under the Closed filter', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: /Check close/ }),
    );
    expect(screen.getByRole('tab', { name: /Check close/ })).toBeInTheDocument();
    expect(screen.getByRole('tabpanel')).toHaveAttribute(
      'aria-labelledby',
      'tab-s1',
    );

    await user.click(screen.getByRole('button', { name: 'Session actions' }));
    await user.click(screen.getByRole('menuitem', { name: 'Close…' }));
    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).getByText(
        'The terminal stops. The folder stays as it is.',
      ),
    ).toBeInTheDocument();

    await user.click(
      within(dialog).getByRole('button', { name: 'Close session' }),
    );

    await waitFor(() => expect(closeSession).toHaveBeenCalledWith('s1'));
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole('tab', { name: /Check close/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByText('Choose a session')).toBeInTheDocument();
    expect(
      topNewSessionButton(),
    ).toHaveFocus();

    await user.click(screen.getByRole('button', { name: 'Show closed' }));
    expect(
      screen.getByRole('button', { name: /Check close/ }),
    ).toHaveTextContent('Closed');
  });

  it('keeps the session open when the close is cancelled', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: /Check close/ }),
    );
    await user.click(screen.getByRole('button', { name: 'Session actions' }));
    await user.click(screen.getByRole('menuitem', { name: 'Close…' }));

    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(closeSession).not.toHaveBeenCalled();
    expect(screen.getByRole('tab', { name: /Check close/ })).toBeInTheDocument();
  });
});

describe('usage', () => {
  it('opens the usage panel from the sidebar and closes it', async () => {
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [
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
          name: 'Wallet',
          plan: 'Pay as you go',
          client: 'OpenCode 2',
          provider: 'OpenRouter',
          auth: 'key',
          sample: false,
          rows: [{ label: 'Credit', value: '$12.40', min: '$5.00' }],
          foot: '',
          server: null,
          model: null,
        },
      ],
      sessions: [activeSession],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: /^Usage/ }));

    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).getByRole('heading', { name: 'Usage' }),
    ).toBeInTheDocument();
    expect(within(dialog).getByText('Work')).toBeInTheDocument();
    expect(within(dialog).getByText('42% left')).toBeInTheDocument();
    expect(within(dialog).getByText('Wallet')).toBeInTheDocument();
    expect(within(dialog).getByText('$12.40')).toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: 'Close usage' }));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(screen.getByRole('button', { name: /^Usage/ })).toHaveFocus();
  });
});

describe('project switching', () => {
  it('restores each project last active tab when it is selected again', async () => {
    const webTask = makeSession({
      id: 's1',
      name: 'Web task',
      short: 'web-task',
      projectId: 'p1',
      branch: 'elesdee/web/web-task',
    });
    const coreTask = makeSession({
      id: 's2',
      name: 'Core task',
      short: 'core-task',
      projectId: 'p2',
      branch: 'elesdee/core/core-task',
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [
        { id: 'p1', name: 'web', folder: '/dev/web' },
        { id: 'p2', name: 'core', folder: '/dev/core' },
      ],
      accounts: [],
      sessions: [webTask, coreTask],
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: /Web task/ }));
    expect(screen.getByRole('tab', { name: /Web task/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );

    await user.click(screen.getByRole('button', { name: 'core, 1 session' }));

    expect(screen.queryByRole('tab', { name: /Web task/ })).not.toBeInTheDocument();
    expect(screen.getByText('Open a session from the sidebar.')).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { name: 'Choose a session' }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /Core task/ }));
    expect(screen.getByRole('tab', { name: /Core task/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );

    await user.click(screen.getByRole('button', { name: 'web, 1 session' }));
    expect(screen.getByRole('tab', { name: /Web task/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(screen.getByRole('heading', { name: 'Web task' })).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'core, 1 session' }));
    expect(screen.getByRole('tab', { name: /Core task/ })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(
      screen.getByRole('heading', { name: 'Core task' }),
    ).toBeInTheDocument();
  });
});

describe('permission requests', () => {
  const permissionSession = makeSession({
    id: 's1',
    name: 'Permission task',
    short: 'permission-task',
    waitingFor: 'permission',
    pendingPermission: {
      requestId: 'req-1',
      tool: 'Bash',
      summary: 'pnpm test --filter web',
    },
  });

  function bootstrapPermission() {
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [permissionSession],
    });
  }

  async function openCard(user: ReturnType<typeof userEvent.setup>) {
    render(<App />);
    await user.click(
      await screen.findByRole('button', { name: /Permission task/ }),
    );
    expect(
      await screen.findByRole('heading', { name: 'Permission needed' }),
    ).toBeInTheDocument();
  }

  it('sends allow and keeps the card until the session event clears it', async () => {
    bootstrapPermission();
    let resolveAnswer: () => void = () => {};
    vi.mocked(respondPermission).mockReturnValue(
      new Promise<void>((resolve) => {
        resolveAnswer = resolve;
      }),
    );
    let emit: Parameters<typeof subscribeEvents>[0] | undefined;
    vi.mocked(subscribeEvents).mockImplementation((onSession) => {
      emit = onSession;
      return () => {};
    });

    const user = userEvent.setup();
    await openCard(user);

    await user.click(screen.getByRole('button', { name: 'Allow' }));

    await waitFor(() =>
      expect(respondPermission).toHaveBeenCalledWith('s1', 'req-1', 'allow'),
    );
    expect(
      screen.getByRole('button', { name: 'Allowing…' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeDisabled();

    resolveAnswer();
    await waitFor(() =>
      expect(
        screen.getByRole('heading', { name: 'Permission needed' }),
      ).toBeInTheDocument(),
    );

    act(() => {
      emit?.({
        ...permissionSession,
        workState: 'working',
        waitingFor: null,
        pendingPermission: null,
      });
    });

    await waitFor(() =>
      expect(
        screen.queryByRole('heading', { name: 'Permission needed' }),
      ).not.toBeInTheDocument(),
    );
  });

  it('sends deny and shows the denying state', async () => {
    bootstrapPermission();
    vi.mocked(respondPermission).mockResolvedValue(undefined);

    const user = userEvent.setup();
    await openCard(user);

    await user.click(screen.getByRole('button', { name: 'Deny' }));

    await waitFor(() =>
      expect(respondPermission).toHaveBeenCalledWith('s1', 'req-1', 'deny'),
    );
    expect(
      screen.getByRole('button', { name: 'Denying…' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Allow' })).toBeDisabled();
  });

  it('hides the card and shows a toast when the request is already resolved', async () => {
    bootstrapPermission();
    vi.mocked(respondPermission).mockRejectedValue(
      new ApiError(409, 'No pending request.'),
    );

    const user = userEvent.setup();
    await openCard(user);

    await user.click(screen.getByRole('button', { name: 'Allow' }));

    expect(
      await screen.findByText(
        'That permission request was already resolved.',
      ),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        screen.queryByRole('heading', { name: 'Permission needed' }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByText('Waiting for you')).not.toBeInTheDocument();
  });
});


describe('settings', () => {
  it('opens from the footer and restores focus on close', async () => {
    const user = userEvent.setup();
    render(<App />);

    const settingsButton = await screen.findByRole('button', {
      name: 'Settings',
    });
    await user.click(settingsButton);

    const dialog = screen.getByRole('dialog', { name: 'Settings' });
    expect(
      within(dialog).getByRole('heading', { name: 'Settings' }),
    ).toBeInTheDocument();

    await user.keyboard('{Escape}');

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Settings' }),
      ).not.toBeInTheDocument(),
    );
    expect(settingsButton).toHaveFocus();
  });

  it('adds a Google account and offers it for Antigravity', async () => {
    const google: Account = {
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
    vi.mocked(createAccount).mockResolvedValue(google);
    vi.mocked(listAccounts).mockResolvedValue([google]);

    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole('button', { name: 'Settings' }));
    const settings = screen.getByRole('dialog', { name: 'Settings' });
    await user.click(
      within(settings).getByRole('button', { name: 'Add account' }),
    );

    const add = screen.getByRole('dialog', { name: 'Add account' });
    const key = 'secret-key';
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    await within(add).findByRole('option', { name: 'Google' });
    await user.selectOptions(within(add).getByLabelText('Provider'), 'google');
    await user.type(within(add).getByLabelText('Account name'), 'Google work');
    await user.type(
      within(add).getByLabelText('API key', { selector: '#add-account-key' }),
      key,
    );
    await user.click(within(add).getByRole('button', { name: 'Add account' }));

    await waitFor(() =>
      expect(createAccount).toHaveBeenCalledWith({
        name: 'Google work',
        providerId: 'google',
        auth: 'key',
        key,
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Add account' }),
      ).not.toBeInTheDocument(),
    );

    expect(within(settings).getByText('Google work')).toBeInTheDocument();
    expect(
      within(settings).getByText('Antigravity · Google AI'),
    ).toBeInTheDocument();
    expect(within(settings).getByText('No usage data')).toBeInTheDocument();

    const logged = [...log.mock.calls, ...error.mock.calls, ...warn.mock.calls]
      .flat()
      .map(String)
      .join(' ');
    expect(logged).not.toContain(key);

    await user.click(
      screen.getByRole('button', { name: 'Close settings' }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Settings' }),
      ).not.toBeInTheDocument(),
    );

    await user.click(screen.getByRole('button', { name: /^Usage/ }));
    const usage = screen.getByRole('dialog', { name: 'Usage' });
    expect(
      within(usage).getByRole('region', { name: 'Google work' }),
    ).toHaveTextContent('No usage data');
    await user.click(within(usage).getByRole('button', { name: 'Close usage' }));

    await user.click(topNewSessionButton());
    const sessionDialog = screen.getByRole('dialog', { name: 'New session' });
    await user.click(
      within(sessionDialog).getByRole('radio', { name: 'Antigravity' }),
    );
    await user.click(within(sessionDialog).getByLabelText('Account'));
    await user.click(
      await screen.findByRole('option', { name: /Google work/ }),
    );

    expect(within(sessionDialog).getByText('No usage data')).toBeInTheDocument();
  });
});


describe('shell controls and closed state', () => {
  it('starts a new session from the empty main', async () => {
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [],
    });

    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole('heading', { name: 'Choose a session' });
    const empty = document.querySelector('.empty-main') as HTMLElement;
    await user.click(
      within(empty).getByRole('button', { name: 'New session' }),
    );

    const dialog = screen.getByRole('dialog', { name: 'New session' });
    expect(
      within(dialog).getByRole('combobox', { name: 'Project' }),
    ).toHaveValue('web');
  });

  it('opens the form from a project plus and the tab plus', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: /Check close/ }),
    );

    await user.click(
      screen.getByRole('button', { name: 'New session in web' }),
    );
    let dialog = screen.getByRole('dialog', { name: 'New session' });
    expect(
      within(dialog).getByRole('combobox', { name: 'Project' }),
    ).toHaveValue('web');
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    const tabbar = document.querySelector('.tabbar') as HTMLElement;
    await user.click(
      within(tabbar).getByRole('button', { name: 'New session' }),
    );
    dialog = screen.getByRole('dialog', { name: 'New session' });
    expect(
      within(dialog).getByRole('combobox', { name: 'Project' }),
    ).toHaveValue('web');
  });

  it('reopens from the closed banner', async () => {
    const closed = makeSession({
      id: 's2',
      name: 'Old task',
      short: 'old-task',
      status: 'closed',
      workState: null,
      kind: 'terminal',
      client: null,
      accountId: null,
      branch: null,
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [closed],
    });
    vi.mocked(reopenSession).mockResolvedValue({
      ...closed,
      status: 'active',
      workState: 'working',
    });

    const user = userEvent.setup();
    render(<App />);

    await user.click(
      await screen.findByRole('button', { name: 'Show closed' }),
    );
    await user.click(screen.getByRole('button', { name: /Old task/ }));

    await user.click(
      await screen.findByRole('button', { name: 'Reopen' }),
    );

    await waitFor(() => expect(reopenSession).toHaveBeenCalledWith('s2'));
    expect(await screen.findByRole('tab', { name: /Old task/ })).toBeVisible();
  });

  it('copies from the branch pill menu', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const fakeNavigator = Object.create(navigator) as Navigator;
    Object.defineProperty(fakeNavigator, 'clipboard', {
      value: { writeText },
    });
    vi.stubGlobal('navigator', fakeNavigator);

    const branchSession = makeSession({
      id: 's1',
      name: 'Branch task',
      short: 'branch-task',
      branch: 'elesdee/web/branch-task',
      folder: '/data/worktrees/s1',
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [branchSession],
    });

    render(<App />);
    fireEvent.click(
      await screen.findByRole('button', { name: /Branch task/ }),
    );

    fireEvent.click(
      screen.getByRole('button', { name: 'elesdee/web/branch-task' }),
    );
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy branch name' }));

    expect(writeText).toHaveBeenCalledWith('elesdee/web/branch-task');
    expect(await screen.findByText('Branch name copied.')).toBeInTheDocument();
  });
});


describe('desktop shell', () => {
  it('shows the active project in the browser titlebar without drag chrome', async () => {
    render(<App />);

    const titlebar = await screen.findByRole('banner');
    expect(titlebar).not.toHaveClass('is-tauri');
    expect(titlebar).not.toHaveAttribute('data-tauri-drag-region');
    expect(titlebar).toHaveTextContent('elesdee');
    expect(titlebar).toHaveTextContent('web');
  });

  it('uses the Tauri drag region and titlebar padding in the desktop shell', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    render(<App />);

    const titlebar = await screen.findByRole('banner');
    expect(titlebar).toHaveClass('is-tauri');
    expect(titlebar).toHaveAttribute('data-tauri-drag-region', 'true');
    expect(titlebar).toHaveTextContent('elesdee');
    expect(titlebar).toHaveTextContent('web');
  });

  function branchBootstrap() {
    const branchSession = makeSession({
      id: 's1',
      name: 'Branch task',
      short: 'branch-task',
      branch: 'elesdee/web/branch-task',
      folder: '/data/worktrees/s1',
    });
    vi.mocked(bootstrap).mockResolvedValue({
      projects: [{ id: 'p1', name: 'web', folder: '/dev/web' }],
      accounts: [],
      sessions: [branchSession],
    });
    return branchSession;
  }

  async function openBranchMenu(user: ReturnType<typeof userEvent.setup>) {
    await user.click(await screen.findByRole('button', { name: /Branch task/ }));
    await user.click(
      screen.getByRole('button', { name: 'elesdee/web/branch-task' }),
    );
  }

  it('hides the open actions in a normal browser', async () => {
    branchBootstrap();
    const user = userEvent.setup();
    render(<App />);
    await openBranchMenu(user);

    expect(
      screen.queryByRole('menuitem', { name: 'Open folder' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Open in Terminal' }),
    ).not.toBeInTheDocument();
  });

  it('opens the folder and terminal through the Tauri commands', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    branchBootstrap();
    const user = userEvent.setup();
    render(<App />);

    await openBranchMenu(user);
    await user.click(screen.getByRole('menuitem', { name: 'Open folder' }));
    expect(openFolder).toHaveBeenCalledWith('/data/worktrees/s1');

    await openBranchMenu(user);
    await user.click(
      screen.getByRole('menuitem', { name: 'Open in Terminal' }),
    );
    expect(openTerminal).toHaveBeenCalledWith('/data/worktrees/s1');
  });

  it('shows the returned shell error as a toast', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(openFolder).mockRejectedValue('No such folder.');
    branchBootstrap();
    const user = userEvent.setup();
    render(<App />);

    await openBranchMenu(user);
    await user.click(screen.getByRole('menuitem', { name: 'Open folder' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'No such folder.',
    );
  });
});
