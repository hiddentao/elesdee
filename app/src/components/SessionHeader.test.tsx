import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Account, Session } from '../api/types';
import { SessionHeader } from './SessionHeader';

const account: Account = {
  id: 'a1',
  name: 'Work',
  plan: 'Max',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  sample: false,
  rows: [{ label: 'Weekly', used: 30 }],
  foot: '',
  server: null,
  model: null,
};

const session: Session = {
  id: 's1',
  name: 'Header session',
  short: 'header',
  projectId: 'p1',
  kind: 'agent',
  status: 'active',
  workState: 'working',
  client: 'Claude Code',
  accountId: 'a1',
  branch: 'elesdee/web/header',
  folder: '/dev/web',
  preview: 'Working.',
  waitingFor: null,
  pendingPermission: null,
  contextLeft: 47,
  startedAt: '2026-01-01T00:00:00Z',
  shortcuts: [],
};

function renderHeader(sessionOverrides: Partial<Session> = {}) {
  const onOpenSwitch = vi.fn();
  render(
    <SessionHeader
      session={{ ...session, ...sessionOverrides }}
      accounts={[account]}
      actionsOpen={false}
      onOpenMenu={vi.fn()}
      branchOpen={false}
      onOpenBranchMenu={vi.fn()}
      panelOpen={false}
      onTogglePanel={vi.fn()}
      onOpenSwitch={onOpenSwitch}
    />,
  );
  return { onOpenSwitch };
}

describe('SessionHeader', () => {
  it('shows the context left for the session in the usage pill', async () => {
    const user = userEvent.setup();
    renderHeader();

    const pill = screen.getByRole('button', { name: /Claude Code/ });
    await user.hover(pill);

    const tip = screen.getByRole('tooltip');
    expect(tip).toHaveTextContent('Context left');
    expect(tip).toHaveTextContent('47% left');
    expect(tip).toHaveTextContent('70% left');
  });

  it('says when the context left is unknown', async () => {
    const user = userEvent.setup();
    renderHeader({ contextLeft: null });

    await user.hover(screen.getByRole('button', { name: /Claude Code/ }));

    expect(screen.getByRole('tooltip')).toHaveTextContent(
      'Context left unknown',
    );
  });

  it('opens the switch chooser from the usage pill', async () => {
    const user = userEvent.setup();
    const { onOpenSwitch } = renderHeader();

    await user.click(screen.getByRole('button', { name: /Claude Code/ }));

    expect(onOpenSwitch).toHaveBeenCalledTimes(1);
  });
});
