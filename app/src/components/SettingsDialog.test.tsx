import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { LanguageProvider } from '../i18n/LanguageProvider';
import type { PairedDevice } from '../api/types';
import { SettingsDialog } from './SettingsDialog';

const device: PairedDevice = {
  endpointId: 'phone-1',
  name: 'Pixel',
  platform: 'Android',
  pairedAt: '2026-11-18T09:00:00Z',
  lastSeen: '2026-11-19T10:00:00Z',
  connected: true,
};

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
    onStartProviderSignIn: vi.fn(),
    onGetProviderSignIn: vi.fn(),
    onSubmitProviderSignInCode: vi.fn(),
    onCancelProviderSignIn: vi.fn(),
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

describe('SettingsDialog Devices', () => {
  it('lists a paired phone and removes it after confirmation', async () => {
    const user = userEvent.setup();
    const props = renderSettings({
      onLoadDevices: vi.fn().mockResolvedValue([device]),
    });

    await user.click(screen.getByRole('button', { name: 'Devices' }));

    expect(await screen.findByText('Pixel')).toBeVisible();
    expect(screen.getByText('Connected')).toBeVisible();
    expect(screen.getByText(/Last seen/)).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Remove Pixel' }));
    const confirm = screen.getByRole('dialog', { name: 'Remove phone' });
    expect(within(confirm).getByText(/Pixel/)).toBeVisible();
    await user.click(
      within(confirm).getByRole('button', { name: 'Remove phone' }),
    );

    await waitFor(() =>
      expect(props.onRemoveDevice).toHaveBeenCalledWith('phone-1'),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Remove phone' }),
      ).not.toBeInTheDocument(),
    );
  });

  it('keeps the phone when removal is cancelled', async () => {
    const user = userEvent.setup();
    const props = renderSettings({
      onLoadDevices: vi.fn().mockResolvedValue([device]),
    });

    await user.click(screen.getByRole('button', { name: 'Devices' }));
    expect(await screen.findByText('Pixel')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Remove Pixel' }));
    const confirm = screen.getByRole('dialog', { name: 'Remove phone' });
    await user.click(within(confirm).getByRole('button', { name: 'Cancel' }));

    expect(props.onRemoveDevice).not.toHaveBeenCalled();
    expect(screen.getByText('Pixel')).toBeVisible();
  });

  it('explains how to pair when no phone exists', async () => {
    const user = userEvent.setup();
    renderSettings();

    await user.click(screen.getByRole('button', { name: 'Devices' }));

    expect(await screen.findByText('No phone is paired.')).toBeVisible();
    expect(
      screen.getByText(/scan the pairing code/),
    ).toBeVisible();
  });
});

describe('SettingsDialog Diagnostics', () => {
  it('exports a bundle and shows the saved path', async () => {
    const user = userEvent.setup();
    const props = renderSettings({
      onExportDiagnostics: vi
        .fn()
        .mockResolvedValue('/tmp/elesdee-diagnostics.json'),
    });

    await user.click(screen.getByRole('button', { name: 'Diagnostics' }));
    expect(
      screen.getByText(/Keys, pairing secrets, tickets/),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Export diagnostics' }),
    );

    expect(props.onExportDiagnostics).toHaveBeenCalledOnce();
    expect(
      await screen.findByText('Saved to /tmp/elesdee-diagnostics.json'),
    ).toBeVisible();
  });

  it('stays quiet when the folder picker is cancelled', async () => {
    const user = userEvent.setup();
    const props = renderSettings();
    await user.click(screen.getByRole('button', { name: 'Diagnostics' }));
    await user.click(
      screen.getByRole('button', { name: 'Export diagnostics' }),
    );

    expect(props.onExportDiagnostics).toHaveBeenCalledOnce();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
});
