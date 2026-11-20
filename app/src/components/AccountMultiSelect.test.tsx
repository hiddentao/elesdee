import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { listAccounts } from '../api/client';
import type { Account } from '../api/types';
import { AccountMultiSelect } from './AccountMultiSelect';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return {
    ...actual,
    listAccounts: vi.fn(),
  };
});

const work: Account = {
  id: 'a1',
  name: 'Work',
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

const personal: Account = {
  id: 'a2',
  name: 'Personal',
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

const codex: Account = {
  id: 'a3',
  name: 'Codex',
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

const accounts = [work, personal, codex];

beforeEach(() => {
  vi.mocked(listAccounts).mockImplementation(async (query) => {
    const text = query.toLowerCase();
    return accounts.filter((account) =>
      account.name.toLowerCase().includes(text),
    );
  });
});

describe('AccountMultiSelect', () => {
  it('shows the allowed accounts and keeps the current one fixed', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <AccountMultiSelect
        id="allowed"
        label="Allowed for this session"
        accounts={accounts}
        value={['a1', 'a2']}
        requiredId="a1"
        onChange={onChange}
      />,
    );

    expect(screen.getByText('Work')).toBeInTheDocument();
    expect(screen.getByText('Personal')).toBeInTheDocument();
    expect(screen.getByText('Current')).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Remove Work' }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Remove Personal' }));
    expect(onChange).toHaveBeenCalledWith(['a1']);
  });

  it('adds an account through the filterable list', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <AccountMultiSelect
        id="allowed"
        label="Allowed for this session"
        accounts={accounts}
        value={['a1']}
        requiredId="a1"
        onChange={onChange}
      />,
    );

    const combo = screen.getByRole('combobox', { name: 'Add an account' });
    await user.click(combo);
    await user.type(combo, 'Codex');
    await user.click(
      await screen.findByRole('option', { name: /Codex/ }),
    );

    await waitFor(() => expect(onChange).toHaveBeenCalledWith(['a1', 'a3']));
  });

  it('does not offer an account that is already allowed', async () => {
    const user = userEvent.setup();
    render(
      <AccountMultiSelect
        id="allowed"
        label="Allowed for this session"
        accounts={accounts}
        value={['a1']}
        requiredId="a1"
        onChange={vi.fn()}
      />,
    );

    const combo = screen.getByRole('combobox', { name: 'Add an account' });
    await user.click(combo);
    await user.type(combo, 'Work');

    expect(await screen.findByText('No matches.')).toBeInTheDocument();
    expect(
      screen.queryByRole('option', { name: /Work/ }),
    ).not.toBeInTheDocument();
  });
});
