import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import { isTauri, pickFolder } from '../api/shell';
import type { Project } from '../api/types';
import { AddProjectDialog } from './AddProjectDialog';

vi.mock('../api/shell', () => ({
  isTauri: vi.fn(() => true),
  pickFolder: vi.fn(),
}));

const createdProject: Project = {
  id: 'p1',
  name: 'core',
  folder: '/dev/core',
};

function renderDialog(onCreate = vi.fn()) {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <AddProjectDialog
      onCreate={onCreate}
      onCreated={onCreated}
      onClose={onClose}
    />,
  );
  return { onCreate, onCreated, onClose };
}

async function fillForm(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
  folder: string,
) {
  await user.type(screen.getByLabelText('Name'), name);
  await user.type(screen.getByLabelText('Folder'), folder);
}

describe('AddProjectDialog', () => {
  it('focuses the name and validates it before calling the server', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    expect(screen.getByLabelText('Name')).toHaveFocus();

    await user.click(screen.getByRole('button', { name: 'Add project' }));

    expect(screen.getByText('Enter a name.')).toBeInTheDocument();
    expect(screen.getByText('Enter a folder.')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveFocus();
    expect(onCreate).not.toHaveBeenCalled();
  });

  it('adds a project and returns it', async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(createdProject);
    const { onCreated } = renderDialog(onCreate);

    await fillForm(user, 'core', '/dev/core');
    await user.click(screen.getByRole('button', { name: 'Add project' }));

    await waitFor(() =>
      expect(onCreated).toHaveBeenCalledWith(createdProject),
    );
    expect(onCreate).toHaveBeenCalledWith({
      name: 'core',
      folder: '/dev/core',
    });
  });

  it('fills the folder from the desktop picker', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(pickFolder).mockResolvedValue('/picked/folder');
    const user = userEvent.setup();
    renderDialog();

    await user.click(screen.getByRole('button', { name: 'Browse…' }));

    expect(pickFolder).toHaveBeenCalledOnce();
    expect(screen.getByLabelText('Folder')).toHaveValue('/picked/folder');
  });

  it('shows a duplicate project error inline', async () => {
    const user = userEvent.setup();
    const onCreate = vi
      .fn()
      .mockRejectedValue(
        new ApiError(409, 'A project with that folder already exists.'),
      );
    const { onCreated } = renderDialog(onCreate);

    await fillForm(user, 'core', '/dev/core');
    await user.click(screen.getByRole('button', { name: 'Add project' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'A project with that folder already exists.',
    );
    expect(onCreated).not.toHaveBeenCalled();
  });
});
