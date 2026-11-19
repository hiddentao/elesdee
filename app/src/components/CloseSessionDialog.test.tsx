import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { makeSession as baseSession } from '../test/fixtures';
import { CloseSessionDialog } from './CloseSessionDialog';

function makeSession(overrides: Partial<Session> = {}) {
  return baseSession({
    name: 'Check close',
    short: 'check-close',
    kind: 'terminal',
    client: null,
    accountId: null,
    ...overrides,
  });
}

describe('CloseSessionDialog', () => {
  it('shows the terminal copy and cancels without closing', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const onClose = vi.fn();
    render(
      <CloseSessionDialog
        session={makeSession()}
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );

    expect(
      screen.getByText('The terminal stops. The folder stays as it is.'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Close session' }),
    ).toHaveClass('btn-primary');
    expect(
      screen.getByRole('button', { name: 'Close session' }),
    ).not.toHaveClass('btn-danger');
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();

    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('describes the commit, push and worktree for an agent session', () => {
    render(
      <CloseSessionDialog
        session={makeSession({
          kind: 'agent',
          client: 'Claude Code',
          accountId: 'a1',
          branch: 'elesdee/web/fix-login',
        })}
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(
      screen.getByText(
        "Closing commits the agent's work and, when the repository has a remote, pushes the branch. The worktree is removed when that succeeds. If it fails, the worktree stays and the reason appears in the session preview. The branch stays.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/elesdee\/web\/fix-login/),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(
        'The session stops. Files in the folder stay as they are.',
      ),
    ).not.toBeInTheDocument();
  });

  it('keeps the dialog open and shows the server error', async () => {
    const user = userEvent.setup();
    const onConfirm = vi
      .fn()
      .mockRejectedValue(new ApiError(409, 'The session is already closed.'));
    const onClose = vi.fn();
    render(
      <CloseSessionDialog
        session={makeSession()}
        onConfirm={onConfirm}
        onClose={onClose}
      />,
    );

    await user.click(screen.getByRole('button', { name: 'Close session' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The session is already closed.',
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
