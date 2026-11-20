import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentCheck,
  AiConfig,
  Client,
  ClientInstall,
  CreateConfigInput,
  Provider,
  ProviderOption,
  UpdateConfigInput,
} from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { AiConfigDialog } from './AiConfigDialog';

const provider: Provider = {
  id: 'p1',
  name: 'DeepSeek',
  vendor: 'deepseek',
  auth: 'key',
  server: null,
  sample: false,
  connected: true,
  plan: 'pay as you go',
  rows: [],
  foot: '',
  supportedClients: ['Claude Code', 'Codex', 'OpenCode 2'],
};

const option: ProviderOption = {
  id: 'deepseek',
  label: 'DeepSeek',
  auth: ['key'],
  supportedClients: ['Claude Code', 'Codex', 'OpenCode 2'],
  efforts: {
    'Claude Code': ['low', 'medium', 'high', 'xhigh', 'max'],
    Codex: ['minimal', 'low', 'medium', 'high'],
    'OpenCode 2': ['low', 'medium', 'high'],
  },
  defaultModel: null,
  custom: false,
  signInClient: null,
};

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const agents: AgentCheck[] = [
  { name: 'Claude Code', installed: false, version: null, path: null },
  {
    name: 'Codex',
    installed: true,
    version: '0.9.0 (codex)',
    path: '/usr/local/bin/codex',
  },
  {
    name: 'OpenCode 2',
    installed: true,
    version: '1.0.0',
    path: '/usr/local/bin/opencode',
  },
];

interface InstallCallbacks {
  onInstallClient: (client: Client) => Promise<ClientInstall>;
  onGetClientInstall: (id: string) => Promise<ClientInstall>;
  onClientInstalled: (agent: AgentCheck) => void;
}

function renderDialog(
  onSaveConfig: (
    id: string | null,
    input: CreateConfigInput | UpdateConfigInput,
  ) => Promise<AiConfig>,
  installCallbacks: InstallCallbacks = {
    onInstallClient: vi.fn(),
    onGetClientInstall: vi.fn(),
    onClientInstalled: vi.fn(),
  },
) {
  render(
    <LanguageProvider>
      <AiConfigDialog
        providers={[provider]}
        options={[option]}
        agents={agents}
        agentsChecked
        onInstallClient={installCallbacks.onInstallClient}
        onGetClientInstall={installCallbacks.onGetClientInstall}
        onClientInstalled={installCallbacks.onClientInstalled}
        onSaveConfig={onSaveConfig}
        onCreateProvider={vi.fn()}
        onStartProviderSignIn={vi.fn()}
        onGetProviderSignIn={vi.fn()}
        onSubmitProviderSignInCode={vi.fn()}
        onCancelProviderSignIn={vi.fn()}
        onProviderSaved={vi.fn()}
        onSaved={vi.fn()}
        onClose={vi.fn()}
      />
    </LanguageProvider>,
  );
  return onSaveConfig;
}

function installJob(overrides: Partial<ClientInstall> = {}): ClientInstall {
  return {
    id: 'install-1',
    client: 'Claude Code',
    state: 'running',
    stage: 'installing',
    step: 3,
    total: 5,
    version: null,
    path: null,
    error: null,
    ...overrides,
  };
}

describe('AiConfigDialog', () => {
  it('loads models for the chosen client and saves the model, effort and arguments', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/models?client=Codex')) {
        return jsonResponse({
          models: [
            { id: 'gateway-large', label: 'Gateway Large' },
            { id: 'gateway-small', label: 'Gateway Small' },
          ],
          source: 'provider',
          fallback: false,
        });
      }
      return jsonResponse({});
    });
    vi.stubGlobal('fetch', fetchMock);
    const onSaveConfig = vi.fn(async (_id: string | null, _input: unknown) => ({
      id: 'c1',
      name: 'DeepSeek (Codex)',
      providerId: 'p1',
      client: 'Codex' as const,
      model: 'gateway-large',
      effort: 'high',
      args: ['--verbose'],
      env: [],
      config: {},
      createdAt: '',
      updatedAt: '',
    }));
    renderDialog(onSaveConfig);
    const user = userEvent.setup();

    await user.click(screen.getByRole('radio', { name: 'Codex' }));
    await user.click(screen.getByLabelText('Model'));
    const large = await screen.findByRole('option', { name: /Gateway Large/ });
    await user.click(large);
    await user.selectOptions(screen.getByLabelText('Effort'), 'high');
    await user.type(
      screen.getByLabelText('Extra command-line arguments'),
      '--verbose',
    );
    await user.click(screen.getByRole('button', { name: 'Add AI config' }));

    await waitFor(() => expect(onSaveConfig).toHaveBeenCalledTimes(1));
    expect(onSaveConfig).toHaveBeenCalledWith(null, {
      providerId: 'p1',
      client: 'Codex',
      model: 'gateway-large',
      effort: 'high',
      args: ['--verbose'],
      env: [],
      config: {},
    });
    expect(
      fetchMock.mock.calls.some(([url]) =>
        String(url).includes('/models?client=Codex'),
      ),
    ).toBe(true);
  });

  it('accepts a typed model name that the provider does not list', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          models: [{ id: 'gateway-large', label: 'Gateway Large' }],
          source: 'known',
          fallback: true,
        }),
      ),
    );
    const onSaveConfig = vi.fn(async (_id: string | null, _input: unknown) => ({
      id: 'c2',
      name: 'DeepSeek (Codex)',
      providerId: 'p1',
      client: 'Codex' as const,
      model: 'my-custom-model',
      effort: null,
      args: [],
      env: [],
      config: {},
      createdAt: '',
      updatedAt: '',
    }));
    renderDialog(onSaveConfig);
    const user = userEvent.setup();

    await user.click(screen.getByRole('radio', { name: 'Codex' }));
    const modelInput = screen.getByLabelText('Model');
    await user.click(modelInput);
    await screen.findByText(/The provider list could not be read/);
    await user.type(modelInput, 'my-custom-model');
    await user.keyboard('{Enter}');
    await user.click(screen.getByRole('button', { name: 'Add AI config' }));

    await waitFor(() => expect(onSaveConfig).toHaveBeenCalledTimes(1));
    expect(onSaveConfig.mock.calls[0][1]).toMatchObject({
      model: 'my-custom-model',
      client: 'Codex',
    });
  });

  it('shows one install action for each missing client', () => {
    renderDialog(vi.fn());

    expect(screen.getByText('Not installed')).toBeInTheDocument();
    expect(screen.getByText('Installed · 0.9.0')).toBeInTheDocument();
    expect(screen.getByText('Installed · 1.0.0')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Install Claude Code' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Install Codex' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Install OpenCode 2' }),
    ).not.toBeInTheDocument();
  });

  it('installs a missing client after confirmation and shows progress', async () => {
    const onInstallClient = vi.fn(async () => installJob());
    const onGetClientInstall = vi
      .fn<(id: string) => Promise<ClientInstall>>()
      .mockResolvedValueOnce(installJob({ stage: 'verifying', step: 4 }))
      .mockResolvedValue(
        installJob({
          state: 'done',
          stage: 'finishing',
          step: 5,
          version: '3.0.0',
          path: '/data/clients/claude-code/bin/claude',
        }),
      );
    const onClientInstalled = vi.fn();
    renderDialog(vi.fn(), {
      onInstallClient,
      onGetClientInstall,
      onClientInstalled,
    });
    const user = userEvent.setup();

    await user.click(
      screen.getByRole('button', { name: 'Install Claude Code' }),
    );
    const dialog = screen.getByRole('dialog', { name: 'Install Claude Code' });
    expect(
      within(dialog).getByText(/download Claude Code from its vendor/),
    ).toBeInTheDocument();
    await user.click(
      within(dialog).getByRole('button', { name: 'Install Claude Code' }),
    );

    expect(onInstallClient).toHaveBeenCalledWith('Claude Code');
    await waitFor(() => expect(onGetClientInstall).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        screen.getByText('Installed · 3.0.0'),
      ).toBeInTheDocument(),
    );
    expect(onClientInstalled).toHaveBeenCalledWith({
      name: 'Claude Code',
      installed: true,
      version: '3.0.0',
      path: '/data/clients/claude-code/bin/claude',
    });
    expect(
      screen.queryByRole('button', { name: 'Install Claude Code' }),
    ).not.toBeInTheDocument();
  });

  it('reports a failed install and keeps the install action', async () => {
    const onInstallClient = vi.fn(async () =>
      installJob({
        state: 'failed',
        error: 'The download failed.',
      }),
    );
    const onGetClientInstall = vi.fn();
    renderDialog(vi.fn(), {
      onInstallClient,
      onGetClientInstall,
      onClientInstalled: vi.fn(),
    });
    const user = userEvent.setup();

    await user.click(
      screen.getByRole('button', { name: 'Install Claude Code' }),
    );
    const dialog = screen.getByRole('dialog', { name: 'Install Claude Code' });
    await user.click(
      within(dialog).getByRole('button', { name: 'Install Claude Code' }),
    );

    expect(
      await screen.findByText('Could not install Claude Code. The download failed.'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Install Claude Code' }),
    ).toBeInTheDocument();
    expect(onGetClientInstall).not.toHaveBeenCalled();
  });
});
