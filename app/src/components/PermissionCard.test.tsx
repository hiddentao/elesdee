import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { PermissionCard } from './PermissionCard';

const permission = {
  requestId: 'req-1',
  tool: 'Bash',
  summary: 'pnpm test --filter web',
};

describe('PermissionCard', () => {
  it('announces the request and shows the tool and summary', () => {
    render(<PermissionCard permission={permission} onRespond={vi.fn()} />);

    expect(
      screen.getByRole('heading', { name: 'Permission needed' }),
    ).toBeInTheDocument();

    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Bash');
    expect(status).toHaveTextContent('pnpm test --filter web');
    expect(screen.getByText('pnpm test --filter web').tagName).toBe('PRE');

    expect(screen.getByRole('button', { name: 'Allow' })).not.toHaveFocus();
    expect(screen.getByRole('button', { name: 'Deny' })).not.toHaveFocus();
  });

  it('sends allow and disables both buttons until the session changes', async () => {
    const user = userEvent.setup();
    let resolveAnswer: () => void = () => {};
    const onRespond = vi.fn().mockReturnValue(
      new Promise<void>((resolve) => {
        resolveAnswer = resolve;
      }),
    );

    render(<PermissionCard permission={permission} onRespond={onRespond} />);

    await user.click(screen.getByRole('button', { name: 'Allow' }));

    expect(onRespond).toHaveBeenCalledWith('allow');
    expect(
      screen.getByRole('button', { name: 'Allowing…' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeDisabled();

    resolveAnswer();

    expect(
      screen.getByRole('button', { name: 'Allowing…' }),
    ).toBeDisabled();
  });

  it('sends deny and shows the denying state', async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn().mockResolvedValue(undefined);

    render(<PermissionCard permission={permission} onRespond={onRespond} />);

    await user.click(screen.getByRole('button', { name: 'Deny' }));

    expect(onRespond).toHaveBeenCalledWith('deny');
    expect(
      screen.getByRole('button', { name: 'Denying…' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Allow' })).toBeDisabled();
  });

  it('re-enables the buttons when the answer fails', async () => {
    const user = userEvent.setup();
    const onRespond = vi.fn().mockRejectedValue(new Error('Network error.'));

    render(<PermissionCard permission={permission} onRespond={onRespond} />);

    await user.click(screen.getByRole('button', { name: 'Allow' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The answer was not sent. Try again.',
    );
    expect(screen.getByRole('button', { name: 'Allow' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeEnabled();
  });
});
