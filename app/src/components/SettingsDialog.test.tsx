import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ComponentProps } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, listProviders } from '../api/client';
import type { Account } from '../api/types';
import { SettingsDialog } from './SettingsDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return { ...actual, listProviders: vi.fn() };
});

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

type SettingsProps = ComponentProps<typeof SettingsDialog>;

function renderSettings(overrides: Partial<SettingsProps> = {}) {
  const props: SettingsProps = {
    accounts,
    version: '0.1.0',
    onCreateAccount: vi.fn(),
    onAccountCreated: vi.fn(),
    onUpdateAccount: vi.fn().mockResolvedValue(accounts[0]),
    onRemoveAccount: vi.fn().mockResolvedValue({
      clearedProjectDefaults: [],
      removedPresets: 0,
    }),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<SettingsDialog {...props} />);
  return props;
}

describe('SettingsDialog', () => {
  beforeEach(() => {
    vi.mocked(listProviders).mockResolvedValue([
      {
        id: 'anthropic',
        label: 'Anthropic',
        client: 'Claude Code',
        auth: ['subscription', 'key'],
        defaultModel: null,
        custom: false,
      },
    ]);
  });

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

    expect(
      within(dialog).getByRole('button', { name: 'Edit account Work' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('button', { name: 'Remove account Work' }),
    ).toBeInTheDocument();
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

  it('opens the edit dialog with the account fields', async () => {
    const user = userEvent.setup();
    renderSettings();
    const settings = screen.getByRole('dialog', { name: 'Settings' });

    await user.click(
      within(settings).getByRole('button', { name: 'Edit account Work' }),
    );

    const editDialog = screen.getByRole('dialog', { name: 'Edit account' });
    expect(editDialog).toBeInTheDocument();
    expect(within(editDialog).getByLabelText('Account name')).toHaveValue(
      'Work',
    );
  });

  it('removes an account only after the confirmation', async () => {
    const user = userEvent.setup();
    const { onRemoveAccount } = renderSettings();
    const settings = screen.getByRole('dialog', { name: 'Settings' });

    await user.click(
      within(settings).getByRole('button', {
        name: 'Remove account Google work',
      }),
    );

    const confirm = screen.getByRole('dialog', { name: 'Remove account' });
    expect(onRemoveAccount).not.toHaveBeenCalled();
    expect(
      within(confirm).getByText(/Google work/),
    ).toBeInTheDocument();

    await user.click(
      within(confirm).getByRole('button', { name: 'Remove account' }),
    );

    await waitFor(() =>
      expect(onRemoveAccount).toHaveBeenCalledWith('google-1'),
    );
    expect(
      screen.queryByRole('dialog', { name: 'Remove account' }),
    ).not.toBeInTheDocument();
  });

  it('keeps the remove dialog open and shows a refusal', async () => {
    const user = userEvent.setup();
    const onRemoveAccount = vi
      .fn()
      .mockRejectedValue(
        new ApiError(409, 'A session is using this account.'),
      );
    renderSettings({ onRemoveAccount });
    const settings = screen.getByRole('dialog', { name: 'Settings' });

    await user.click(
      within(settings).getByRole('button', { name: 'Remove account Work' }),
    );
    const confirm = screen.getByRole('dialog', { name: 'Remove account' });
    await user.click(
      within(confirm).getByRole('button', { name: 'Remove account' }),
    );

    expect(await within(confirm).findByRole('alert')).toHaveTextContent(
      'A session is using this account.',
    );
    expect(
      screen.getByRole('dialog', { name: 'Remove account' }),
    ).toBeInTheDocument();
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
