import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Account } from '../api/types';
import { UsageDialog } from './UsageDialog';

function makeAccount(overrides: Partial<Account> = {}): Account {
  return {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'Anthropic',
    auth: 'subscription',
    sample: false,
    rows: [],
    foot: '',
    server: null,
    model: null,
    ...overrides,
  };
}

const accounts: Account[] = [
  makeAccount({
    sample: true,
    rows: [
      { label: '5-hour', used: 58 },
      { label: 'Weekly', used: 20 },
    ],
    foot: 'Resets in 2h.',
  }),
  makeAccount({
    id: 'a2',
    name: 'Wallet',
    plan: 'Pay as you go',
    client: 'OpenCode 2',
    provider: 'OpenRouter',
    auth: 'key',
    sample: false,
    rows: [{ label: 'Credit', value: '$12.40', min: '$5.00' }],
    foot: 'Billed monthly.',
  }),
  makeAccount({
    id: 'a3',
    name: 'Empty',
    plan: 'Free',
    client: 'Codex',
    provider: 'OpenAI',
    rows: [],
  }),
];

describe('UsageDialog', () => {
  it('lists every account with its limits, balances and foot', () => {
    render(<UsageDialog accounts={accounts} onClose={vi.fn()} />);

    expect(screen.getByRole('heading', { name: 'Usage' })).toBeInTheDocument();
    expect(screen.getByText('Work')).toBeInTheDocument();
    expect(
      within(screen.getByRole('region', { name: 'Work' })).getByText('Sample'),
    ).toBeInTheDocument();
    expect(screen.getByText('Max · Claude Code')).toBeInTheDocument();
    expect(screen.getByText('5-hour')).toBeInTheDocument();
    expect(screen.getByText('42% left')).toBeInTheDocument();
    expect(screen.getByText('80% left')).toBeInTheDocument();
    expect(screen.getByText('Resets in 2h.')).toBeInTheDocument();

    expect(screen.getByText('Wallet')).toBeInTheDocument();
    expect(screen.getByText('$12.40')).toBeInTheDocument();
    expect(screen.getByText('Billed monthly.')).toBeInTheDocument();

    expect(screen.getByText('Empty')).toBeInTheDocument();
    expect(screen.getByText('No usage data')).toBeInTheDocument();

    expect(screen.getByRole('button', { name: 'Close' })).toHaveFocus();
  });

  it('shows no usage data when there are no accounts', () => {
    render(<UsageDialog accounts={[]} onClose={vi.fn()} />);

    expect(screen.getByText('No usage data')).toBeInTheDocument();
  });
});
