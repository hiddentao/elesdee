import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type {
  Provider,
  ProviderOption,
  ProviderSignIn,
} from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { ProviderDialog } from './ProviderDialog';

vi.mock('../api/shell', () => ({
  openUrl: vi.fn(async () => undefined),
}));

import { openUrl } from '../api/shell';

const options: ProviderOption[] = [
  {
    id: 'anthropic',
    label: 'Anthropic',
    auth: ['subscription', 'key'],
    supportedClients: ['Claude Code', 'OpenCode 2'],
    efforts: {},
    defaultModel: null,
    custom: false,
    signInClient: 'Claude Code',
  },
  {
    id: 'openai',
    label: 'OpenAI',
    auth: ['subscription', 'key'],
    supportedClients: ['Codex', 'OpenCode 2'],
    efforts: {},
    defaultModel: null,
    custom: false,
    signInClient: 'Codex',
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    auth: ['key'],
    supportedClients: ['Claude Code', 'Codex', 'OpenCode 2'],
    efforts: {},
    defaultModel: null,
    custom: false,
    signInClient: null,
  },
];

const provider: Provider = {
  id: 'work-claude',
  name: 'Work Claude',
  vendor: 'anthropic',
  auth: 'subscription',
  server: null,
  sample: false,
  connected: false,
  plan: 'subscription',
  rows: [],
  foot: '',
  supportedClients: ['Claude Code', 'OpenCode 2'],
};

function view(overrides: Partial<ProviderSignIn>): ProviderSignIn {
  return {
    id: 'sign-in-1',
    vendor: 'anthropic',
    client: 'Claude Code',
    mode: 'browser',
    state: 'starting',
    url: null,
    code: null,
    reason: null,
    provider: null,
    ...overrides,
  };
}

function renderDialog(overrides: {
  provider?: Provider | null;
  onStartSignIn?: ReturnType<typeof vi.fn>;
  onGetSignIn?: ReturnType<typeof vi.fn>;
  onSubmitSignInCode?: ReturnType<typeof vi.fn>;
  onSubmit?: ReturnType<typeof vi.fn>;
  onSaved?: ReturnType<typeof vi.fn>;
  onCancelSignIn?: ReturnType<typeof vi.fn>;
} = {}) {
  const props = {
    provider: overrides.provider ?? null,
    options,
    onSubmit: (overrides.onSubmit ?? vi.fn()) as never,
    onStartSignIn: (overrides.onStartSignIn ?? vi.fn()) as never,
    onGetSignIn: (overrides.onGetSignIn ?? vi.fn()) as never,
    onSubmitSignInCode: (overrides.onSubmitSignInCode ?? vi.fn()) as never,
    onCancelSignIn: (overrides.onCancelSignIn ?? vi.fn()) as never,
    onSaved: (overrides.onSaved ?? vi.fn()) as never,
    onClose: vi.fn(),
  };
  render(
    <LanguageProvider>
      <ProviderDialog {...props} />
    </LanguageProvider>,
  );
  return props;
}

describe('ProviderDialog subscription sign-in', () => {
  it('starts the sign-in and shows the waiting state with the address', async () => {
    const user = userEvent.setup();
    const waiting = view({
      state: 'waiting',
      url: 'https://example.com/signin',
    });
    const props = renderDialog({
      onStartSignIn: vi.fn(async () => waiting),
      onGetSignIn: vi.fn(async () => waiting),
    });

    await user.click(screen.getByRole('radio', { name: 'Subscription' }));
    await user.click(
      screen.getByRole('button', { name: 'Sign in and add' }),
    );

    expect(props.onStartSignIn).toHaveBeenCalledWith({
      vendor: 'anthropic',
      name: undefined,
      providerId: undefined,
      mode: 'browser',
    });
    expect(
      await screen.findByText(/Finish signing in through Claude Code/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: 'Open the sign-in page' }),
    );
    expect(openUrl).toHaveBeenCalledWith('https://example.com/signin');
  });

  it('shows the client-issued code and accepts a pasted code', async () => {
    const user = userEvent.setup();
    const waiting = view({ state: 'waiting', code: 'ABCD-EFGH' });
    const props = renderDialog({
      onStartSignIn: vi.fn(async () => waiting),
      onGetSignIn: vi.fn(() => new Promise(() => undefined)),
      onSubmitSignInCode: vi.fn(async () => view({ state: 'verifying' })),
    });

    await user.click(screen.getByRole('radio', { name: 'Subscription' }));
    await user.click(
      screen.getByRole('button', { name: 'Sign in and add' }),
    );

    expect(
      await screen.findByText(/Enter this code on the sign-in page/),
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText('One-time code'), 'WXYZ-1234');
    await user.click(screen.getByRole('button', { name: 'Submit code' }));
    expect(props.onSubmitSignInCode).toHaveBeenCalledWith(
      'sign-in-1',
      'WXYZ-1234',
    );
    expect(await screen.findByText('Checking the credential…')).toBeInTheDocument();
  });

  it('uses the device-code flow for OpenAI', async () => {
    const user = userEvent.setup();
    const waiting = view({
      vendor: 'openai',
      client: 'Codex',
      mode: 'code',
      state: 'waiting',
      code: 'AAAAB-BB6TU',
    });
    const props = renderDialog({
      onStartSignIn: vi.fn(async () => waiting),
      onGetSignIn: vi.fn(async () => waiting),
    });

    await user.selectOptions(screen.getByLabelText('Vendor'), 'openai');
    await user.click(screen.getByRole('radio', { name: 'Subscription' }));
    await user.click(
      screen.getByRole('button', { name: 'Use a one-time code instead' }),
    );

    expect(props.onStartSignIn).toHaveBeenCalledWith({
      vendor: 'openai',
      name: undefined,
      providerId: undefined,
      mode: 'code',
    });
    expect(
      await screen.findByText(/AAAAB-BB6TU/),
    ).toBeInTheDocument();
  });

  it('shows the reason a sign-in failed and offers a new attempt', async () => {
    const user = userEvent.setup();
    const failed = view({ state: 'failed', reason: 'timeout' });
    const props = renderDialog({
      onStartSignIn: vi.fn(async () => failed),
      onGetSignIn: vi.fn(async () => failed),
    });

    await user.click(screen.getByRole('radio', { name: 'Subscription' }));
    await user.click(
      screen.getByRole('button', { name: 'Sign in and add' }),
    );
    expect(
      await screen.findByText('The sign-in timed out. Try again.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(props.onStartSignIn).toHaveBeenCalledTimes(2);
  });

  it('keeps the API-key path', async () => {
    const user = userEvent.setup();
    const props = renderDialog();

    await user.selectOptions(screen.getByLabelText('Vendor'), 'deepseek');
    await user.type(
      screen.getByLabelText('API key', {
        selector: 'input[type="password"]',
      }),
      'sk-test',
    );
    await user.click(screen.getByRole('button', { name: 'Add provider' }));

    expect(props.onSubmit).toHaveBeenCalledWith({
      vendor: 'deepseek',
      auth: 'key',
      key: 'sk-test',
    });
  });

  it('shows a disconnected subscription provider and reconnects it', async () => {
    const user = userEvent.setup();
    const waiting = view({ state: 'waiting' });
    const props = renderDialog({
      provider,
      onStartSignIn: vi.fn(async () => waiting),
      onGetSignIn: vi.fn(async () => waiting),
    });

    expect(screen.getByText('Not connected')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(props.onStartSignIn).toHaveBeenCalledWith({
      vendor: 'anthropic',
      name: 'Work Claude',
      providerId: 'work-claude',
      mode: 'browser',
    });
  });
});
