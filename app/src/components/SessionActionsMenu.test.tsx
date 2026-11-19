import { useRef, useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Session } from '../api/types';
import { makeSession } from '../test/fixtures';
import {
  SessionActionsMenu,
  type SessionMenuAction,
} from './SessionActionsMenu';

function Harness({
  session,
  onAction = vi.fn(),
}: {
  session: Session;
  onAction?: (action: SessionMenuAction) => void;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);

  return (
    <>
      <button ref={triggerRef} type="button" onClick={() => setOpen(true)}>
        Trigger
      </button>
      {open ? (
        <SessionActionsMenu
          session={session}
          point={{ x: 100, y: 100 }}
          placement="at-point"
          trigger={triggerRef.current}
          onAction={(action) => {
            onAction(action);
            setOpen(false);
          }}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}

async function openMenu(
  user: ReturnType<typeof userEvent.setup>,
  session: Session,
  onAction?: (action: SessionMenuAction) => void,
) {
  render(<Harness session={session} onAction={onAction} />);
  await user.click(screen.getByRole('button', { name: 'Trigger' }));
}

describe('SessionActionsMenu', () => {
  it('shows the active session actions', async () => {
    const user = userEvent.setup();
    await openMenu(user, makeSession({ branch: null }));

    expect(screen.getByRole('menu')).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Rename…' })).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: 'Copy folder path' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Copy branch name' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Close…' })).toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Reopen' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Delete…' }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('separator')).toBeInTheDocument();
  });

  it('shows the closed session actions and the branch copy', async () => {
    const user = userEvent.setup();
    await openMenu(
      user,
      makeSession({
        status: 'closed',
        branch: 'elesdee/core/old',
      }),
    );

    const items = screen
      .getAllByRole('menuitem')
      .map((item) => item.textContent);
    expect(items).toEqual([
      'Reopen',
      'Rename…',
      'Copy folder path',
      'Copy branch name',
      'Delete…',
    ]);
    expect(
      screen.queryByRole('menuitem', { name: 'Close…' }),
    ).not.toBeInTheDocument();
  });

  it('shows the session header, action icons and a menu close control', async () => {
    const user = userEvent.setup();
    await openMenu(user, makeSession());

    expect(screen.getByText('Session')).toBeInTheDocument();
    expect(screen.getByText('Active, Working')).toBeInTheDocument();
    expect(
      screen
        .getByRole('menuitem', { name: 'Rename…' })
        .querySelector('svg'),
    ).not.toBeNull();

    await user.click(screen.getByRole('button', { name: 'Close menu' }));

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Trigger' })).toHaveFocus();
  });

  it('hides copy actions when there is no folder or branch', async () => {
    const user = userEvent.setup();
    await openMenu(user, makeSession({ folder: '', branch: null }));

    expect(
      screen.queryByRole('menuitem', { name: 'Copy folder path' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Copy branch name' }),
    ).not.toBeInTheDocument();
  });

  it('runs an action, closes the menu and returns focus to the trigger', async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    await openMenu(user, makeSession(), onAction);

    await user.click(screen.getByRole('menuitem', { name: 'Rename…' }));

    expect(onAction).toHaveBeenCalledWith('rename');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Trigger' })).toHaveFocus();
  });

  it('supports arrow, Home, End and Escape keys', async () => {
    const user = userEvent.setup();
    await openMenu(user, makeSession());

    expect(screen.getByRole('menuitem', { name: 'Rename…' })).toHaveFocus();

    await user.keyboard('{ArrowDown}');
    expect(
      screen.getByRole('menuitem', { name: 'Copy folder path' }),
    ).toHaveFocus();

    await user.keyboard('{End}');
    expect(screen.getByRole('menuitem', { name: 'Close…' })).toHaveFocus();

    await user.keyboard('{Home}');
    expect(screen.getByRole('menuitem', { name: 'Rename…' })).toHaveFocus();

    await user.keyboard('{ArrowUp}');
    expect(screen.getByRole('menuitem', { name: 'Close…' })).toHaveFocus();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Trigger' })).toHaveFocus();
  });
});
