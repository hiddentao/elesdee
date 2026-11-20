import { render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Project } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { makeSession } from '../test/fixtures';
import { BranchActionsMenu } from './BranchActionsMenu';
import { MoveSessionDialog, SessionActionsMenu, menuItems } from './SessionActionsMenu';

const session = makeSession({ name: 'Move me', projectId: 'p1' });

const copySession = makeSession({
  name: 'Copy check',
  folder: '/dev/web',
  branch: 'elesdee/core/copy-check',
});

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

function renderWithLanguage(node: ReactNode) {
  return render(<LanguageProvider>{node}</LanguageProvider>);
}

describe('session copy actions', () => {
  it('keeps copy folder and copy branch out of the session menu', () => {
    const labels = menuItems(copySession).map((item) => item.label);
    expect(labels).not.toContain('Copy folder path');
    expect(labels).not.toContain('Copy branch name');

    renderWithLanguage(
      <SessionActionsMenu
        session={copySession}
        point={{ x: 40, y: 40 }}
        placement="at-point"
        trigger={null}
        onAction={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole('menuitem', { name: 'Copy folder path' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('menuitem', { name: 'Copy branch name' }),
    ).not.toBeInTheDocument();
  });

  it('keeps copy folder and copy branch in the branch menu', () => {
    renderWithLanguage(
      <BranchActionsMenu
        session={copySession}
        trigger={document.createElement('button')}
        canOpen={false}
        onAction={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(
      screen.getByRole('menuitem', { name: 'Copy branch name' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('menuitem', { name: 'Copy folder path' }),
    ).toBeInTheDocument();
  });
});
