import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError, listAccounts, listBranches } from '../api/client';
import type { Account, Project } from '../api/types';
import { ProjectSettingsDialog } from './ProjectSettingsDialog';

vi.mock('../api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../api/client')>();
  return {
    ...actual,
    listAccounts: vi.fn(),
    listBranches: vi.fn(),
  };
});

const account: Account = {
  id: 'a1',
  name: 'Work',
  plan: 'Max',
  client: 'Claude Code',
  provider: 'Anthropic',
  auth: 'subscription',
  sample: true,
  rows: [],
  foot: '',
  server: null,
  model: null,
};

const project: Project = {
  id: 'p1',
  name: 'core',
  folder: '/dev/core',
  defaultAccountId: 'a1',
  defaultSourceBranch: 'feature',
};

function renderDialog(
  onSave = vi.fn().mockResolvedValue(undefined),
  target: Project = project,
) {
  const onClose = vi.fn();
  render(
    <ProjectSettingsDialog
      project={target}
      accounts={[account]}
      onSave={onSave}
      onClose={onClose}
    />,
  );
  return { onSave, onClose };
}

beforeEach(() => {
  vi.mocked(listAccounts).mockResolvedValue([account]);
  vi.mocked(listBranches).mockResolvedValue([
    { name: 'main', current: true },
    { name: 'feature', current: false },
  ]);
});

describe('ProjectSettingsDialog', () => {
  it('shows the stored defaults and saves them', async () => {
    const user = userEvent.setup();
    const { onSave, onClose } = renderDialog();

    expect(screen.getByLabelText('Default account')).toHaveValue('Work');
    expect(screen.getByLabelText('Default source branch')).toHaveValue(
      'feature',
    );

    await user.click(screen.getByRole('button', { name: 'Save defaults' }));

    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        defaultAccountId: 'a1',
        defaultSourceBranch: 'feature',
      }),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it('clears both defaults', async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog();

    await user.click(
      screen.getByRole('button', { name: 'Clear default account' }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Clear default branch' }),
    );

    expect(screen.getByLabelText('Default account')).toHaveValue('');
    expect(screen.getByLabelText('Default source branch')).toHaveValue('');

    await user.click(screen.getByRole('button', { name: 'Save defaults' }));

    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        defaultAccountId: null,
        defaultSourceBranch: null,
      }),
    );
  });

  it('loads the account and branch lists asynchronously', async () => {
    const user = userEvent.setup();
    const { onSave } = renderDialog(vi.fn().mockResolvedValue(undefined), {
      id: 'p1',
      name: 'core',
      folder: '/dev/core',
    });

    expect(screen.getByLabelText('Default account')).toHaveValue('');
    expect(screen.getByLabelText('Default source branch')).toHaveValue('');

    const accountInput = screen.getByLabelText('Default account');
    await user.click(accountInput);
    expect(
      await screen.findByRole('option', { name: /Work/ }),
    ).toBeInTheDocument();
    expect(listAccounts).toHaveBeenCalled();
    await user.click(screen.getByRole('option', { name: /Work/ }));

    const branchInput = screen.getByLabelText('Default source branch');
    await user.click(branchInput);
    expect(
      await screen.findByRole('option', { name: /^feature/ }),
    ).toBeInTheDocument();
    expect(listBranches).toHaveBeenCalledWith(
      '/dev/core',
      '',
      expect.anything(),
    );
    await user.click(screen.getByRole('option', { name: /^feature/ }));

    await user.click(screen.getByRole('button', { name: 'Save defaults' }));

    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        defaultAccountId: 'a1',
        defaultSourceBranch: 'feature',
      }),
    );
  });

  it('shows a server error and keeps the dialog open', async () => {
    const user = userEvent.setup();
    const onSave = vi
      .fn()
      .mockRejectedValue(
        new ApiError(422, 'The source branch does not exist.'),
      );
    const { onClose } = renderDialog(onSave);

    await user.click(screen.getByRole('button', { name: 'Save defaults' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The source branch does not exist.',
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
