import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Project } from '../api/types';
import { makeSession } from '../test/fixtures';
import { TabStrip } from './TabStrip';

const project: Project = { id: 'p1', name: 'web', folder: '/dev/web' };

const sessions = [
  makeSession({ id: 's1', name: 'First' }),
  makeSession({ id: 's2', name: 'Second' }),
  makeSession({ id: 's3', name: 'Third' }),
];

function renderStrip(
  activeSessionId: string | null = 's2',
  onSelectSession = vi.fn(),
  onSessionMenu = vi.fn(),
  onNewSession = vi.fn(),
) {
  render(
    <TabStrip
      project={project}
      sessions={sessions}
      activeSessionId={activeSessionId}
      onSelectSession={onSelectSession}
      onSessionMenu={onSessionMenu}
      onNewSession={onNewSession}
    />,
  );
  return onSelectSession;
}

describe('TabStrip', () => {
  it('uses a roving tab stop and a panel relationship without a state line', () => {
    renderStrip();
    const tabs = screen.getAllByRole('tab');

    expect(tabs[0]).toHaveAttribute('tabindex', '-1');
    expect(tabs[1]).toHaveAttribute('tabindex', '0');
    expect(tabs[0]).toHaveAttribute('aria-controls', 'session-panel');
    expect(tabs[1]).toHaveAttribute('aria-selected', 'true');
    expect(within(tabs[1]).getByText('Second')).toBeInTheDocument();
    expect(within(tabs[1]).queryByText('Working')).not.toBeInTheDocument();
  });

  it('gives each work state a different marker shape', () => {
    const states = [
      makeSession({ id: 's1', name: 'Running', workState: 'working' }),
      makeSession({ id: 's2', name: 'Waiting', workState: 'waiting' }),
      makeSession({ id: 's3', name: 'Input', workState: 'input' }),
      makeSession({ id: 's4', name: 'Paused', workState: 'paused' }),
      makeSession({ id: 's5', name: 'Done', workState: 'done' }),
    ];

    render(
      <TabStrip
        project={project}
        sessions={states}
        activeSessionId="s1"
        onSelectSession={vi.fn()}
        onSessionMenu={vi.fn()}
        onNewSession={vi.fn()}
      />,
    );

    const shapes = screen.getAllByRole('tab').map((tab) =>
      tab.querySelector('.status-marker')?.getAttribute('data-shape'),
    );

    expect(shapes).toEqual([
      'dot',
      'diamond',
      'square',
      'square-ring',
      'thick-ring',
    ]);
    expect(new Set(shapes).size).toBe(states.length);
  });

  it('opens a new session from the trailing plus', async () => {
    const user = userEvent.setup();
    const onNewSession = vi.fn();
    renderStrip('s2', vi.fn(), vi.fn(), onNewSession);

    await user.click(screen.getByRole('button', { name: 'New session' }));

    expect(onNewSession).toHaveBeenCalledTimes(1);
  });

  it('opens the session menu on right-click', () => {
    const onSessionMenu = vi.fn();
    renderStrip('s2', vi.fn(), onSessionMenu);

    fireEvent.contextMenu(screen.getByRole('tab', { name: /Second/ }));

    expect(onSessionMenu).toHaveBeenCalledWith(
      expect.objectContaining({ id: 's2' }),
      expect.objectContaining({ x: expect.any(Number), y: expect.any(Number) }),
      expect.anything(),
    );
  });

  it('moves selection with arrow, Home and End keys', async () => {
    const user = userEvent.setup();
    const onSelect = renderStrip();
    const second = screen.getByRole('tab', { name: /Second/ });
    second.focus();

    await user.keyboard('{ArrowRight}');
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 's3' }));
    expect(screen.getByRole('tab', { name: /Third/ })).toHaveFocus();

    await user.keyboard('{Home}');
    expect(onSelect).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 's1' }),
    );
    expect(screen.getByRole('tab', { name: /First/ })).toHaveFocus();

    await user.keyboard('{End}');
    expect(onSelect).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 's3' }),
    );
    expect(screen.getByRole('tab', { name: /Third/ })).toHaveFocus();

    await user.keyboard('{ArrowLeft}');
    expect(onSelect).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: 's2' }),
    );
    expect(screen.getByRole('tab', { name: /Second/ })).toHaveFocus();
  });
});
