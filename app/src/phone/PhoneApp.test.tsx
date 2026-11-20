import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { bootstrap } from '../api/client';
import type { BootstrapResponse } from '../api/types';
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
