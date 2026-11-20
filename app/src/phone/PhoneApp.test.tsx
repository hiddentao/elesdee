import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { bootstrap } from '../api/client';
import type {
  BootstrapResponse,
  Question,
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

vi.mock('../api/events', () => ({
  subscribeEvents: vi.fn(() => () => undefined),
  subscribeAux: vi.fn(() => () => undefined),
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

    const form = dialog.querySelector('form');
    expect(form).not.toBeNull();
    fireEvent.submit(form as HTMLFormElement);

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

  it('marks the active tab with aria-current', async () => {
    window.history.replaceState(null, '', '#session=w');
    vi.mocked(bootstrap).mockResolvedValue(
      response({ sessions: [working, waiting] }),
    );
    renderPhone();

    const activeTab = (await screen.findByRole('button', {
      name: /Working session/,
    })) as HTMLElement;
    expect(activeTab).toHaveClass('phone-tab');
    expect(activeTab).toHaveAttribute('aria-current', 'page');

    const otherTab = screen.getByRole('button', {
      name: /Waiting session/,
    });
    expect(otherTab).not.toHaveAttribute('aria-current');
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
