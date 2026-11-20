import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getSwitchOptions, listAccounts } from '../api/client';
import type { Account, Session } from '../api/types';
import { SwitchAccountDialog } from './SwitchAccountDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return {
    ...actual,
    getSwitchOptions: vi.fn(),
    listAccounts: vi.fn(),
  };
});

const current: Account = {
  id: 'a1',
  name: 'Current',
  plan: 'Max',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  sample: false,
  rows: [{ label: 'Weekly', used: 10 }],
  foot: '',
  server: null,
  model: null,
};

const next: Account = {
  id: 'a2',
  name: 'Room account',
  plan: 'Pro',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  sample: false,
  rows: [{ label: 'Weekly', used: 20 }],
  foot: '',
  server: null,
  model: null,
};

const otherClient: Account = {
  id: 'a3',
  name: 'Codex account',
  plan: 'Pro',
  client: 'Codex',
  provider: 'OpenAI',
  auth: 'subscription',
  sample: false,
  rows: [{ label: 'Weekly', used: 40 }],
  foot: '',
  server: null,
  model: null,
};

const session: Session = {
  id: 's1',
  name: 'Switch session',
  short: 'switch',
  projectId: 'p1',
  kind: 'agent',
  status: 'active',
  workState: 'working',
  client: 'Claude Code',
  accountId: 'a1',
  branch: 'elesdee/web/switch',
  folder: '/dev/web',
  preview: 'Working.',
  waitingFor: null,
  pendingPermission: null,
  contextLeft: 62,
  startedAt: '2026-01-01T00:00:00Z',
  shortcuts: [],
};

function renderDialog(
  onSwitch = vi.fn().mockResolvedValue(undefined),
  onAllowedChange = vi.fn().mockResolvedValue(undefined),
) {
  render(
    <SwitchAccountDialog
      session={session}
      onSwitch={onSwitch}
      onAllowedChange={onAllowedChange}
      onClose={vi.fn()}
    />,
  );
  return { onSwitch, onAllowedChange };
}

beforeEach(() => {
  vi.mocked(getSwitchOptions).mockResolvedValue({
    accounts: [current, next, otherClient],
    allowedAccountIds: ['a1', 'a2', 'a3'],
    nextAccountId: 'a2',
  });
  vi.mocked(listAccounts).mockImplementation(async (query) => {
    const text = query.toLowerCase();
    return [current, next, otherClient].filter((account) =>
      account.name.toLowerCase().includes(text),
    );
  });
});

describe('SwitchAccountDialog', () => {
  it('lists the accounts with usage and marks the next automatic target', async () => {
    const user = userEvent.setup();
    renderDialog();

    const combo = await screen.findByRole('combobox', { name: 'Account' });
    await user.click(combo);

    const option = await screen.findByRole('option', { name: /Room account/ });
    expect(option).toHaveTextContent('80% left');
    expect(option).toHaveTextContent('Next auto-switch');
    expect(
      screen.queryByRole('option', { name: /Current/ }),
    ).not.toBeInTheDocument();
  });

  it('switches to the chosen account', async () => {
    const user = userEvent.setup();
    const { onSwitch } = renderDialog();

    const combo = await screen.findByRole('combobox', { name: 'Account' });
    await user.click(combo);
    await user.type(combo, 'Room');
    await user.click(
      await screen.findByRole('option', { name: /Room account/ }),
    );
    await user.click(screen.getByRole('button', { name: 'Switch account' }));

    await waitFor(() => expect(onSwitch).toHaveBeenCalledWith('a2'));
  });

  it('explains the handoff when the chosen account uses another client', async () => {
    const user = userEvent.setup();
    renderDialog();

    const combo = await screen.findByRole('combobox', { name: 'Account' });
    await user.click(combo);
    await user.type(combo, 'Codex');
    await user.click(
      await screen.findByRole('option', { name: /Codex account/ }),
    );

    expect(
      screen.getByText(/Switching to Codex writes a handoff note first/),
    ).toBeInTheDocument();
  });

  it('offers only allowed accounts and saves an allowed-list change', async () => {
    const user = userEvent.setup();
    vi.mocked(getSwitchOptions).mockResolvedValueOnce({
      accounts: [current, next, otherClient],
      allowedAccountIds: ['a1', 'a2'],
      nextAccountId: 'a2',
    });
    const { onAllowedChange } = renderDialog();

    const combo = await screen.findByRole('combobox', { name: 'Account' });
    await user.click(combo);
    await user.type(combo, 'Codex');
    expect(
      screen.queryByRole('option', { name: /Codex account/ }),
    ).not.toBeInTheDocument();
    await user.clear(combo);

    expect(
      screen.getByRole('button', { name: 'Remove Room account' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Remove Current' }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole('button', { name: 'Remove Room account' }),
    );

    await waitFor(() =>
      expect(onAllowedChange).toHaveBeenCalledWith(['a1']),
    );
    expect(
      screen.queryByRole('button', { name: 'Remove Room account' }),
    ).not.toBeInTheDocument();
  });

  it('shows a retry when the account list does not load', async () => {
    vi.mocked(getSwitchOptions).mockRejectedValueOnce(new Error('offline'));
    renderDialog();

    expect(
      await screen.findByRole('alert'),
    ).toHaveTextContent('The accounts were not loaded.');
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });
});
