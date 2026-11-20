import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Project } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { makeSession } from '../test/fixtures';
import { MoveSessionDialog, SessionActionsMenu } from './SessionActionsMenu';

const session = makeSession({ name: 'Move me', projectId: 'p1' });

const projects: Project[] = [
  { id: 'p1', name: 'web', folder: '/dev/web' },
  { id: 'p2', name: 'mobile', folder: '/dev/mobile' },
];

describe('SessionActionsMenu move action', () => {
  it('offers a single-pointer move to another project', async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    const onClose = vi.fn();

    render(
      <LanguageProvider>
        <SessionActionsMenu
          session={session}
          point={{ x: 10, y: 10 }}
          placement="at-point"
          trigger={null}
          canMove
          onAction={onAction}
          onClose={onClose}
        />
      </LanguageProvider>,
    );

    await user.click(
      screen.getByRole('menuitem', { name: 'Move to project…' }),
    );

    expect(onAction).toHaveBeenCalledWith('move');
    expect(onClose).toHaveBeenCalled();
  });

  it('does not offer a move when there is no other project', () => {
    render(
      <LanguageProvider>
        <SessionActionsMenu
          session={session}
          point={{ x: 10, y: 10 }}
          placement="at-point"
          trigger={null}
          onAction={vi.fn()}
          onClose={vi.fn()}
        />
      </LanguageProvider>,
    );

    expect(
      screen.queryByRole('menuitem', { name: 'Move to project…' }),
    ).not.toBeInTheDocument();
  });
});

describe('MoveSessionDialog', () => {
  it('moves the session to the chosen project', async () => {
    const user = userEvent.setup();
    const onMove = vi.fn(async () => true);
    const onClose = vi.fn();

    render(
      <LanguageProvider>
        <MoveSessionDialog
          session={session}
          projects={projects}
          onMove={onMove}
          onClose={onClose}
        />
      </LanguageProvider>,
    );

    expect(
      screen.getByRole('heading', { name: 'Move Move me to a project' }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Move' }));

    expect(onMove).toHaveBeenCalledWith(session, 'p2');
    expect(onClose).toHaveBeenCalled();
  });
});
