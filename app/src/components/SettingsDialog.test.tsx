import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { SettingsDialog } from './SettingsDialog';


function renderSettings(overrides: Partial<Parameters<typeof SettingsDialog>[0]> = {}) {
  const props = {
    providers: [],
    configs: [],
    options: [],
    version: '0.1.0',
    settings: { startAtLogin: false, keepAwake: false },
    startAtLoginActual: null,
    startAtLoginError: null,
    keepAwakeActive: null,
    keepAwakeError: null,
    onCreateProvider: vi.fn(),
    onUpdateProvider: vi.fn(),
    onRemoveProvider: vi.fn(),
    onCreateConfig: vi.fn(),
    onUpdateConfig: vi.fn(),
    onDeleteConfig: vi.fn(),
    onProviderSaved: vi.fn(),
    onConfigSaved: vi.fn(),
    onChangeSettings: vi.fn(),
    onLoadDevices: vi.fn().mockResolvedValue([]),
    onRemoveDevice: vi.fn().mockResolvedValue(undefined),
    onExportDiagnostics: vi.fn().mockResolvedValue(null),
    onClose: vi.fn(),
    ...overrides,
  };
  render(
    <LanguageProvider>
      <SettingsDialog {...props} />
    </LanguageProvider>,
  );
  return props;
}

describe('SettingsDialog General', () => {
  it('saves both preferences through the switch controls', async () => {
    const user = userEvent.setup();
    const props = renderSettings();

    await user.click(screen.getByRole('button', { name: 'General' }));

    const start = screen.getByRole('switch', {
      name: 'Start elesdee when the system starts',
    });
    const awake = screen.getByRole('switch', {
      name: 'Keep the computer awake while agents work',
    });
    expect(start).toHaveAttribute('aria-checked', 'false');
    expect(awake).toHaveAttribute('aria-checked', 'false');

    await user.click(start);
    expect(props.onChangeSettings).toHaveBeenCalledWith({
      startAtLogin: true,
    });

    await user.click(awake);
    expect(props.onChangeSettings).toHaveBeenCalledWith({ keepAwake: true });
  });

  it('shows the platform refusal', async () => {
    renderSettings({
      settings: { startAtLogin: true, keepAwake: true },
      startAtLoginError: 'The login item was refused.',
    });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'General' }));

    expect(screen.getByRole('alert')).toHaveTextContent(
      'This computer did not accept the change: The login item was refused.',
    );
  });

  it('shows when the platform reports a different state', async () => {
    renderSettings({
      settings: { startAtLogin: false, keepAwake: false },
      startAtLoginActual: true,
    });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'General' }));

    expect(
      screen.getByText('This computer reports a different start-at-login state.'),
    ).toBeVisible();
  });
});

