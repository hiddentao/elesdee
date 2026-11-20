import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { SessionRequest } from '../api/types';
import { SessionRequestForm } from './SessionRequestForm';

const request: SessionRequest = {
  id: 'r1',
  parentSessionId: 's1',
  parentSessionName: 'Coordinator',
  projectId: 'p1',
  projectName: 'web',
  name: 'Helper',
  folder: '/dev/web',
  sourceBranch: 'main',
  firstPrompt: 'Fix the tests',
  accountId: 'a1',
  client: 'Claude Code',
  status: 'pending',
  childSessionId: null,
  createdAt: '2026-01-01T00:00:00Z',
};

describe('SessionRequestForm', () => {
  it('approves with the further-sessions option', async () => {
    const user = userEvent.setup();
    const onApprove = vi.fn().mockResolvedValue(undefined);
    render(
      <SessionRequestForm
        request={request}
        onApprove={onApprove}
        onDeny={vi.fn()}
      />,
    );

    expect(screen.getByText('Coordinator')).toBeInTheDocument();
    expect(screen.getByText('Fix the tests')).toBeInTheDocument();
    await user.click(
      screen.getByLabelText('Allow further sessions from this session'),
    );
    await user.click(screen.getByRole('button', { name: 'Approve' }));

    expect(onApprove).toHaveBeenCalledWith(true);
  });

  it('denies the request', async () => {
    const user = userEvent.setup();
    const onDeny = vi.fn().mockResolvedValue(undefined);
    render(
      <SessionRequestForm
        request={request}
        onApprove={vi.fn()}
        onDeny={onDeny}
      />,
    );

    await user.click(screen.getByRole('button', { name: 'Deny' }));
    expect(onDeny).toHaveBeenCalledOnce();
  });
});
