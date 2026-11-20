import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AiConfig,
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

function renderDialog(
  onSaveConfig: (
    id: string | null,
    input: CreateConfigInput | UpdateConfigInput,
  ) => Promise<AiConfig>,
) {
  render(
    <LanguageProvider>
      <AiConfigDialog
        providers={[provider]}
        options={[option]}
        onSaveConfig={onSaveConfig}
        onCreateProvider={vi.fn()}
        onProviderSaved={vi.fn()}
        onSaved={vi.fn()}
        onClose={vi.fn()}
      />
    </LanguageProvider>,
  );
  return onSaveConfig;
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
});
