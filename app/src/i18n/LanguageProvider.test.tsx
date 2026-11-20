import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Provider } from '../api/types';
import { SettingsDialog } from '../components/SettingsDialog';
import {
  DESKTOP_LANGUAGE_KEY,
  LanguageProvider,
  useLanguage,
} from './LanguageProvider';

function Probe() {
  const { language, setLanguage, t } = useLanguage();
  return (
    <div>
      <span data-testid="language">{language}</span>
      <span data-testid="closed">{t('status.closed')}</span>
      <button type="button" onClick={() => setLanguage('de')}>
        switch
      </button>
    </div>
  );
}

const provider: Provider = {
  id: 'a1',
  name: 'Work',
  vendor: 'anthropic',
  auth: 'subscription',
  sample: true,
  connected: false,
  plan: 'Max',
  rows: [{ label: '5-hour', used: 58 }],
  foot: '',
  server: null,
  supportedClients: ['Claude Code', 'OpenCode 2'],
};

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.lang = 'en';
});

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.lang = 'en';
  vi.unstubAllGlobals();
});

describe('LanguageProvider', () => {
  it('starts in English when no choice is stored', () => {
    render(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );

    expect(screen.getByTestId('language')).toHaveTextContent('en');
    expect(screen.getByTestId('closed')).toHaveTextContent('Closed');
    expect(document.documentElement.lang).toBe('en');
  });

  it('uses the computer language on first launch', () => {
    vi.stubGlobal('navigator', {
      languages: ['pt-PT'],
      language: 'pt-PT',
    });

    render(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );

    expect(screen.getByTestId('language')).toHaveTextContent('pt-BR');
    expect(screen.getByTestId('closed')).toHaveTextContent('Fechada');
    expect(document.documentElement.lang).toBe('pt-BR');
  });

  it('falls back to English for an unsupported computer language', () => {
    vi.stubGlobal('navigator', {
      languages: ['xx-YY'],
      language: 'xx-YY',
    });

    render(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );

    expect(screen.getByTestId('language')).toHaveTextContent('en');
  });

  it('reads a stored choice before the computer language', () => {
    vi.stubGlobal('navigator', {
      languages: ['ja-JP'],
      language: 'ja-JP',
    });
    window.localStorage.setItem(DESKTOP_LANGUAGE_KEY, 'fr');

    render(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );

    expect(screen.getByTestId('language')).toHaveTextContent('fr');
    expect(screen.getByTestId('closed')).toHaveTextContent('Fermée');
  });

  it('switches language, stores the choice and updates the html tag', async () => {
    const user = userEvent.setup();
    render(
      <LanguageProvider>
        <Probe />
      </LanguageProvider>,
    );

    await user.click(screen.getByRole('button', { name: 'switch' }));

    expect(screen.getByTestId('language')).toHaveTextContent('de');
    expect(screen.getByTestId('closed')).toHaveTextContent('Geschlossen');
    expect(document.documentElement.lang).toBe('de');
    expect(window.localStorage.getItem(DESKTOP_LANGUAGE_KEY)).toBe('de');
  });

  it('keeps English when used without a provider', () => {
    render(<Probe />);

    expect(screen.getByTestId('language')).toHaveTextContent('en');
    expect(screen.getByTestId('closed')).toHaveTextContent('Closed');
  });

  it('switches the Settings page and its labels', async () => {
    const user = userEvent.setup();
    render(
      <LanguageProvider>
        <SettingsDialog
          providers={[provider]}
          configs={[]}
          options={[]}
          version="0.1.0"
          agents={[]}
          agentsChecked
          onInstallClient={vi.fn()}
          onUpdateClient={vi.fn()}
          onCheckClientUpdate={vi.fn()}
          onGetClientInstall={vi.fn()}
          onClientInstalled={vi.fn()}
          update={null}
          settings={{ startAtLogin: false, keepAwake: false }}
          startAtLoginActual={null}
          startAtLoginError={null}
          keepAwakeActive={null}
          keepAwakeError={null}
          onCreateProvider={vi.fn()}
          onUpdateProvider={vi.fn()}
          onRemoveProvider={vi.fn()}
          onStartProviderSignIn={vi.fn()}
          onGetProviderSignIn={vi.fn()}
          onSubmitProviderSignInCode={vi.fn()}
          onCancelProviderSignIn={vi.fn()}
          onCreateConfig={vi.fn()}
          onUpdateConfig={vi.fn()}
          onDeleteConfig={vi.fn()}
          onProviderSaved={vi.fn()}
          onConfigSaved={vi.fn()}
          onChangeSettings={vi.fn()}
          onLoadDevices={vi.fn()}
          onRemoveDevice={vi.fn()}
          onStartPairing={vi.fn()}
          onPairingStatus={vi.fn()}
          onApprovePairing={vi.fn()}
          onDenyPairing={vi.fn()}
          onCancelPairing={vi.fn()}
          pairingQrUrl={(cacheBust: number) => `/api/pairing/qr?v=${cacheBust}`}
          onExportDiagnostics={vi.fn()}
          onCheckForUpdate={vi.fn()}
          onInstallUpdate={vi.fn()}
          onClose={vi.fn()}
        />
      </LanguageProvider>,
    );

    const settings = screen.getByRole('dialog', { name: 'Settings' });
    await user.click(
      screen.getByRole('button', { name: 'Languages' }),
    );

    expect(
      screen.getByRole('heading', { name: 'Languages' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Interface language')).toBeInTheDocument();

    await user.selectOptions(
      screen.getByLabelText('Interface language'),
      'de',
    );

    expect(
      screen.getByRole('dialog', { name: 'Einstellungen' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Sprachen' }),
    ).toHaveAttribute('aria-current', 'page');
    expect(screen.getByText('Oberflächensprache')).toBeInTheDocument();
    expect(settings).toBeInTheDocument();
  });
});
