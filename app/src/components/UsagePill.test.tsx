import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { Account } from '../api/types';
import { UsagePill } from './UsagePill';

const account: Account = {
  id: 'a1',
  name: 'Work',
  plan: 'Max',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  rows: [
    { label: '5-hour', used: 58 },
    { label: 'Weekly', used: 20 },
  ],
  foot: 'Resets in 2h.',
  server: null,
  model: null,
};

function renderPill(overrides: Partial<Account> = {}) {
  render(<UsagePill account={{ ...account, ...overrides }} />);
  return {
    trigger: screen.getByRole('button', { name: /Claude Code/ }),
  };
}

describe('UsagePill', () => {
  it('keeps the limits out of the accessibility tree while closed', () => {
    const { trigger } = renderPill();

    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(trigger).not.toHaveAttribute('aria-describedby');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('shows the full usage on hover and hides it again', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill();

    await user.hover(trigger);

    const tip = screen.getByRole('tooltip');
    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    expect(trigger).toHaveAttribute('aria-describedby', tip.id);
    expect(tip).toHaveTextContent('5-hour');
    expect(tip).toHaveTextContent('42% left');
    expect(tip).toHaveTextContent('80% left');
    expect(tip).toHaveTextContent('Resets in 2h.');

    await user.unhover(trigger);

    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });

  it('toggles the limits with a click', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill();

    await user.click(trigger);

    expect(trigger).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('tooltip')).toBeInTheDocument();

    await user.click(trigger);

    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('opens from the keyboard and closes on Escape', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill();

    await user.tab();
    expect(trigger).toHaveFocus();
    expect(trigger).toHaveAttribute('aria-expanded', 'false');

    await user.keyboard('{Enter}');
    expect(trigger).toHaveAttribute('aria-expanded', 'true');

    await user.keyboard('{Escape}');
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('closes when the click lands outside', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill();

    await user.click(trigger);
    expect(screen.getByRole('tooltip')).toBeInTheDocument();

    await user.click(document.body);

    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('shows no usage data when the account has no rows', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill({ rows: [], foot: '' });

    expect(trigger).toHaveTextContent('No usage data');

    await user.click(trigger);
    expect(screen.getByRole('tooltip')).toHaveTextContent('No usage data');
  });
});
