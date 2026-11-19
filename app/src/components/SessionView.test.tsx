import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Session } from '../api/types';
import { makeSession } from '../test/fixtures';
import { SessionView } from './SessionView';

vi.mock('./TerminalPane', () => ({
  TerminalPane: () => <div>Terminal pane</div>,
}));

const permission = {
  requestId: 'req-1',
  tool: 'Bash',
  summary: 'pnpm test --filter web',
};

function renderView(session: Session, onReopen = vi.fn()) {
  return render(
    <SessionView
      session={session}
      onRespondPermission={vi.fn()}
      onReopen={onReopen}
    />,
  );
}

describe('SessionView permission card', () => {
  it('docks the card below the terminal for an active agent', async () => {
    renderView(
      makeSession({
        waitingFor: 'permission',
        pendingPermission: permission,
      }),
    );

    const title = screen.getByRole('heading', {
      name: 'Permission needed',
    });
    const terminal = await screen.findByText('Terminal pane');

    expect(
      terminal.compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.queryByText('Waiting for you')).not.toBeInTheDocument();
  });

  it('shows no card when there is no pending request', () => {
    renderView(makeSession());

    expect(
      screen.queryByRole('heading', { name: 'Permission needed' }),
    ).not.toBeInTheDocument();
  });

  it('shows no card for a closed agent session', () => {
    renderView(
      makeSession({
        status: 'closed',
        workState: null,
        pendingPermission: permission,
      }),
    );

    expect(
      screen.queryByRole('heading', { name: 'Permission needed' }),
    ).not.toBeInTheDocument();
  });

  it('shows no card for a terminal session', () => {
    renderView(
      makeSession({
        kind: 'terminal',
        client: null,
        accountId: null,
        pendingPermission: permission,
      }),
    );

    expect(
      screen.queryByRole('heading', { name: 'Permission needed' }),
    ).not.toBeInTheDocument();
  });

  it('keeps the waiting note when the permission data is missing', () => {
    renderView(makeSession({ waitingFor: 'permission' }));

    expect(screen.getByText('Waiting for you')).toBeInTheDocument();
  });

  it('shows a closed banner with the branch and a visible Reopen button', async () => {
    const user = userEvent.setup();
    const onReopen = vi.fn();
    renderView(
      makeSession({
        status: 'closed',
        workState: null,
        branch: 'elesdee/core/fix-login',
      }),
      onReopen,
    );

    expect(screen.getByText('Closed')).toBeInTheDocument();
    expect(
      screen.getByText(
        'The session log is kept. The branch elesdee/core/fix-login stays.',
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Reopen' }));

    expect(onReopen).toHaveBeenCalledTimes(1);
  });
});
