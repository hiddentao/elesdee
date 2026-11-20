import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { QuitDialog } from './QuitDialog';

describe('QuitDialog', () => {
  it('warns about running sessions and confirms a quit', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();

    render(
      <QuitDialog sessionCount={2} onConfirm={onConfirm} onClose={onClose} />,
    );

    expect(screen.getByRole('heading', { name: 'Quit elesdee?' })).toBeVisible();
    expect(screen.getByText('2 sessions are still running.')).toBeVisible();
    expect(
      screen.getByText(
        'Closing the window keeps elesdee and its sessions running.',
      ),
    ).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Quit' }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it('cancels without quitting', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();

    render(
      <QuitDialog sessionCount={1} onConfirm={onConfirm} onClose={onClose} />,
    );

    expect(screen.getByText('One session is still running.')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
