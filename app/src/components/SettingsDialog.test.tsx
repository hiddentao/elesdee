import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Account } from '../api/types';
import { SettingsDialog } from './SettingsDialog';

const accounts: Account[] = [
  {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'anthropic',
    auth: 'subscription',
    sample: true,
    rows: [{ label: '5-hour', used: 58 }],
    foot: 'Resets in 2h.',
    server: null,
    model: null,
  },
  {
    id: 'google-1',
    name: 'Google work',
    plan: 'Google AI',
    client: 'Antigravity',
    provider: 'google',
    auth: 'key',
    sample: false,
    rows: [],
    foot: 'No official usage source.',
    server: null,
    model: 'gemini-3.8-flash-medium',
  },
];

function renderSettings(onClose = vi.fn()) {
  render(
    <SettingsDialog
      accounts={accounts}
      version="0.1.0"
      onCreateAccount={vi.fn()}
      onAccountCreated={vi.fn()}
      onClose={onClose}
    />,
  );
  return { onClose };
}

describe('SettingsDialog', () => {
  it('lists providers with their client, plan and usage', () => {
    renderSettings();
    const dialog = screen.getByRole('dialog', { name: 'Settings' });

    expect(
      within(dialog).getByRole('heading', { name: 'Settings' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('button', { name: 'Providers' }),
    ).toHaveAttribute('aria-current', 'page');

    expect(within(dialog).getByText('Work')).toBeInTheDocument();
    expect(within(dialog).getByText('Sample')).toBeInTheDocument();
    expect(within(dialog).getByText('Claude Code · Max')).toBeInTheDocument();
    expect(within(dialog).getByText('42% left')).toBeInTheDocument();

    const googleRow = within(dialog).getByText('Google work').closest('li');
    expect(googleRow).not.toBeNull();
    expect(within(googleRow as HTMLElement).queryByText('Sample')).not.toBeInTheDocument();
    expect(within(dialog).getByText('Antigravity · Google AI')).toBeInTheDocument();
    expect(within(dialog).getByText('No usage data')).toBeInTheDocument();
  });

  it('shows the app name and version in About', async () => {
    const user = userEvent.setup();
    renderSettings();
    const dialog = screen.getByRole('dialog', { name: 'Settings' });

    await user.click(within(dialog).getByRole('button', { name: 'About' }));

    expect(
      within(dialog).getByRole('heading', { name: 'About' }),
    ).toBeInTheDocument();
    expect(within(dialog).getByText('elesdee')).toBeInTheDocument();
    expect(within(dialog).getByText('Version 0.1.0')).toBeInTheDocument();
  });

  it('opens the add-account dialog and returns focus', async () => {
    const user = userEvent.setup();
    renderSettings();
    const settings = screen.getByRole('dialog', { name: 'Settings' });
    const addButton = within(settings).getByRole('button', {
      name: 'Add account',
    });

    await user.click(addButton);

    const addDialog = screen.getByRole('dialog', { name: 'Add account' });
    expect(addDialog).toBeInTheDocument();

    await user.click(within(addDialog).getByRole('button', { name: 'Cancel' }));

    expect(
      screen.queryByRole('dialog', { name: 'Add account' }),
    ).not.toBeInTheDocument();
    expect(addButton).toHaveFocus();
  });

  it('closes from the header button', async () => {
    const user = userEvent.setup();
    const { onClose } = renderSettings();

    await user.click(
      screen.getByRole('button', { name: 'Close settings' }),
    );

    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
