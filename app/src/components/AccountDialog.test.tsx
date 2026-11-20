import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, listProviders } from '../api/client';
import type { Account, ProviderInfo } from '../api/types';
import { AccountDialog } from './AccountDialog';

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
    id: 'deepseek',
    label: 'DeepSeek',
    client: 'Claude Code',
    auth: ['key'],
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

const deepseek: Account = {
  id: 'deepseek-work',
  name: 'DeepSeek work',
  plan: 'pay as you go',
  client: 'Claude Code',
  provider: 'deepseek',
  auth: 'key',
  sample: false,
  rows: [],
  foot: 'No official usage source.',
  server: null,
  model: 'deepseek-chat',
  apiStyle: null,
};

function renderAdd(onSubmit = vi.fn().mockResolvedValue(account)) {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(
    <AccountDialog onSubmit={onSubmit} onSaved={onSaved} onClose={onClose} />,
  );
  return { onSubmit, onSaved, onClose };
}

function renderEdit(onSubmit = vi.fn().mockResolvedValue(deepseek)) {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(
    <AccountDialog
      account={deepseek}
      onSubmit={onSubmit}
      onSaved={onSaved}
      onClose={onClose}
    />,
  );
  return { onSubmit, onSaved, onClose };
}

async function chooseGoogle(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByRole('option', { name: 'Google' });
  await user.selectOptions(screen.getByLabelText('Provider'), 'google');
}

describe('AccountDialog', () => {
  beforeEach(() => {
    vi.mocked(listProviders).mockResolvedValue(providers);
  });

  it('validates the account name and API key when adding', async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderAdd();
    await chooseGoogle(user);

    await user.click(screen.getByRole('button', { name: 'Add account' }));

    expect(screen.getByText('Enter an account name.')).toBeInTheDocument();
    expect(screen.getByText('Enter an API key.')).toBeInTheDocument();
    expect(screen.getByLabelText('Account name')).toHaveFocus();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('sends the Google account body without exposing the key', async () => {
    const user = userEvent.setup();
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { onSubmit, onSaved } = renderAdd();
    await chooseGoogle(user);

    expect(
      screen.getByLabelText('API key', { selector: '#account-key' }),
    ).toHaveAttribute('type', 'password');
    expect(screen.getByLabelText('Model (optional)')).toHaveAttribute(
      'placeholder',
      'gemini-3.8-flash-medium',
    );

    await user.type(screen.getByLabelText('Account name'), 'Google work');
    await user.type(
      screen.getByLabelText('API key', { selector: '#account-key' }),
      'secret-key',
    );
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(account));

    expect(onSubmit).toHaveBeenCalledWith({
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

  it('sends a custom provider with its API style and server', async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderAdd();
    await screen.findByRole('option', { name: 'Custom provider' });
    await user.selectOptions(screen.getByLabelText('Provider'), 'custom');

    await user.click(
      screen.getByRole('radio', { name: 'OpenAI-compatible' }),
    );
    await user.type(screen.getByLabelText('Account name'), 'Gateway');
    await user.type(
      screen.getByLabelText('API key', { selector: '#account-key' }),
      'gateway-key',
    );
    await user.type(
      screen.getByLabelText('Server URL'),
      'https://gateway.example/v1',
    );
    await user.type(screen.getByLabelText('Model (optional)'), 'gpt-test');
    await user.click(screen.getByRole('button', { name: 'Add account' }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit).toHaveBeenCalledWith({
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

  it('fills the edit form and keeps the saved key when the field is blank', async () => {
    const user = userEvent.setup();
    const { onSubmit, onSaved } = renderEdit();

    expect(
      screen.getByRole('dialog', { name: 'Edit account' }),
    ).toBeInTheDocument();
    await screen.findByRole('option', { name: 'DeepSeek' });
    expect(screen.getByLabelText('Account name')).toHaveValue('DeepSeek work');
    expect(screen.getByLabelText('Model (optional)')).toHaveValue(
      'deepseek-chat',
    );
    expect(
      screen.getByLabelText('API key (leave blank to keep the saved key)'),
    ).toHaveValue('');

    await user.clear(screen.getByLabelText('Account name'));
    await user.type(screen.getByLabelText('Account name'), 'DeepSeek edited');
    await user.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(deepseek));
    expect(onSubmit).toHaveBeenCalledWith({
      name: 'DeepSeek edited',
      providerId: 'deepseek',
      auth: 'key',
      model: 'deepseek-chat',
    });
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty('key');
  });

  it('sends a replacement key when the edit field is filled', async () => {
    const user = userEvent.setup();
    const { onSubmit } = renderEdit();

    await user.type(
      screen.getByLabelText('API key (leave blank to keep the saved key)'),
      'sk-new-key',
    );
    await user.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit).toHaveBeenCalledWith({
      name: 'DeepSeek work',
      providerId: 'deepseek',
      auth: 'key',
      key: 'sk-new-key',
      model: 'deepseek-chat',
    });
  });

  it.each([400, 409, 422, 500])(
    'shows the server error inline for status %i',
    async (status) => {
      const user = userEvent.setup();
      const onSubmit = vi
        .fn()
        .mockRejectedValue(new ApiError(status, `Server says ${status}.`));
      renderAdd(onSubmit);
      await chooseGoogle(user);

      await user.type(screen.getByLabelText('Account name'), 'Google work');
      await user.type(
        screen.getByLabelText('API key', { selector: '#account-key' }),
        'secret-key',
      );
      await user.click(screen.getByRole('button', { name: 'Add account' }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        `Server says ${status}.`,
      );
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    },
  );
});
