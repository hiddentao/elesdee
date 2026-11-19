import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type { Account, Project } from '../api/types';
import { makeSession } from '../test/fixtures';
import { NewSessionDialog } from './NewSessionDialog';

const projects: Project[] = [
  { id: 'p1', name: 'core', folder: '/dev/core' },
  { id: 'p2', name: 'web', folder: '/dev/web' },
];

const accounts: Account[] = [
  {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'Anthropic',
    auth: 'subscription',
    sample: true,
    rows: [{ label: '5-hour', used: 58 }],
    foot: 'Resets in 2h.',
    server: null,
    model: null,
  },
  {
    id: 'a2',
    name: 'Personal',
    plan: 'Plus',
    client: 'Codex',
    provider: 'OpenAI',
    auth: 'subscription',
    sample: true,
    rows: [],
    foot: '',
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

const createdSession = makeSession({
  name: 'Fix login',
  short: 'fix-login',
  branch: 'elesdee/core/fix-login',
  folder: '/dev/core',
  preview: '',
});

function renderDialog(
  onCreate = vi.fn(),
  initialProjectId: string | null = null,
) {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <NewSessionDialog
      projects={projects}
      accounts={accounts}
      initialProjectId={initialProjectId}
      onCreate={onCreate}
      onCreated={onCreated}
      onClose={onClose}
    />,
  );
  return { onCreate, onCreated, onClose };
}

async function chooseProject(user: ReturnType<typeof userEvent.setup>, name: string) {
  const input = screen.getByLabelText('Project');
  await user.click(input);
  await user.type(input, name);
  await user.click(screen.getByRole('option', { name: new RegExp(name) }));
}

describe('NewSessionDialog', () => {
  it('focuses the first field and shows the server error inline', async () => {
    const user = userEvent.setup();
    const onCreate = vi
      .fn()
      .mockRejectedValue(new ApiError(400, 'The project folder does not exist.'));
    const { onCreated } = renderDialog(onCreate);

    expect(screen.getByLabelText('Project')).toHaveFocus();

    await chooseProject(user, 'core');
    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));

    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);
    expect(
      screen.getByRole('option', { name: /42% left/ }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('option', { name: /Work/ }));

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The project folder does not exist.',
    );
    expect(onCreated).not.toHaveBeenCalled();
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      accountId: 'a1',
      folder: '/dev/core',
    });
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('client');
  });

  it('shows the repository folder for an agent and sends it', async () => {
    const user = userEvent.setup();
    const onCreate = vi.fn().mockResolvedValue(createdSession);
    const { onCreated } = renderDialog(onCreate, 'p1');

    expect(screen.getByLabelText('Repository folder')).toHaveValue('/dev/core');
    expect(
      screen.getByText('The folder must be inside a Git repository.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: 'Claude Code' }));
    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);
    await user.click(screen.getByRole('option', { name: /Work/ }));

    expect(screen.getByText('42% left')).toBeInTheDocument();
    expect(screen.getByText('Sample')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(createdSession));
    expect(onCreate.mock.calls[0][0]).toMatchObject({
      projectId: 'p1',
      kind: 'agent',
      accountId: 'a1',
      folder: '/dev/core',
    });
    expect(onCreate.mock.calls[0][0]).not.toHaveProperty('client');
  });

  it('validates that a project is chosen before it calls the server', async () => {
    const user = userEvent.setup();
    const { onCreate, onCreated } = renderDialog();

    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(screen.getByText('Choose a project from the list.')).toBeInTheDocument();
    expect(screen.getByLabelText('Project')).toHaveFocus();
    expect(onCreate).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('offers Antigravity and its account with no usage data', async () => {
    const user = userEvent.setup();
    renderDialog();

    await user.click(screen.getByRole('radio', { name: 'Antigravity' }));

    const accountInput = screen.getByLabelText('Account');
    await user.click(accountInput);

    const option = screen.getByRole('option', { name: /Google work/ });
    expect(option).toHaveTextContent('No usage data');
    await user.click(option);

    expect(screen.getByText('No usage data')).toBeInTheDocument();
    expect(screen.queryByText('Sample')).not.toBeInTheDocument();
  });

  it('labels the folder for the chosen session kind', async () => {
    const user = userEvent.setup();
    renderDialog();

    expect(screen.getByLabelText('Repository folder')).toBeInTheDocument();
    expect(
      screen.getByText('The folder must be inside a Git repository.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: 'Terminal' }));

    expect(screen.getByLabelText('Folder')).toBeInTheDocument();
    expect(
      screen.queryByText('The folder must be inside a Git repository.'),
    ).not.toBeInTheDocument();
  });

  it('requires a folder for a terminal session', async () => {
    const user = userEvent.setup();
    const { onCreate } = renderDialog();

    await chooseProject(user, 'web');
    await user.click(screen.getByRole('radio', { name: 'Terminal' }));

    const folder = screen.getByLabelText('Folder');
    expect(folder).toHaveValue('/dev/web');

    await user.clear(folder);
    await user.click(screen.getByRole('button', { name: 'Start session' }));

    expect(screen.getByText('Enter a folder.')).toBeInTheDocument();
    expect(onCreate).not.toHaveBeenCalled();
  });
});
