import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import type {
  GitHubDevice,
  GitHubRepo,
  GitHubStatus,
  Project,
} from '../api/types';
import {
  AddProjectDialog,
  type GitHubActions,
} from './AddProjectDialog';

const disconnected: GitHubStatus = {
  connected: false,
  login: null,
  name: null,
  avatarUrl: null,
  defaultCloneFolder: '/dev/projects',
};

const connected: GitHubStatus = {
  connected: true,
  login: 'octocat',
  name: 'The Octocat',
  avatarUrl: 'https://example.com/avatar.png',
  defaultCloneFolder: '/dev/projects',
};

const waiting: GitHubDevice = {
  id: 'flow-1',
  state: 'waiting',
  userCode: 'ABCD-1234',
  verificationUri: 'https://github.com/login/device',
  interval: 1,
  expiresIn: 900,
  login: null,
  reason: null,
};

const success: GitHubDevice = {
  ...waiting,
  state: 'success',
  login: 'octocat',
};

const repos: GitHubRepo[] = [
  {
    id: 1,
    fullName: 'acme/web',
    name: 'web',
    owner: 'acme',
    private: false,
    cloneUrl: 'https://github.com/acme/web.git',
    htmlUrl: 'https://github.com/acme/web',
    description: 'The web app',
    updatedAt: null,
    added: true,
  },
  {
    id: 2,
    fullName: 'acme/api',
    name: 'api',
    owner: 'acme',
    private: true,
    cloneUrl: 'https://github.com/acme/api.git',
    htmlUrl: 'https://github.com/acme/api',
    description: '',
    updatedAt: null,
    added: false,
  },
];

const createdProject: Project = {
  id: 'web',
  name: 'web',
  folder: '/dev/projects/web',
};

function actions(overrides: Partial<GitHubActions> = {}): GitHubActions {
  return {
    status: vi.fn(async () => disconnected),
    startSignIn: vi.fn(async () => waiting),
    getSignIn: vi.fn(async () => success),
    cancelSignIn: vi.fn(async () => undefined),
    signOut: vi.fn(async () => undefined),
    listRepos: vi.fn(async () => repos),
    clone: vi.fn(async () => createdProject),
    ...overrides,
  };
}

function renderDialog(github: GitHubActions) {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(
    <AddProjectDialog
      onCreate={vi.fn()}
      onCreated={onCreated}
      onClose={onClose}
      github={github}
    />,
  );
  return { onCreated, onClose };
}

async function openGitHub(
  user: ReturnType<typeof userEvent.setup>,
): Promise<void> {
  await user.click(screen.getByRole('radio', { name: 'From GitHub' }));
}

describe('AddProjectDialog GitHub source', () => {
  it('offers the device code and connects when the sign-in is approved', async () => {
    const user = userEvent.setup();
    const github = actions();
    renderDialog(github);

    await openGitHub(user);
    await user.click(
      await screen.findByRole('button', { name: 'Connect GitHub' }),
    );

    expect(await screen.findByText('ABCD-1234')).toBeInTheDocument();
    expect(await screen.findByText('Open GitHub')).toBeInTheDocument();

    expect(
      await screen.findByText('Signed in as @octocat', {}, { timeout: 4000 }),
    ).toBeInTheDocument();
    expect(github.getSignIn).toHaveBeenCalledWith('flow-1');
  });

  it('shows a loading state while the repositories load', async () => {
    const user = userEvent.setup();
    let finish: (items: GitHubRepo[]) => void = () => undefined;
    const pending = new Promise<GitHubRepo[]>((resolve) => {
      finish = resolve;
    });
    renderDialog(
      actions({
        status: vi.fn(async () => connected),
        listRepos: vi.fn(() => pending),
      }),
    );

    await openGitHub(user);
    await user.click(await screen.findByLabelText('Repository'));

    expect(await screen.findByText('Loading…')).toBeInTheDocument();
    finish(repos);
    expect(await screen.findByText('acme/web')).toBeInTheDocument();
  });

  it('reports a refused sign-in', async () => {
    const user = userEvent.setup();
    const github = actions({
      startSignIn: vi.fn(async () => ({
        ...waiting,
        state: 'failed' as const,
        reason: 'denied' as const,
      })),
    });
    renderDialog(github);

    await openGitHub(user);
    await user.click(
      await screen.findByRole('button', { name: 'Connect GitHub' }),
    );

    expect(
      await screen.findByText('The GitHub sign-in was not approved. Try again.'),
    ).toBeInTheDocument();
  });

  it('lists repositories with the clone address and the added mark', async () => {
    const user = userEvent.setup();
    const github = actions({
      status: vi.fn(async () => connected),
      listRepos: vi.fn(async () => repos),
    });
    renderDialog(github);

    await openGitHub(user);
    expect(
      await screen.findByText('Signed in as @octocat'),
    ).toBeInTheDocument();

    const picker = screen.getByLabelText('Repository');
    await user.click(picker);

    expect(await screen.findByText('acme/web')).toBeInTheDocument();
    expect(
      screen.getByText('Public · https://github.com/acme/web.git'),
    ).toBeInTheDocument();
    expect(screen.getByText('Added')).toBeInTheDocument();
    expect(
      screen.getByText('Private · https://github.com/acme/api.git'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('option', { name: /acme\/web/ }));

    expect(screen.getByLabelText('Name')).toHaveValue('web');
    expect(
      screen.getByText('The repository is cloned into /dev/projects/web.'),
    ).toBeInTheDocument();
  });

  it('clones the chosen repository and returns the project', async () => {
    const user = userEvent.setup();
    const clone = vi.fn(async () => createdProject);
    const github = actions({
      status: vi.fn(async () => connected),
      listRepos: vi.fn(async () => repos),
      clone,
    });
    const { onCreated } = renderDialog(github);

    await openGitHub(user);
    await user.click(await screen.findByLabelText('Repository'));
    await user.click(
      await screen.findByRole('option', { name: /acme\/web/ }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Clone and add project' }),
    );

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(createdProject));
    expect(clone).toHaveBeenCalledWith({
      fullName: 'acme/web',
      name: 'web',
      parentFolder: '/dev/projects',
    });
  });

  it('shows a clone failure inline', async () => {
    const user = userEvent.setup();
    const github = actions({
      status: vi.fn(async () => connected),
      listRepos: vi.fn(async () => repos),
      clone: vi.fn(async () => {
        throw new ApiError(409, 'A folder with this name already exists.');
      }),
    });
    const { onCreated } = renderDialog(github);

    await openGitHub(user);
    await user.click(await screen.findByLabelText('Repository'));
    await user.click(
      await screen.findByRole('option', { name: /acme\/web/ }),
    );
    await user.click(
      screen.getByRole('button', { name: 'Clone and add project' }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'A folder with this name already exists.',
    );
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('shows the status failure and retries', async () => {
    const user = userEvent.setup();
    const status = vi
      .fn<() => Promise<GitHubStatus>>()
      .mockRejectedValueOnce(new ApiError(502, 'GitHub could not be reached.'))
      .mockResolvedValueOnce(disconnected);
    renderDialog(actions({ status }));

    await openGitHub(user);
    expect(
      await screen.findByText('The GitHub connection could not be checked.'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Retry' }));

    expect(
      await screen.findByRole('button', { name: 'Connect GitHub' }),
    ).toBeInTheDocument();
    expect(status).toHaveBeenCalledTimes(2);
  });

  it('disconnects and returns to the connect action', async () => {
    const user = userEvent.setup();
    const signOut = vi.fn(async () => undefined);
    renderDialog(
      actions({
        status: vi.fn(async () => connected),
        signOut,
      }),
    );

    await openGitHub(user);
    expect(
      await screen.findByText('Signed in as @octocat'),
    ).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Disconnect' }));

    expect(
      await screen.findByRole('button', { name: 'Connect GitHub' }),
    ).toBeInTheDocument();
    expect(signOut).toHaveBeenCalledOnce();
  });
});
