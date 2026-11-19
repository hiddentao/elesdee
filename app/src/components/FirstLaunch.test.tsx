import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { AgentCheck } from '../api/types';
import { FirstLaunch } from './FirstLaunch';

const agents: AgentCheck[] = [
  { name: 'Claude Code', installed: true, version: '2.1.328 (Claude Code)' },
  { name: 'Codex', installed: false, version: null },
  { name: 'OpenCode 2', installed: false, version: null },
];

describe('FirstLaunch', () => {
  it('shows the agent check and one Add project action', async () => {
    const user = userEvent.setup();
    const onAddProject = vi.fn();
    render(
      <FirstLaunch
        agents={agents}
        agentsChecked
        onAddProject={onAddProject}
      />,
    );

    expect(
      screen.getByRole('heading', { name: 'Welcome to elesdee' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Claude Code')).toBeInTheDocument();
    expect(screen.getByText('Installed · 2.1.328')).toBeInTheDocument();
    expect(screen.getAllByText('Not installed')).toHaveLength(2);
    expect(
      screen.getByText('Install Codex to start sessions with it.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Install OpenCode 2 to start sessions with it.'),
    ).toBeInTheDocument();
    expect(screen.getAllByRole('button')).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'Add project' }));

    expect(onAddProject).toHaveBeenCalledTimes(1);
  });

  it('says when the agent check is unavailable', () => {
    render(
      <FirstLaunch agents={[]} agentsChecked={false} onAddProject={vi.fn()} />,
    );

    expect(screen.getByText('Unavailable')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Add project' }),
    ).toBeInTheDocument();
  });
});
