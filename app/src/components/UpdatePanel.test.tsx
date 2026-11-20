import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { UpdateSnapshot } from '../api/updater';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { UpdatePanel } from './UpdatePanel';

function snapshot(overrides: Partial<UpdateSnapshot>): UpdateSnapshot {
  return {
    state: 'idle',
    currentVersion: '0.1.0',
    version: null,
    progress: null,
    error: null,
    ...overrides,
  };
}

function renderPanel(update: UpdateSnapshot | null) {
  const onCheck = vi.fn();
  const onInstall = vi.fn();
  render(
    <LanguageProvider>
      <UpdatePanel update={update} onCheck={onCheck} onInstall={onInstall} />
    </LanguageProvider>,
  );
  return { onCheck, onInstall };
}

describe('UpdatePanel', () => {
  it('renders nothing outside the desktop shell', () => {
    renderPanel(null);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('offers a check when idle', async () => {
    const user = userEvent.setup();
    const { onCheck } = renderPanel(snapshot({ state: 'idle' }));
    expect(
      screen.getByText('Updates are checked at launch and every few hours.'),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it('shows the checking state without a control', () => {
    renderPanel(snapshot({ state: 'checking' }));
    expect(screen.getByText('Checking for updates…')).toBeVisible();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('reports the latest version and allows another check', async () => {
    const user = userEvent.setup();
    const { onCheck } = renderPanel(snapshot({ state: 'upToDate' }));
    expect(screen.getByText('You have the latest version.')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(onCheck).toHaveBeenCalledTimes(1);
  });

  it('shows download progress for a known version', () => {
    renderPanel(
      snapshot({
        state: 'downloading',
        version: '0.2.0',
        progress: 42,
      }),
    );
    expect(screen.getByText('Downloading version 0.2.0…')).toBeVisible();
    const progress = screen.getByRole('progressbar', {
      name: 'Update download progress',
    });
    expect(progress).toHaveAttribute('aria-valuenow', '42');
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('shows a download without a known version', () => {
    renderPanel(snapshot({ state: 'downloading' }));
    expect(screen.getByText('Downloading the update…')).toBeVisible();
  });

  it('offers the install when the update is ready', async () => {
    const user = userEvent.setup();
    const { onInstall } = renderPanel(
      snapshot({ state: 'ready', version: '0.2.0' }),
    );
    expect(
      screen.getByText('Version 0.2.0 is ready to install.'),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Install and restart' }),
    );
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it('explains a deferred update and still allows it now', async () => {
    const user = userEvent.setup();
    const { onInstall } = renderPanel(
      snapshot({ state: 'deferred', version: '0.2.0' }),
    );
    expect(
      screen.getByText(
        'Version 0.2.0 installs when all sessions are closed.',
      ),
    ).toBeVisible();
    await user.click(
      screen.getByRole('button', { name: 'Install and restart' }),
    );
    expect(onInstall).toHaveBeenCalledTimes(1);
  });

  it('shows the installing state without a control', () => {
    renderPanel(snapshot({ state: 'installing', version: '0.2.0' }));
    expect(screen.getByText('Installing version 0.2.0…')).toBeVisible();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('says when this build has no updates', () => {
    renderPanel(
      snapshot({ state: 'unavailable', error: 'notConfigured' }),
    );
    expect(
      screen.getByText('Updates are unavailable in this build.'),
    ).toBeVisible();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it.each([
    [
      'network',
      'The update check could not reach GitHub. Check the connection and try again.',
    ],
    [
      'signature',
      'The update did not pass its signature check. It was not installed.',
    ],
    ['checkFailed', 'The update check failed. Try again.'],
    ['downloadFailed', 'The update could not be downloaded. Try again.'],
    [
      'installFailed',
      'The update could not be installed. The app is still running.',
    ],
  ] as const)('shows the %s error and offers a retry', async (error, text) => {
    const user = userEvent.setup();
    const { onCheck } = renderPanel(snapshot({ state: 'error', error }));
    expect(screen.getByText(text)).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Try again' }));
    expect(onCheck).toHaveBeenCalledTimes(1);
  });
});
