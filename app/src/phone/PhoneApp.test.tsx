import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  bootstrap,
  createSession,
  switchSessionConfig,
} from '../api/client';
import type {
  BootstrapResponse,
  Question,
  Session,
  SessionRequest,
} from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { coreProject, makeSession } from '../test/fixtures';
import PhoneApp from './PhoneApp';

vi.mock('../api/client', () => ({
  answerQuestion: vi.fn(),
  approveSessionRequest: vi.fn(),
  bootstrap: vi.fn(),
  closeSession: vi.fn(),
  createSession: vi.fn(),
  denySessionRequest: vi.fn(),
  listBranches: vi.fn(),
  respondPermission: vi.fn(),
  sendInput: vi.fn(),
  switchSessionConfig: vi.fn(),
}));

const handlers = vi.hoisted(() => ({
  session: null as ((session: Session) => void) | null,
  aux: null as ((event: { type: string; [key: string]: unknown }) => void) | null,
}));

vi.mock('../api/events', () => ({
  subscribeEvents: vi.fn(
    (handler: (session: Session) => void) => {
      handlers.session = handler;
      return () => undefined;
    },
  ),
  subscribeAux: vi.fn(
    (handler: (event: { type: string; [key: string]: unknown }) => void) => {
      handlers.aux = handler;
      return () => undefined;
    },
  ),
  subscribeHistory: vi.fn(() => () => undefined),
}));

vi.mock('../components/TerminalPane', () => ({
  TerminalPane: () => <div>Terminal pane</div>,
}));

const config = {
  id: 'c1',
  name: 'Claude Max',
  providerId: 'max',
  client: 'Claude Code' as const,
  model: null,
  effort: null,
  args: [],
  env: [],
  config: {},
  createdAt: '2026-11-19T10:00:00Z',
  updatedAt: '2026-11-19T10:00:00Z',
};

function response(
  overrides: Partial<BootstrapResponse> = {},
): BootstrapResponse {
  return {
    projects: [coreProject],
    providers: [],
    configs: [config],
    sessions: [],
    questions: [],
    sessionRequests: [],
    settings: { startAtLogin: false, keepAwake: false },
    ...overrides,
  };
}

const working = makeSession({
  id: 'w',
  name: 'Working session',
  workState: 'working',
});

const waiting = makeSession({
  id: 'wait',
  name: 'Waiting session',
  workState: 'waiting',
  waitingFor: 'input',
  preview: 'Waiting to push the fix',
});

const paused = makeSession({
  id: 'p',
  name: 'Paused session',
  workState: 'paused',
});

const done = makeSession({
  id: 'd',
  name: 'Done session',
  workState: 'done',
});

function renderPhone() {
  return render(
    <LanguageProvider>
      <PhoneApp />
    </LanguageProvider>,
  );
}

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
    configurable: true,
    writable: true,
    value: () => undefined,
  });
  window.localStorage.clear();
  window.history.replaceState(null, '', '/');
  vi.mocked(createSession).mockReset();
  vi.mocked(switchSessionConfig).mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe('phone empty state', () => {
  it('explains that a project must be added on the computer', async () => {
    vi.mocked(bootstrap).mockResolvedValue(
      response({ projects: [], configs: [], sessions: [] }),
    );

    renderPhone();

    expect(await screen.findByText('No projects yet')).toBeInTheDocument();
    expect(
      screen.getByText(
        'Add a project on the computer to start a session.',
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'New session' }),
    ).toBeDisabled();
    expect(
      screen.queryByRole('group', { name: 'Session filter' }),
    ).not.toBeInTheDocument();
  });
});

describe('phone session list', () => {
  it('shows only working and waiting sessions under Running', async () => {
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, waiting, paused, done] }),
    );
    const user = userEvent.setup();

    renderPhone();
    await screen.findByText('Working session');

    await user.click(screen.getByRole('button', { name: 'Running' }));

    expect(screen.getByText('Working session')).toBeInTheDocument();
    expect(screen.getByText('Waiting session')).toBeInTheDocument();
    expect(screen.queryByText('Paused session')).not.toBeInTheDocument();
    expect(screen.queryByText('Done session')).not.toBeInTheDocument();
  });

  it('keeps the preview and one status on a waiting row', async () => {
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [waiting] }),
    );

    renderPhone();
    const name = await screen.findByText('Waiting session');
    const row = name.closest('.phone-row') as HTMLElement;

    expect(row.querySelector('.phone-row-preview')).toHaveTextContent(
      'Waiting to push the fix',
    );
    expect(row.querySelector('.phone-row-state')).toHaveTextContent('Waiting');
    expect(row).not.toHaveTextContent('Waiting for you');
  });

  it('keeps closed sessions out of the All filter', async () => {
    const closed = makeSession({
      id: 'c',
      name: 'Closed session',
      status: 'closed',
      workState: null,
      waitingFor: null,
    });
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, closed] }),
    );

    renderPhone();
    await screen.findByText('Working session');

    expect(screen.queryByText('Closed session')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'All' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('says when no session matches the filter', async () => {
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    const user = userEvent.setup();

    renderPhone();
    await screen.findByText('Working session');

    await user.click(screen.getByRole('button', { name: 'Waiting' }));

    expect(
      screen.getByText('No sessions match this filter.'),
    ).toBeInTheDocument();
    expect(screen.queryByText('No sessions yet.')).not.toBeInTheDocument();
  });

  it('shows the work state on the active tab sheet row', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, waiting] }),
    );
    const user = userEvent.setup();

    renderPhone();
    await user.click(
      await screen.findByRole('button', { name: /All tabs/ }),
    );

    const sheet = await screen.findByRole('dialog', { name: 'All tabs' });
    const activeRow = sheet.querySelector(
      '.phone-sheet-row.is-on',
    ) as HTMLElement;
    const meta = activeRow.querySelector('.phone-sheet-row-meta');
    expect(meta).toHaveTextContent('Working');
    expect(meta).not.toHaveTextContent('Open');
  });
});

describe('phone sheets', () => {
  it('takes focus, marks the page inert and returns focus on Escape', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    const user = userEvent.setup();

    const { container } = renderPhone();
    const trigger = await screen.findByRole('button', { name: /All tabs/ });
    expect(trigger).toHaveAttribute('aria-controls', 'phone-tabs-sheet');

    await user.click(trigger);

    const dialog = await screen.findByRole('dialog', { name: 'All tabs' });
    expect(dialog).toHaveAttribute('id', 'phone-tabs-sheet');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(container.querySelector('.phone')).toHaveAttribute('inert');
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    expect(document.activeElement).toHaveClass('phone-sheet-row');

    fireEvent.keyDown(document.activeElement as HTMLElement, {
      key: 'Escape',
    });

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'All tabs' }),
      ).not.toBeInTheDocument(),
    );
    expect(trigger).toHaveFocus();
    expect(container.querySelector('.phone')).not.toHaveAttribute('inert');
  });

  it('keeps Tab inside the open sheet', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, waiting] }),
    );
    const user = userEvent.setup();

    renderPhone();
    await user.click(
      await screen.findByRole('button', { name: /All tabs/ }),
    );

    const dialog = await screen.findByRole('dialog', { name: 'All tabs' });
    const focusables = Array.from(
      dialog.querySelectorAll<HTMLElement>('button:not([disabled])'),
    );
    const first = focusables[0];
    const last = focusables[focusables.length - 1];

    last.focus();
    fireEvent.keyDown(last, { key: 'Tab' });
    expect(first).toHaveFocus();

    first.focus();
    fireEvent.keyDown(first, { key: 'Tab', shiftKey: true });
    expect(last).toHaveFocus();
  });

  it('shows a failed config switch inside the open sheet', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({
        sessions: [{ ...working, allowedConfigIds: ['c1', 'c2'] }],
        configs: [
          config,
          { ...config, id: 'c2', name: 'Second config' },
        ],
      }),
    );
    vi.mocked(switchSessionConfig).mockRejectedValue(new Error('refused'));
    const user = userEvent.setup();

    const { container } = renderPhone();
    await user.click(
      await screen.findByRole('button', { name: 'Session actions' }),
    );
    await user.click(
      within(
        await screen.findByRole('dialog', { name: 'Session actions' }),
      ).getByRole('button', { name: 'Switch AI config…' }),
    );

    const sheet = await screen.findByRole('dialog', {
      name: 'Switch AI config',
    });
    await user.click(
      within(sheet).getByRole('button', { name: /Second config/ }),
    );

    expect(
      await within(sheet).findByRole('alert'),
    ).toHaveTextContent('The AI config was not changed. Try again.');
    expect(sheet).toBeInTheDocument();
    expect(container.querySelector('.phone')).toHaveAttribute('inert');
  });

  it('makes the usage dialog modal and returns focus to the actions trigger', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    const user = userEvent.setup();

    const { container } = renderPhone();
    const actions = await screen.findByRole('button', {
      name: 'Session actions',
    });
    await user.click(actions);
    const actionsSheet = await screen.findByRole('dialog', {
      name: 'Session actions',
    });
    await user.click(
      within(actionsSheet).getByRole('button', { name: 'Usage' }),
    );

    const usage = await screen.findByRole('dialog', { name: 'Usage' });
    expect(usage).toHaveAttribute('aria-modal', 'true');
    expect(container.querySelector('.phone')).toHaveAttribute('inert');
    expect(container.querySelector('.phone')?.contains(usage)).toBe(false);

    await user.click(within(usage).getByRole('button', { name: 'Close' }));

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Usage' }),
      ).not.toBeInTheDocument(),
    );
    expect(actions).toHaveFocus();
    expect(container.querySelector('.phone')).not.toHaveAttribute('inert');
  });

  it('returns focus to the actions trigger when the close dialog is cancelled', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    const user = userEvent.setup();

    const { container } = renderPhone();
    const actions = await screen.findByRole('button', {
      name: 'Session actions',
    });
    await user.click(actions);
    await user.click(
      within(
        await screen.findByRole('dialog', { name: 'Session actions' }),
      ).getByRole('button', { name: 'Close…' }),
    );

    const close = await screen.findByRole('dialog', {
      name: 'Close session?',
    });
    expect(container.querySelector('.phone')).toHaveAttribute('inert');
    await user.click(within(close).getByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Close session?' }),
      ).not.toBeInTheDocument(),
    );
    expect(actions).toHaveFocus();
    expect(container.querySelector('.phone')).not.toHaveAttribute('inert');
  });

  it('leaves the sheet and shows the list when the active session goes away', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    const user = userEvent.setup();

    const { container } = renderPhone();
    await user.click(
      await screen.findByRole('button', { name: 'Session actions' }),
    );
    expect(
      await screen.findByRole('dialog', { name: 'Session actions' }),
    ).toBeInTheDocument();

    act(() => {
      handlers.session?.({
        ...working,
        status: 'closed',
        workState: null,
        waitingFor: null,
      });
    });

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Session actions' }),
      ).not.toBeInTheDocument(),
    );
    expect(container.querySelector('.phone')).not.toHaveAttribute('inert');
    expect(
      screen.getByRole('heading', { level: 1, name: 'Sessions' }),
    ).toBeInTheDocument();
  });

  it('focuses the first invalid launch field and describes it', async () => {
    vi.mocked(bootstrap).mockResolvedValue(response());
    const user = userEvent.setup();

    renderPhone();
    await user.click(
      await screen.findByRole('button', { name: 'New session' }),
    );

    const dialog = await screen.findByRole('dialog', { name: 'New session' });
    const project = screen.getByRole('combobox', { name: 'Project' });
    expect(project).toHaveFocus();

    const submit = within(dialog).getByRole('button', {
      name: 'Start session',
    });
    expect(submit).not.toBeDisabled();
    expect(submit).toHaveAttribute('aria-disabled', 'true');

    await user.click(submit);

    await waitFor(() => expect(project).toHaveFocus());
    expect(project).toHaveAttribute('aria-invalid', 'true');
    const describedBy = project.getAttribute('aria-describedby');
    expect(describedBy).toBeTruthy();
    expect(document.getElementById(describedBy as string)).toHaveTextContent(
      'Choose a project and an AI config first.',
    );
    expect(
      screen.getByRole('combobox', { name: 'AI config' }),
    ).not.toHaveAttribute('aria-invalid');
  });

  it('lets Escape close a combobox before the sheet', async () => {
    vi.mocked(bootstrap).mockResolvedValue(response());
    const user = userEvent.setup();

    renderPhone();
    await user.click(
      await screen.findByRole('button', { name: 'New session' }),
    );

    const dialog = await screen.findByRole('dialog', { name: 'New session' });
    const project = screen.getByRole('combobox', { name: 'Project' });
    await user.click(project);
    expect(project).toHaveAttribute('aria-expanded', 'true');

    fireEvent.keyDown(project, { key: 'Escape' });
    expect(dialog).toBeInTheDocument();
    expect(project).toHaveAttribute('aria-expanded', 'false');

    fireEvent.keyDown(project, { key: 'Escape' });
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'New session' }),
      ).not.toBeInTheDocument(),
    );
  });
});

describe('phone card announcements', () => {
  function liveRegion(): HTMLElement {
    const region = document.querySelector<HTMLElement>('.phone-live');
    expect(region).not.toBeNull();
    return region as HTMLElement;
  }

  it('announces a new permission card once', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({
        sessions: [
          {
            ...working,
            pendingPermission: {
              requestId: 'r1',
              tool: 'Bash',
              summary: 'npm test',
            },
          },
        ],
      }),
    );
    const user = userEvent.setup();

    renderPhone();
    await screen.findByText('Permission needed');

    const region = liveRegion();
    await waitFor(() =>
      expect(region).toHaveTextContent('Permission needed: Bash'),
    );

    const mutations: MutationRecord[] = [];
    const observer = new MutationObserver((records) => {
      mutations.push(...records);
    });
    observer.observe(region, {
      childList: true,
      characterData: true,
      subtree: true,
    });

    await user.type(
      screen.getByRole('textbox', { name: 'Prompt' }),
      'a later prompt',
    );
    await waitFor(() =>
      expect(screen.getByRole('textbox', { name: 'Prompt' })).toHaveValue(
        'a later prompt',
      ),
    );
    expect(mutations).toHaveLength(0);
    observer.disconnect();
  });

  it('announces a new question card', async () => {
    window.history.replaceState(null, '', '#session=w');
    const question: Question = {
      id: 'q1',
      sessionId: 'w',
      sessionName: 'Working session',
      text: 'Which branch?',
      options: [],
      status: 'pending',
      answer: null,
      createdAt: '2026-11-19T10:00:00Z',
    };
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working], questions: [question] }),
    );

    renderPhone();
    await screen.findByText('Which branch?');

    await waitFor(() =>
      expect(liveRegion()).toHaveTextContent('Question'),
    );
  });

  it('announces the same card wording again for a new card', async () => {
    window.history.replaceState(null, '', '#session=w');
    const first: Question = {
      id: 'q1',
      sessionId: 'w',
      sessionName: 'Working session',
      text: 'Which branch?',
      options: [],
      status: 'pending',
      answer: null,
      createdAt: '2026-11-19T10:00:00Z',
    };
    const second: Question = {
      ...first,
      id: 'q2',
      text: 'Which folder?',
    };
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working], questions: [first] }),
    );

    renderPhone();
    await screen.findByText('Which branch?');
    const region = liveRegion();
    await waitFor(() => expect(region).toHaveTextContent('Question'));

    const mutations: MutationRecord[] = [];
    const observer = new MutationObserver((records) => {
      mutations.push(...records);
    });
    observer.observe(region, {
      childList: true,
      characterData: true,
      subtree: true,
    });

    act(() => {
      handlers.aux?.({
        type: 'question.updated',
        question: { ...first, status: 'answered' },
      });
      handlers.aux?.({ type: 'question.updated', question: second });
    });

    await screen.findByText('Which folder?');
    await waitFor(() => expect(region).toHaveTextContent('Question'));

    expect(mutations.length).toBeGreaterThan(0);
    observer.disconnect();
  });

  it('announces a new session request card', async () => {
    window.history.replaceState(null, '', '#session=w');
    const request: SessionRequest = {
      id: 'req1',
      parentSessionId: 'w',
      parentSessionName: 'Working session',
      projectId: 'p1',
      projectName: 'Web',
      name: null,
      folder: '/dev/web',
      sourceBranch: null,
      firstPrompt: null,
      configId: 'c1',
      providerId: 'max',
      client: 'Claude Code',
      status: 'pending',
      childSessionId: null,
      createdAt: '2026-11-19T10:00:00Z',
    };
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working], sessionRequests: [request] }),
    );

    renderPhone();
    await screen.findByText(/wants to start a session/);

    await waitFor(() =>
      expect(liveRegion()).toHaveTextContent('Session requests'),
    );
  });
});

describe('phone headings and current tab', () => {
  it('uses a real H1 for the screen title', async () => {
    vi.mocked(bootstrap).mockResolvedValue(response());
    renderPhone();

    expect(
      await screen.findByRole('heading', {
        level: 1,
        name: 'Sessions',
      }),
    ).toBeInTheDocument();
  });

  it('uses a real H1 for the session title', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );
    renderPhone();

    expect(
      await screen.findByRole('heading', {
        level: 1,
        name: 'Working session',
      }),
    ).toBeInTheDocument();
  });

  it('marks the active view with aria-current', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, waiting] }),
    );
    renderPhone();

    const activeTab = (await screen.findByRole('button', {
      name: /Working session/,
    })) as HTMLElement;
    expect(activeTab).toHaveClass('phone-tab');
    expect(activeTab).toHaveAttribute('aria-current', 'true');

    const otherTab = screen.getByRole('button', {
      name: /Waiting session/,
    });
    expect(otherTab).not.toHaveAttribute('aria-current');
  });

  it('marks a waiting active session on the list row', async () => {
    window.history.replaceState(null, '', '#session=wait');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [waiting] }),
    );
    const user = userEvent.setup();
    renderPhone();

    await user.click(
      await screen.findByRole('button', { name: 'Back to sessions' }),
    );
    const row = (await screen.findByText('Waiting session')).closest(
      '.phone-row',
    ) as HTMLElement;
    expect(row).toHaveClass('is-on');
    expect(row).toHaveAttribute('aria-current', 'true');
  });
});

describe('phone session controls', () => {
  it('keeps secondary actions in one overflow sheet', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({
        sessions: [{ ...working, allowedConfigIds: ['c1', 'c2'] }],
      }),
    );
    const user = userEvent.setup();

    renderPhone();
    expect(
      screen.queryByRole('group', { name: 'Session actions' }),
    ).not.toBeInTheDocument();

    await user.click(
      await screen.findByRole('button', { name: 'Session actions' }),
    );

    const sheet = await screen.findByRole('dialog', {
      name: 'Session actions',
    });
    expect(sheet).toHaveTextContent('Switch AI config…');
    expect(sheet).toHaveTextContent('Usage');
    expect(sheet).toHaveTextContent('Close…');
  });

  it('focuses the new session title after a session starts', async () => {
    vi.mocked(bootstrap).mockResolvedValue(response({ sessions: [] }));
    vi.mocked(createSession).mockResolvedValue(
      makeSession({ id: 'new', name: 'Started session' }),
    );
    const user = userEvent.setup();

    renderPhone();
    await user.click(
      await screen.findByRole('button', { name: 'New session' }),
    );

    const start = await screen.findByRole('dialog', {
      name: 'New session',
    });
    await user.click(
      within(start).getByRole('combobox', { name: 'Project' }),
    );
    await user.click(screen.getByRole('option', { name: /core/ }));
    await user.click(
      within(start).getByRole('combobox', { name: 'AI config' }),
    );
    await user.click(screen.getByRole('option', { name: /Claude Max/ }));

    await user.click(
      within(start).getByRole('button', { name: 'Start session' }),
    );

    const title = await screen.findByRole('heading', {
      level: 1,
      name: 'Started session',
    });
    await waitFor(() => expect(title).toHaveFocus());
  });

  it('keeps the key rail and its add action reachable', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working] }),
    );

    renderPhone();

    const rail = await screen.findByRole('toolbar', {
      name: 'Keyboard keys',
    });
    expect(rail).toHaveClass('phone-key-rail');
    expect(rail.querySelector('.key-chip.add-key')).toBeInTheDocument();
  });
});
