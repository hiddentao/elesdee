import { useRef, useState } from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Session } from '../api/types';
import { makeSession } from '../test/fixtures';
import { BranchActionsMenu, type BranchMenuAction } from './BranchActionsMenu';

function Harness({
  onAction,
  canOpen = false,
}: {
  onAction: (action: BranchMenuAction) => void;
  canOpen?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const session: Session = makeSession({
    branch: 'elesdee/core/fix-login',
    folder: '/dev/core',
  });

  return (
    <>
      <button ref={triggerRef} type="button" onClick={() => setOpen(true)}>
        Branch
      </button>
      {open ? (
        <BranchActionsMenu
          session={session}
          trigger={triggerRef.current as HTMLButtonElement}
          canOpen={canOpen}
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

describe('BranchActionsMenu', () => {
  it('offers the branch and folder copies', async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<Harness onAction={onAction} />);

    await user.click(screen.getByRole('button', { name: 'Branch' }));

    expect(screen.getByRole('menuitem', { name: 'Copy branch name' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Copy folder path' })).toBeInTheDocument();

    await user.click(
      screen.getByRole('menuitem', { name: 'Copy branch name' }),
    );
    expect(onAction).toHaveBeenCalledWith('copy-branch');
  });

  it('copies the folder path', async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<Harness onAction={onAction} />);

    await user.click(screen.getByRole('button', { name: 'Branch' }));
    await user.click(
      screen.getByRole('menuitem', { name: 'Copy folder path' }),
    );

    expect(onAction).toHaveBeenCalledWith('copy-folder');
  });

  it('shows the open actions only when the shell can open them', async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<Harness onAction={onAction} canOpen />);

    await user.click(screen.getByRole('button', { name: 'Branch' }));

    expect(
      screen.getByRole('menuitem', { name: 'Open folder' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: 'Open in Terminal' }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('menuitem', { name: 'Open folder' }));
    expect(onAction).toHaveBeenCalledWith('open-folder');
  });

  it('hides the open actions outside the desktop shell', async () => {
    const user = userEvent.setup();
    render(<Harness onAction={vi.fn()} />);

    await user.click(screen.getByRole('button', { name: 'Branch' }));

    expect(
      screen.queryByRole('menuitem', { name: 'Open folder' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Open in Terminal' }),
    ).not.toBeInTheDocument();
  });

  it('closes on Escape and returns focus to the branch pill', async () => {
    const user = userEvent.setup();
    render(<Harness onAction={vi.fn()} />);

    const trigger = screen.getByRole('button', { name: 'Branch' });
    await user.click(trigger);
    await user.keyboard('{Escape}');

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it('closes when the scrim is clicked', async () => {
    const user = userEvent.setup();
    render(<Harness onAction={vi.fn()} />);

    await user.click(screen.getByRole('button', { name: 'Branch' }));
    fireEvent.click(document.querySelector('.menu-scrim') as HTMLElement);

    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
