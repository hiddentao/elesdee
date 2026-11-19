import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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
  onOpenUsage?: () => void;
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
    onOpenUsage = vi.fn(),
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
      onOpenUsage={onOpenUsage}
      onOpenSettings={onOpenSettings}
    />,
  );
}

describe('Sidebar', () => {
  it('shows the waiting text and the session kind and client', () => {
    renderSidebar([
      makeSession({ waitingFor: 'permission' }),
      makeClosed(),
    ]);

    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.getByText('Waiting for you')).toBeInTheDocument();
    expect(screen.getByText('Agent · Claude Code')).toBeInTheDocument();
    expect(screen.queryByText('Old task')).not.toBeInTheDocument();
  });

  it('filters between active and closed sessions', async () => {
    const user = userEvent.setup();
    renderSidebar([makeSession(), makeClosed()]);

    await user.click(screen.getByRole('button', { name: /^Closed/ }));
    expect(screen.getByText('Old task')).toBeInTheDocument();
    expect(screen.getByText('Fix login')).toBeInTheDocument();
    expect(screen.getByText('Agent · Claude Code · Input')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    expect(screen.queryByText('Fix login')).not.toBeInTheDocument();
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

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    await user.click(screen.getByRole('button', { name: /^Closed/ }));

    expect(
      screen.queryByRole('button', { name: /Fix login/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Old task/ }),
    ).toHaveTextContent('core');

    await user.click(screen.getByRole('button', { name: /Old task/ }));
    expect(onSelectSession).toHaveBeenCalledWith(
      expect.objectContaining({ id: 's2' }),
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

    await user.click(screen.getByRole('button', { name: /^Closed/ }));

    expect(
      screen.getByRole('button', { name: 'Collapse core' }),
    ).toHaveAttribute('aria-expanded', 'true');
    expect(
      screen.getByRole('button', { name: /Old task/ }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Closed/ }));

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

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    await user.click(screen.getByRole('button', { name: /^Active/ }));

    expect(
      screen.getByRole('button', { name: 'Collapse core' }),
    ).toHaveAttribute('aria-expanded', 'true');

    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');

    await user.click(screen.getByRole('button', { name: /^Closed/ }));

    expect(
      screen.getByRole('button', { name: 'Expand core' }),
    ).toHaveAttribute('aria-expanded', 'false');
  });

  it('expands a collapsed project when a session closes under the filter', async () => {
    const user = userEvent.setup();
    const other = makeSession({
      id: 's3',
      projectId: 'p2',
      name: 'Other task',
      branch: 'elesdee/web/other-task',
      preview: 'Reviewing the web app',
    });
    const view = renderSidebar(
      [makeSession(), other],
      { projects: [project, webProject] },
    );

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    await user.click(screen.getByRole('button', { name: 'Collapse core' }));
    await user.click(screen.getByRole('button', { name: /^Closed/ }));

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
        onOpenUsage={vi.fn()}
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

  it('keeps a project visible when the filter hides all its sessions', () => {
    renderSidebar([makeClosed()]);

    expect(screen.getByText('No active sessions')).toBeInTheDocument();
    expect(
      screen.queryByText('No sessions match this filter.'),
    ).not.toBeInTheDocument();
  });

  it('names the active filter under a project with hidden sessions', async () => {
    const user = userEvent.setup();
    renderSidebar(
      [
        makeSession({ id: 's1', projectId: 'p1', name: 'Fix login' }),
        makeClosed({ id: 's2', projectId: 'p2', name: 'Old task' }),
      ],
      { projects: [project, webProject] },
    );

    expect(screen.getByText('No active sessions')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Closed/ }));
    expect(screen.queryByText('No closed sessions')).not.toBeInTheDocument();
    expect(screen.queryByText('No active sessions')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Active/ }));
    expect(screen.getByText('No closed sessions')).toBeInTheDocument();
    expect(screen.queryByText('No active sessions')).not.toBeInTheDocument();
  });

  it('shows the tightest account usage and opens the panel', async () => {
    const user = userEvent.setup();
    const onOpenUsage = vi.fn();
    const account: Account = {
      id: 'a1',
      name: 'Work',
      plan: 'Max',
      client: 'Claude Code',
      provider: 'Anthropic',
      auth: 'subscription',
      rows: [{ label: '5-hour', used: 58 }],
      foot: 'Resets soon.',
      server: null,
      model: null,
    };

    renderSidebar([makeSession()], {
      accounts: [account],
      onOpenUsage,
    });

    expect(screen.getByText('Usage')).toBeInTheDocument();
    expect(screen.getByText('Work')).toBeInTheDocument();
    expect(screen.getByText('5-hour')).toBeInTheDocument();
    expect(screen.getByText('42% left')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: /^Usage/ }));

    expect(onOpenUsage).toHaveBeenCalledTimes(1);
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
