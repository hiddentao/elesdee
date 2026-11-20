import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type { PairedDevice } from '../api/types';
import { LanguageProvider } from '../i18n/LanguageProvider';
import { PairingDialog } from './PairingDialog';

const ticket = 'elesdee-pair-secret-ticket';

const paired: PairedDevice = {
  endpointId: 'phone-1',
  name: 'Pixel',
  platform: 'android',
  pairedAt: '2026-11-20T09:00:00Z',
  lastSeen: '2026-11-20T09:00:00Z',
  connected: false,
};

function renderPairing(
  overrides: Partial<Parameters<typeof PairingDialog>[0]> = {},
) {
  const props = {
    onStart: vi.fn().mockResolvedValue({
      expiresAt: Math.floor(Date.now() / 1000) + 120,
      relayReady: true,
      ticket,
    }),
    onStatus: vi.fn().mockResolvedValue({ state: 'waiting' }),
    onApprove: vi.fn().mockResolvedValue(paired),
    onDeny: vi.fn().mockResolvedValue(undefined),
    onCancel: vi.fn().mockResolvedValue(undefined),
    qrUrl: (cacheBust: number) => `/api/pairing/qr?v=${cacheBust}`,
    onPaired: vi.fn(),
    onClose: vi.fn(),
    pollMs: 10,
    ...overrides,
  };
  render(
    <LanguageProvider>
      <PairingDialog {...props} />
    </LanguageProvider>,
  );
  return props;
}

describe('PairingDialog', () => {
  it('waits with the QR code from the core and keeps the ticket out of the DOM', async () => {
    renderPairing();

    const qr = await screen.findByRole('img', { name: 'Pairing QR code' });
    expect(qr.getAttribute('src')).toMatch(/^\/api\/pairing\/qr\?v=\d+$/);
    expect(
      screen.getByText('Scan this code with the phone app.'),
    ).toBeVisible();
    expect(screen.queryByLabelText('Pairing ticket')).not.toBeInTheDocument();
  });

  it('shows the ticket when the QR image fails', async () => {
    renderPairing();

    const qr = await screen.findByRole('img', { name: 'Pairing QR code' });
    fireEvent.error(qr);

    const fallback = screen.getByLabelText('Pairing ticket');
    expect(fallback).toHaveValue(ticket);
    expect(
      screen.getByText(
        'If the phone cannot scan the code, paste this ticket in the phone app.',
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole('img', { name: 'Pairing QR code' }),
    ).not.toBeInTheDocument();
  });

  it('shows the phone name and code, then approves the phone', async () => {
    const user = userEvent.setup();
    const props = renderPairing({
      onStatus: vi.fn().mockResolvedValue({
        state: 'confirm',
        phoneName: 'Pixel',
        code: '428913',
        expiresAt: Math.floor(Date.now() / 1000) + 60,
      }),
    });

    expect(
      await screen.findByText('The phone Pixel wants to pair.'),
    ).toBeVisible();
    expect(screen.getByText('428913')).toBeVisible();
    expect(
      screen.getByLabelText('Confirmation code 4 2 8 9 1 3'),
    ).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Approve' }));

    expect(props.onApprove).toHaveBeenCalledOnce();
    await waitFor(() => expect(props.onPaired).toHaveBeenCalledWith(paired));
  });

  it('drops the ticket from the page once the phone connects', async () => {
    const user = userEvent.setup();
    const onStatus = vi.fn().mockResolvedValue({ state: 'waiting' });
    renderPairing({ onStatus });
    const qr = await screen.findByRole('img', { name: 'Pairing QR code' });
    fireEvent.error(qr);
    expect(screen.getByLabelText('Pairing ticket')).toBeVisible();

    onStatus.mockResolvedValue({
      state: 'confirm',
      phoneName: 'Pixel',
      code: '428913',
    });

    await waitFor(() =>
      expect(screen.getByText('428913')).toBeVisible(),
    );
    expect(screen.queryByLabelText('Pairing ticket')).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue(ticket)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Deny' }));
  });

  it('denies the phone and stops the flow', async () => {
    const user = userEvent.setup();
    const props = renderPairing({
      onStatus: vi.fn().mockResolvedValue({
        state: 'confirm',
        phoneName: 'Pixel',
        code: '428913',
      }),
    });

    await screen.findByText('The phone Pixel wants to pair.');
    await user.click(screen.getByRole('button', { name: 'Deny' }));

    expect(props.onDeny).toHaveBeenCalledOnce();
    expect(
      await screen.findByText('You denied this phone. It is not paired.'),
    ).toBeVisible();
    expect(props.onPaired).not.toHaveBeenCalled();
    expect(
      screen.getByRole('button', { name: 'Pair again' }),
    ).toBeVisible();
  });

  it('shows the expiry and pairs again', async () => {
    const user = userEvent.setup();
    const props = renderPairing({
      onStatus: vi.fn().mockResolvedValue({ state: 'expired' }),
    });

    expect(
      await screen.findByText('The pairing code expired. Pair again.'),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Pair again' }));

    await waitFor(() => expect(props.onStart).toHaveBeenCalledTimes(2));
  });

  it('cancels the ticket and closes', async () => {
    const user = userEvent.setup();
    const props = renderPairing();

    await screen.findByRole('img', { name: 'Pairing QR code' });
    await user.click(screen.getByRole('button', { name: 'Cancel pairing' }));

    expect(props.onCancel).toHaveBeenCalledOnce();
    await waitFor(() => expect(props.onClose).toHaveBeenCalledOnce());
  });

  it('explains when pairing is unavailable', async () => {
    renderPairing({
      onStart: vi
        .fn()
        .mockRejectedValue(new ApiError(503, 'The link is not running.')),
    });

    expect(
      await screen.findByText(
        'Pairing is unavailable. The phone link did not start. Restart elesdee and try again.',
      ),
    ).toBeVisible();
  });
});
