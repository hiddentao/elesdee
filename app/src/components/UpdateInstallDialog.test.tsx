import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { UpdateInstallDialog } from './UpdateInstallDialog';

describe('UpdateInstallDialog', () => {
  it('warns about running sessions and installs on confirm', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onLater = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();

    render(
      <UpdateInstallDialog
        sessionCount={2}
        onConfirm={onConfirm}
        onLater={onLater}
        onClose={onClose}
      />,
    );

    expect(
      screen.getByRole('heading', { name: 'Install the update?' }),
    ).toBeVisible();
    expect(screen.getByText('2 sessions are running.')).toBeVisible();

    await user.click(
      screen.getByRole('button', { name: 'Install and restart' }),
    );
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onLater).not.toHaveBeenCalled();
  });

  it('defers the install until the sessions close', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onLater = vi.fn().mockResolvedValue(undefined);

    render(
      <UpdateInstallDialog
        sessionCount={1}
        onConfirm={onConfirm}
        onLater={onLater}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText('One session is running.')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Later' }));
    expect(onLater).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('shows an error when the install does not start', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockRejectedValue(new Error('unavailable'));

    render(
      <UpdateInstallDialog
        sessionCount={1}
        onConfirm={onConfirm}
        onLater={vi.fn().mockResolvedValue(undefined)}
        onClose={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole('button', { name: 'Install and restart' }),
    );

    expect(
      await screen.findByText(
        'The update could not be installed. The app keeps running.',
      ),
    ).toBeVisible();
  });
});
