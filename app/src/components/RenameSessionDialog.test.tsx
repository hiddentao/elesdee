import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import { makeSession } from '../test/fixtures';
import { RenameSessionDialog } from './RenameSessionDialog';

const session = makeSession({
  name: 'Check close',
  short: 'check-close',
  branch: 'elesdee/core/check-close',
});

function renderDialog(onRename = vi.fn(async () => {})) {
  const onClose = vi.fn();
  render(
    <RenameSessionDialog
      session={session}
      onRename={onRename}
      onClose={onClose}
    />,
  );
  return { onRename, onClose };
}

describe('RenameSessionDialog', () => {
  it('prefills the name and rejects an empty name', async () => {
    const user = userEvent.setup();
    const { onRename } = renderDialog();
    const input = screen.getByLabelText('Name');

    expect(input).toHaveValue('Check close');

    await user.clear(input);
    await user.click(screen.getByRole('button', { name: 'Rename' }));

    expect(screen.getByText('Enter a name.')).toBeInTheDocument();
    expect(onRename).not.toHaveBeenCalled();
  });

  it('rejects a name over 64 characters', async () => {
    const user = userEvent.setup();
    const { onRename } = renderDialog();

    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'a'.repeat(65) },
    });
    await user.click(screen.getByRole('button', { name: 'Rename' }));

    expect(
      screen.getByText('Use 64 characters or fewer.'),
    ).toBeInTheDocument();
    expect(onRename).not.toHaveBeenCalled();
  });

  it('renames and closes on success', async () => {
    const user = userEvent.setup();
    const onRename = vi.fn(async () => {});
    const { onClose } = renderDialog(onRename);
    const input = screen.getByLabelText('Name');

    await user.clear(input);
    await user.type(input, '  New name  ');
    await user.click(screen.getByRole('button', { name: 'Rename' }));

    await waitFor(() => expect(onRename).toHaveBeenCalledWith('New name'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('shows a server error inline', async () => {
    const user = userEvent.setup();
    const onRename = vi
      .fn()
      .mockRejectedValue(new ApiError(404, 'The session was not found.'));
    const { onClose } = renderDialog(onRename);

    await user.click(screen.getByRole('button', { name: 'Rename' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The session was not found.',
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
