import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Account, Project, Session } from '../api/types';
import { coreProject, makeSession as baseSession } from '../test/fixtures';
import { Sidebar } from './Sidebar';

const project = coreProject;
const webProject: Project = { id: 'p2', name: 'web', folder: '/dev/web' };

function makeSession(overrides: Partial<Session> = {}): Session {
  return baseSession({
    name: 'Fix login',
    short: 'fix-login',
    workState: 'input',
    branch: 'elesdee/core/fix-login',
    folder: '/dev/core',
    preview: 'Running the login tests',
    ...overrides,
  });
}

function makeClosed(overrides: Partial<Session> = {}): Session {
  return makeSession({
    id: 's2',
    name: 'Old task',
    short: 'old-task',
    status: 'closed',
    workState: null,
    waitingFor: null,
    preview: 'Nothing running',
    ...overrides,
  });
}

function makeAccount(overrides: Partial<Account> = {}): Account {
  return {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'Anthropic',
    auth: 'subscription',
    sample: true,
    rows: [{ label: '5-hour', used: 58 }],
    foot: 'Resets soon.',
    server: null,
    model: null,
    ...overrides,
  };
}

interface SidebarOptions {
  projects?: Project[];
  accounts?: Account[];
  onSelectProject?: (project: Project) => void;
  onSelectSession?: (session: Session) => void;
  onSessionMenu?: (
    session: Session,
    point: { x: number; y: number },
    trigger: HTMLElement,
  ) => void;
  onAddProject?: () => void;
  onNewSession?: (projectId: string) => void;
  onProjectSettings?: (project: Project) => void;
  onOpenSettings?: () => void;
}

function renderSidebar(
  sessions: Session[],
  {
    projects = [project],
    accounts = [],
    onSelectProject = vi.fn(),
    onSelectSession = vi.fn(),
    onSessionMenu = vi.fn(),
    onAddProject = vi.fn(),
    onNewSession = vi.fn(),
    onProjectSettings = vi.fn(),
    onOpenSettings = vi.fn(),
  }: SidebarOptions = {},
) {
  return render(
    <Sidebar
      projects={projects}
      sessions={sessions}
      accounts={accounts}
      activeProjectId="p1"
      activeSessionId="s1"
      onSelectProject={onSelectProject}
      onSelectSession={onSelectSession}
      onSessionMenu={onSessionMenu}
      onAddProject={onAddProject}
      onNewSession={onNewSession}
      onProjectSettings={onProjectSettings}
      onOpenSettings={onOpenSettings}
    />,
  );
}

describe('Sidebar', () => {
  it('shows two-line rows with the status word and accessible detail', () => {
    renderSidebar([
      makeSession(),
      makeSession({
        id: 's2',
        name: 'Wait task',
        waitingFor: 'permission',
      }),
    ]);

    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.getByText('Running the login tests')).toBeInTheDocument();
    expect(
      within(screen.getByRole('button', { name: /Fix login/ })).getByText(
        'Active',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Waiting for you')).toBeInTheDocument();
    expect(
      screen.getByText('Agent · Claude Code · Input'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Agent · Claude Code · Waiting'),
    ).toBeInTheDocument();
  });

  it('shows the closed status word when closed sessions are shown', async () => {
    const user = userEvent.setup();
    renderSidebar([makeSession(), makeClosed()]);

    await user.click(screen.getByRole('button', { name: 'Show closed' }));

    expect(screen.getByText('Old task')).toBeInTheDocument();
    expect(
      within(screen.getByRole('button', { name: /Old task/ })).getByText(
        'Closed',
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole('button', { name: /Fix login/ })).getByText(
        'Active',
      ),
    ).toBeInTheDocument();
  });

  it('adds closed sessions without emptying the list', async () => {
    const user = userEvent.setup();
    renderSidebar([makeSession(), makeClosed()]);

    await user.click(screen.getByRole('button', { name: 'Show closed' }));
    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.getByText('Old task')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.queryByText('Old task')).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /^Active/ }),
    ).toHaveAttribute('aria-pressed', 'true');

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    expect(screen.getByText('Old task')).toBeInTheDocument();
  });

  it('opens a session when its row is clicked', async () => {
    const user = userEvent.setup();
    const onSelectSession = vi.fn();
    renderSidebar([makeSession()], { onSelectSession });

    await user.click(screen.getByRole('button', { name: /Fix login/ }));

    expect(onSelectSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: 's1' }),
    );
  });

  it('opens the session menu on right-click', () => {
    const onSessionMenu = vi.fn();
    renderSidebar([makeSession()], { onSessionMenu });

    fireEvent.contextMenu(
      screen.getByRole('button', { name: /Fix login/ }),
    );

    expect(onSessionMenu).toHaveBeenCalledWith(
      expect.objectContaining({ id: 's1' }),
      expect.objectContaining({ x: expect.any(Number), y: expect.any(Number) }),
      expect.anything(),
    );
  });

  it('switches projects from the name and collapses from the chevron alone', async () => {
    const user = userEvent.setup();
    const onSelectProject = vi.fn();
    renderSidebar(
      [
        makeSession(),
        makeSession({
          id: 's2',
          projectId: 'p2',
          name: 'Other task',
          branch: 'elesdee/web/other-task',
          preview: 'Reviewing the web app',
        }),
      ],
      { projects: [project, webProject], onSelectProject },
    );

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));

    expect(onSelectProject).not.toHaveBeenCalled();
    expect(
      screen.getByRole('button', { name: 'Collapse web' }),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(document.getElementById('project-p1-sessions')).toHaveAttribute(
      'hidden',
    );
    expect(
      document.getElementById('project-p2-sessions'),
    ).not.toHaveAttribute('hidden');
    expect(
      screen.getByRole('button', { name: /Other task/ }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^core/ }));

    expect(onSelectProject).toHaveBeenCalledWith(project);
    expect(document.getElementById('project-p1-sessions')).toHaveAttribute(
      'hidden',
    );
    expect(
      screen.getByRole('button', { name: /^core/ }),
    ).toHaveAttribute('aria-current', 'true');
  });

  it('collapses a project into a blot pill with a session tooltip', async () => {
    const user = userEvent.setup();
    renderSidebar([
      makeSession(),
      makeSession({
        id: 's2',
        name: 'Wait task',
        waitingFor: 'permission',
      }),
    ]);

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));

    const group = screen.getByRole('group', {
      name: 'core, 2 active sessions',
    });
    expect(within(group).getByText('2')).toBeInTheDocument();
    expect(within(group).getByText('Fix login')).toBeInTheDocument();
    expect(within(group).getByText('Wait task')).toBeInTheDocument();
    expect(
      within(group).getByText('No attention needed'),
    ).toBeInTheDocument();
    expect(within(group).getByText('Waiting for you')).toBeInTheDocument();
    expect(
      within(group).getByRole('button', {
        name: 'Fix login, No attention needed',
      }),
    ).toBeInTheDocument();
  });

  it('opens a new session for a project from its plus', async () => {
    const user = userEvent.setup();
    const onNewSession = vi.fn();
    const onSelectProject = vi.fn();
    renderSidebar([makeSession()], { onNewSession, onSelectProject });

    await user.click(
      screen.getByRole('button', { name: 'New session in core' }),
    );

    expect(onNewSession).toHaveBeenCalledWith('p1');
    expect(onSelectProject).not.toHaveBeenCalled();
  });

  it('lists sessions flat with their project and applies filters', async () => {
    const user = userEvent.setup();
    const onSelectSession = vi.fn();
    renderSidebar(
      [
        makeSession(),
        makeClosed(),
        makeSession({
          id: 's3',
          projectId: 'p2',
          name: 'Other task',
          branch: 'elesdee/web/other-task',
        }),
      ],
      {
        projects: [project, webProject],
        onSelectSession,
      },
    );

    expect(
      screen.getByRole('radiogroup', { name: 'Session list view' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Grouped' })).toBeChecked();

    await user.click(screen.getByRole('radio', { name: 'Flat' }));

    expect(screen.getByRole('radio', { name: 'Flat' })).toBeChecked();
    expect(
      screen.queryByRole('button', { name: /^core/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Fix login/ }),
    ).toHaveTextContent('core');
    expect(
      screen.getByRole('button', { name: /Other task/ }),
    ).toHaveTextContent('web');
    expect(
      screen.queryByRole('button', { name: /Old task/ }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Show closed' }));
    expect(
      screen.getByRole('button', { name: /Old task/ }),
    ).toHaveTextContent('core');

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    expect(
      screen.queryByRole('button', { name: /Old task/ }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /Fix login/ }));
    expect(onSelectSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: 's1' }),
    );
  });

  it('expands a filtered project without losing the saved collapse', async () => {
    const user = userEvent.setup();
    renderSidebar(
      [
        makeClosed(),
        makeSession({
          id: 's3',
          projectId: 'p2',
          name: 'Other task',
          branch: 'elesdee/web/other-task',
        }),
      ],
      { projects: [project, webProject] },
    );

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');

    await user.click(screen.getByRole('button', { name: 'Show closed' }));

    expect(
      screen.getByRole('button', { name: 'Collapse core' }),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(
      screen.getByRole('button', { name: /Old task/ }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Show closed' }));

    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');
  });

  it('keeps a manual collapse after filter expansion when another filter is toggled', async () => {
    const user = userEvent.setup();
    renderSidebar(
      [
        makeSession(),
        makeClosed(),
        makeSession({
          id: 's3',
          projectId: 'p2',
          name: 'Other task',
          branch: 'elesdee/web/other-task',
          preview: 'Reviewing the web app',
        }),
      ],
      { projects: [project, webProject] },
    );

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    await user.click(screen.getByRole('button', { name: 'Show closed' }));

    expect(
      screen.getByRole('button', { name: 'Collapse core' }),
    ).toHaveAttribute('aria-expanded', 'true');

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');

    await user.click(screen.getByRole('button', { name: /^Active/ }));

    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');
  });

  it('expands a collapsed project when a session appears under the filter', async () => {
    const user = userEvent.setup();
    const other = makeSession({
      id: 's3',
      projectId: 'p2',
      name: 'Other task',
      branch: 'elesdee/web/other-task',
      preview: 'Reviewing the web app',
    });
    const view = renderSidebar(
      [other],
      { projects: [project, webProject] },
    );

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    await user.click(screen.getByRole('button', { name: 'Show closed' }));

    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');

    view.rerender(
      <Sidebar
        projects={[project, webProject]}
        sessions={[
          makeClosed({ id: 's1', projectId: 'p1', name: 'Fix login' }),
          other,
        ]}
        accounts={[]}
        activeProjectId="p1"
        activeSessionId="s1"
        onSelectProject={vi.fn()}
        onSelectSession={vi.fn()}
        onSessionMenu={vi.fn()}
        onAddProject={vi.fn()}
        onNewSession={vi.fn()}
        onProjectSettings={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: 'Collapse core' }),
      ).toHaveAttribute('aria-expanded', 'true'),
    );
  });

  it('lists projects when there are no sessions', () => {
    renderSidebar([]);

    expect(
      screen.getByRole('button', { name: /^core,/ }),
    ).toBeInTheDocument();
    expect(screen.getByText('No sessions')).toBeInTheDocument();
  });

  it('opens the project settings from the project header', async () => {
    const user = userEvent.setup();
    const onProjectSettings = vi.fn();
    renderSidebar([], { onProjectSettings });

    await user.click(
      screen.getByRole('button', { name: 'Project settings for core' }),
    );

    expect(onProjectSettings).toHaveBeenCalledWith(project);
  });

  it('keeps a project visible when the filter hides all its sessions', () => {
    renderSidebar([makeClosed()]);

    expect(screen.getByText('No active sessions')).toBeInTheDocument();
  });

  it('hides sidebar chrome before the first project', () => {
    renderSidebar([], { projects: [] });

    expect(screen.getByText('No sessions yet.')).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /^Active/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Show closed' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('radiogroup', { name: 'Session list view' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Add project' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /^Usage/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Settings' }),
    ).toBeInTheDocument();
  });

  it('shows three compact usage rows and opens the account popover', async () => {
    const user = userEvent.setup();
    const accounts = [
      makeAccount({ id: 'a1', name: 'Work' }),
      makeAccount({
        id: 'a2',
        name: 'ChatGPT Pro',
        client: 'Codex',
        rows: [{ label: 'Weekly', used: 78 }],
      }),
      makeAccount({
        id: 'a3',
        name: 'DeepSeek API',
        rows: [{ label: 'Balance', value: '$12.40', min: '$5.00' }],
      }),
      makeAccount({ id: 'a4', name: 'LiteLLM gateway', rows: [] }),
    ];

    renderSidebar([makeSession()], { accounts });

    const usage = screen.getByRole('button', { name: /^Usage/ });
    expect(usage).toHaveTextContent('Work');
    expect(usage).toHaveTextContent('ChatGPT Pro');
    expect(usage).toHaveTextContent('DeepSeek API');
    expect(usage).not.toHaveTextContent('LiteLLM gateway');
    expect(usage).toHaveTextContent('42% left');
    expect(usage).toHaveTextContent('$12.40');

    await user.click(usage);
    const popover = screen.getByRole('dialog', { name: 'Usage' });
    expect(within(popover).getByText('LiteLLM gateway')).toBeInTheDocument();
    expect(within(popover).getByText('No usage data')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('dialog', { name: 'Usage' }),
    ).not.toBeInTheDocument();
    expect(usage).toHaveFocus();

    await user.click(usage);
    fireEvent.mouseDown(document.body);
    expect(
      screen.queryByRole('dialog', { name: 'Usage' }),
    ).not.toBeInTheDocument();
  });

  it('opens settings from the footer', async () => {
    const user = userEvent.setup();
    const onOpenSettings = vi.fn();
    renderSidebar([makeSession()], { onOpenSettings });

    await user.click(screen.getByRole('button', { name: 'Settings' }));

    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it('offers an Add project control when projects exist', async () => {
    const user = userEvent.setup();
    const onAddProject = vi.fn();
    renderSidebar([makeSession()], { onAddProject });

    await user.click(screen.getByRole('button', { name: 'Add project' }));

    expect(onAddProject).toHaveBeenCalledTimes(1);
  });
});
