import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type { Account } from '../api/types';
import { AddAccountDialog } from './AddAccountDialog';

const account: Account = {
  id: 'google-1',
  name: 'Google work',
  plan: 'Google AI',
  client: 'Antigravity',
  provider: 'google',
  auth: 'key',
  rows: [],
  foot: 'No official usage source.',
  server: null,
  model: 'gemini-3.8-flash-medium',
};

function renderDialog(onCreate = vi.fn().mockResolvedValue(account)) {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <AddAccountDialog
      onCreate={onCreate}
      onCreated={onCreated}
      onClose={onClose}
    />,
  );
  return { onCreate, onCreated, onClose };
}

describe('AddAccountDialog', () => {
  it('validates the account name and API key', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Add account' }));

    expect(screen.getByText('Enter an account name.')).toBeInTheDocument();
    expect(screen.getByText('Enter an API key.')).toBeInTheDocument();
    expect(screen.getByLabelText('Account name')).toHaveFocus();
    expect(onCreate).not.toHaveBeenCalled();
  });

  it('sends the Google account body without exposing the key', async () => {
    const user = userEvent.setup();
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { onCreate, onCreated } = renderDialog();

    expect(screen.getByLabelText('API key')).toHaveAttribute('type', 'password');
    expect(screen.getByLabelText('Model')).toHaveValue(
      'gemini-3.8-flash-medium',
    );
    expect(screen.getByText('Google')).toBeInTheDocument();
    expect(screen.getByText('Antigravity')).toBeInTheDocument();

    await user.type(screen.getByLabelText('Account name'), 'Google work');
    await user.type(screen.getByLabelText('API key'), 'secret-key');
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(account));

    expect(onCreate).toHaveBeenCalledWith({
      name: 'Google work',
      providerId: 'google',
      client: 'Antigravity',
      auth: 'key',
      key: 'secret-key',
      model: 'gemini-3.8-flash-medium',
    });

    const logged = [...log.mock.calls, ...error.mock.calls, ...warn.mock.calls]
      .flat()
      .map(String)
      .join(' ');
    expect(logged).not.toContain('secret-key');
  });

  it('omits the model when the field is cleared', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    await user.type(screen.getByLabelText('Account name'), 'Google work');
    await user.type(screen.getByLabelText('API key'), 'secret-key');
    await user.clear(screen.getByLabelText('Model'));
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('model');
  });

  it.each([400, 409, 422, 500])(
    'shows the server error inline for status %i',
    async (status) => {
      const user = userEvent.setup();
      const onCreate = vi
        .fn()
        .mockRejectedValue(new ApiError(status, `Server says ${status}.`));
      renderDialog(onCreate);

      await user.type(screen.getByLabelText('Account name'), 'Google work');
      await user.type(screen.getByLabelText('API key'), 'secret-key');
      await user.click(screen.getByRole('button', { name: 'Add account' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        `Server says ${status}.`,
      );
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    },
  );
});
