import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Account, Session } from '../api/types';
import { UsagePill } from './UsagePill';

const account: Account = {
  id: 'a1',
  name: 'Work',
  plan: 'Max',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  sample: true,
  rows: [
    { label: '5-hour', used: 58 },
    { label: 'Weekly', used: 20 },
  ],
  foot: 'Resets in 2h.',
  server: null,
  model: null,
};

const session: Session = {
  id: 's1',
  name: 'Session',
  short: 'session',
  projectId: 'p1',
  kind: 'agent',
  status: 'active',
  workState: 'working',
  client: 'Claude Code',
  accountId: 'a1',
  branch: 'elesdee/web/session',
  folder: '/dev/web',
  preview: 'Working.',
  waitingFor: null,
  pendingPermission: null,
  contextLeft: 62,
  startedAt: '2026-01-01T00:00:00Z',
  shortcuts: [],
};

function renderPill({
  overrides = {},
  sessionOverrides = {},
  canSwitch = true,
}: {
  overrides?: Partial<Account>;
  sessionOverrides?: Partial<Session>;
  canSwitch?: boolean;
} = {}) {
  const onOpenSwitch = vi.fn();
  render(
    <UsagePill
      session={{ ...session, ...sessionOverrides }}
      account={{ ...account, ...overrides }}
      canSwitch={canSwitch}
      onOpenSwitch={onOpenSwitch}
    />,
  );
  return {
    trigger: screen.getByRole('button', { name: /Claude Code/ }),
    onOpenSwitch,
  };
}

describe('UsagePill', () => {
  it('keeps the limits out of the accessibility tree while closed', () => {
    const { trigger } = renderPill();

    expect(trigger).toHaveAttribute('aria-haspopup', 'dialog');
    expect(trigger).not.toHaveAttribute('aria-describedby');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('shows the full usage and the context left on hover', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill();

    await user.hover(trigger);

    const tip = screen.getByRole('tooltip');
    expect(trigger).toHaveAttribute('aria-describedby', tip.id);
    expect(tip).toHaveTextContent('5-hour');
    expect(tip).toHaveTextContent('42% left');
    expect(tip).toHaveTextContent('80% left');
    expect(tip).toHaveTextContent('Resets in 2h.');
    expect(tip).toHaveTextContent('Context left');
    expect(tip).toHaveTextContent('62% left');
    expect(tip).toHaveTextContent('Compacts automatically.');

    await user.unhover(trigger);

    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('says so when the context left is unknown', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill({ sessionOverrides: { contextLeft: null } });

    await user.hover(trigger);

    expect(screen.getByRole('tooltip')).toHaveTextContent(
      'Context left unknown',
    );
  });

  it('opens the switch chooser on a click', async () => {
    const user = userEvent.setup();
    const { trigger, onOpenSwitch } = renderPill();

    await user.click(trigger);

    expect(onOpenSwitch).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('opens the switch chooser from the keyboard', async () => {
    const user = userEvent.setup();
    const { trigger, onOpenSwitch } = renderPill();

    await user.tab();
    expect(trigger).toHaveFocus();
    expect(screen.getByRole('tooltip')).toHaveTextContent('62% left');

    await user.keyboard('{Enter}');

    expect(onOpenSwitch).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('pins the limits for a closed session and closes on Escape', async () => {
    const user = userEvent.setup();
    const { trigger, onOpenSwitch } = renderPill({ canSwitch: false });

    expect(trigger).not.toHaveAttribute('aria-haspopup');

    await user.click(trigger);
    expect(screen.getByRole('tooltip')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
    expect(onOpenSwitch).not.toHaveBeenCalled();
  });

  it('closes when the click lands outside', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill({ canSwitch: false });

    await user.click(trigger);
    expect(screen.getByRole('tooltip')).toBeInTheDocument();

    await user.click(document.body);

    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  });

  it('shows no usage data when the account has no rows', async () => {
    const user = userEvent.setup();
    const { trigger } = renderPill({ overrides: { rows: [], foot: '' } });

    expect(trigger).toHaveTextContent('No usage data');

    await user.hover(trigger);
    expect(screen.getByRole('tooltip')).toHaveTextContent('No usage data');
  });
});
