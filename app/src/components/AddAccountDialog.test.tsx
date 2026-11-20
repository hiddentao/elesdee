import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, listProviders } from '../api/client';
import type { Account, ProviderInfo } from '../api/types';
import { AddAccountDialog } from './AddAccountDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return { ...actual, listProviders: vi.fn() };
});

const providers: ProviderInfo[] = [
  {
    id: 'anthropic',
    label: 'Anthropic',
    client: 'Claude Code',
    auth: ['subscription', 'key'],
    defaultModel: null,
    custom: false,
  },
  {
    id: 'google',
    label: 'Google',
    client: 'Antigravity',
    auth: ['key'],
    defaultModel: 'gemini-3.8-flash-medium',
    custom: false,
  },
  {
    id: 'custom',
    label: 'Custom provider',
    client: 'OpenCode 2',
    auth: ['key', 'local'],
    defaultModel: null,
    custom: true,
  },
];

const account: Account = {
  id: 'google-1',
  name: 'Google work',
  plan: 'pay as you go',
  client: 'Antigravity',
  provider: 'google',
  auth: 'key',
  sample: false,
  rows: [],
  foot: 'No official usage source.',
  server: null,
  model: 'gemini-3.8-flash-medium',
  apiStyle: null,
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

async function chooseGoogle(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByRole('option', { name: 'Google' });
  await user.selectOptions(screen.getByLabelText('Provider'), 'google');
}

describe('AddAccountDialog', () => {
  beforeEach(() => {
    vi.mocked(listProviders).mockResolvedValue(providers);
  });

  it('validates the account name and API key', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();
    await chooseGoogle(user);

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
    await chooseGoogle(user);

    expect(screen.getByLabelText('API key', { selector: '#add-account-key' })).toHaveAttribute('type', 'password');
    expect(screen.getByLabelText('Model (optional)')).toHaveAttribute(
      'placeholder',
      'gemini-3.8-flash-medium',
    );

    await user.type(screen.getByLabelText('Account name'), 'Google work');
    await user.type(screen.getByLabelText('API key', { selector: '#add-account-key' }), 'secret-key');
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(account));

    expect(onCreate).toHaveBeenCalledWith({
      name: 'Google work',
      providerId: 'google',
      auth: 'key',
      key: 'secret-key',
    });

    const logged = [...log.mock.calls, ...error.mock.calls, ...warn.mock.calls]
      .flat()
      .map(String)
      .join(' ');
    expect(logged).not.toContain('secret-key');
  });

  it('omits the model when the field is empty', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();
    await chooseGoogle(user);

    await user.type(screen.getByLabelText('Account name'), 'Google work');
    await user.type(screen.getByLabelText('API key', { selector: '#add-account-key' }), 'secret-key');
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('model');
  });

  it('sends a custom provider with its API style and server', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();
    await screen.findByRole('option', { name: 'Custom provider' });
    await user.selectOptions(
      screen.getByLabelText('Provider'),
      'custom',
    );

    await user.click(
      screen.getByRole('radio', { name: 'OpenAI-compatible' }),
    );
    await user.type(screen.getByLabelText('Account name'), 'Gateway');
    await user.type(screen.getByLabelText('API key', { selector: '#add-account-key' }), 'gateway-key');
    await user.type(
      screen.getByLabelText('Server URL'),
      'https://gateway.example/v1',
    );
    await user.type(screen.getByLabelText('Model (optional)'), 'gpt-test');
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1));
    expect(onCreate).toHaveBeenCalledWith({
      name: 'Gateway',
      providerId: 'custom',
      auth: 'key',
      key: 'gateway-key',
      server: 'https://gateway.example/v1',
      model: 'gpt-test',
      client: 'OpenCode 2',
      apiStyle: 'openai',
    });
  });

  it.each([400, 409, 422, 500])(
    'shows the server error inline for status %i',
    async (status) => {
      const user = userEvent.setup();
      const onCreate = vi
        .fn()
        .mockRejectedValue(new ApiError(status, `Server says ${status}.`));
      renderDialog(onCreate);
      await chooseGoogle(user);

      await user.type(screen.getByLabelText('Account name'), 'Google work');
      await user.type(screen.getByLabelText('API key', { selector: '#add-account-key' }), 'secret-key');
      await user.click(screen.getByRole('button', { name: 'Add account' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        `Server says ${status}.`,
      );
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    },
  );
});
