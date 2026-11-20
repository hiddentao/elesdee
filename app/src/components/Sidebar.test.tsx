import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Project } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { makeSession } from '../test/fixtures';
import { Sidebar } from './Sidebar';

const projects: Project[] = [
  { id: 'p1', name: 'web', folder: '/dev/web' },
  { id: 'p2', name: 'mobile', folder: '/dev/mobile' },
];

const session = makeSession({
  id: 's1',
  name: 'Move me',
  projectId: 'p1',
  configId: null,
  providerId: null,
});

const originalElementFromPoint = document.elementFromPoint;

afterEach(() => {
  document.elementFromPoint = originalElementFromPoint;
});

function renderSidebar(sessions = [session]) {
  const onMoveSession = vi.fn(async () => true);
  const onSelectSession = vi.fn();
  render(
    <LanguageProvider>
      <Sidebar
        projects={projects}
        sessions={sessions}
        providers={[]}
        activeProjectId={null}
        activeSessionId={null}
        onSelectProject={vi.fn()}
        onSelectSession={onSelectSession}
        onSessionMenu={vi.fn()}
        onMoveSession={onMoveSession}
        onAddProject={vi.fn()}
        onNewSession={vi.fn()}
        onProjectSettings={vi.fn()}
        onOpenSettings={vi.fn()}
      />
    </LanguageProvider>,
  );
  return { onMoveSession, onSelectSession };
}

function sessionRow(): HTMLElement {
  return screen.getByText('Move me').closest('button') as HTMLElement;
}

function projectGroup(id: string): HTMLElement {
  return document.querySelector(`[data-project-id="${id}"]`) as HTMLElement;
}

describe('Sidebar session row', () => {
  it('keeps the full name and status in the row tooltip', () => {
    renderSidebar();

    expect(sessionRow()).toHaveAttribute(
      'title',
      'Move me, Active, Working',
    );
  });

  it('gives a paused session a marker shape that does not rely on colour', () => {
    renderSidebar([{ ...session, workState: 'paused' }]);

    expect(
      sessionRow().querySelector('.status-marker'),
    ).toHaveAttribute('data-shape', 'square-ring');
  });
});

describe('Sidebar session move', () => {
  it('drags a session onto another project and marks the drop target', async () => {
    const { onMoveSession } = renderSidebar();
    const row = sessionRow();
    const target = projectGroup('p2');
    document.elementFromPoint = vi.fn(() => target);

    fireEvent.pointerDown(row, {
      button: 0,
      pointerId: 1,
      clientX: 10,
      clientY: 10,
    });
    fireEvent.pointerMove(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 24,
    });

    expect(target).toHaveClass('is-drop-target');

    fireEvent.pointerUp(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 24,
    });

    await waitFor(() =>
      expect(onMoveSession).toHaveBeenCalledWith(session, 'p2'),
    );
  });

  it('ignores a pointer movement below the threshold', () => {
    const { onMoveSession, onSelectSession } = renderSidebar();
    const row = sessionRow();
    document.elementFromPoint = vi.fn(() => projectGroup('p2'));

    fireEvent.pointerDown(row, {
      button: 0,
      pointerId: 1,
      clientX: 10,
      clientY: 10,
    });
    fireEvent.pointerMove(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 12,
    });
    fireEvent.pointerUp(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 12,
    });
    fireEvent.click(row);

    expect(onMoveSession).not.toHaveBeenCalled();
    expect(onSelectSession).toHaveBeenCalledTimes(1);
  });

  it('cancels a drag when Escape is pressed', () => {
    const { onMoveSession } = renderSidebar();
    const row = sessionRow();
    document.elementFromPoint = vi.fn(() => projectGroup('p2'));

    fireEvent.pointerDown(row, {
      button: 0,
      pointerId: 1,
      clientX: 10,
      clientY: 10,
    });
    fireEvent.pointerMove(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 24,
    });
    expect(projectGroup('p2')).toHaveClass('is-drop-target');

    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.pointerUp(row, {
      pointerId: 1,
      clientX: 12,
      clientY: 24,
    });

    expect(onMoveSession).not.toHaveBeenCalled();
    expect(projectGroup('p2')).not.toHaveClass('is-drop-target');
    expect(screen.getByRole('status')).toHaveTextContent('Move cancelled.');
  });

  it('moves a session with the keyboard and announces the steps', async () => {
    const { onMoveSession } = renderSidebar();
    const row = sessionRow();
    row.focus();

    fireEvent.keyDown(row, { key: 'm' });
    expect(screen.getByRole('status')).toHaveTextContent(
      'Moving Move me. Target project: mobile.',
    );
    expect(projectGroup('p2')).toHaveClass('is-drop-target');

    fireEvent.keyDown(row, { key: 'ArrowUp' });
    expect(screen.getByRole('status')).toHaveTextContent(
      'Target project: mobile.',
    );

    fireEvent.keyDown(row, { key: 'Enter' });

    await waitFor(() =>
      expect(onMoveSession).toHaveBeenCalledWith(session, 'p2'),
    );
    await waitFor(() =>
      expect(screen.getByRole('status')).toHaveTextContent(
        'Move me moved to mobile.',
      ),
    );
  });

  it('cancels a keyboard move with Escape', () => {
    const { onMoveSession } = renderSidebar();
    const row = sessionRow();
    row.focus();

    fireEvent.keyDown(row, { key: 'm' });
    fireEvent.keyDown(row, { key: 'Escape' });

    expect(onMoveSession).not.toHaveBeenCalled();
    expect(projectGroup('p2')).not.toHaveClass('is-drop-target');
    expect(screen.getByRole('status')).toHaveTextContent('Move cancelled.');
  });
});
