import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  activateBrowserTab,
  ApiError,
  browserBack,
  browserForward,
  browserReload,
  browserScreenshotUrl,
  closeBrowserTab,
  createBrowserTab,
  disableDemoFunnel,
  enableDemoFunnel,
  getBrowserState,
  listFiles,
  listSharedFiles,
  openBrowserUrl,
  readFile,
  readSharedFile,
  sessionChanges,
  sharedFileUrl,
  stopDemoServer,
  writeFile,
} from '../api/client';
import type {
  BrowserState,
  DemoServer,
  FileEntry,
  SharedFile,
} from '../api/types';
import { openUrl } from '../api/shell';
import { makeSession } from '../test/fixtures';
import { SessionSidePanel } from './SessionSidePanel';

vi.mock('../api/shell', () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

vi.mock('../api/client', () => ({
  ApiError: class ApiError extends Error {
    readonly status: number;

    constructor(status: number, message: string) {
      super(message);
      this.name = 'ApiError';
      this.status = status;
    }
  },
  activateBrowserTab: vi.fn(),
  browserBack: vi.fn(),
  browserForward: vi.fn(),
  browserReload: vi.fn(),
  browserScreenshotUrl: vi.fn(),
  closeBrowserTab: vi.fn(),
  createBrowserTab: vi.fn(),
  disableDemoFunnel: vi.fn(),
  enableDemoFunnel: vi.fn(),
  getBrowserState: vi.fn(),
  listFiles: vi.fn(),
  listSharedFiles: vi.fn(),
  openBrowserUrl: vi.fn(),
  readFile: vi.fn(),
  readSharedFile: vi.fn(),
  sessionChanges: vi.fn(),
  setupBrowser: vi.fn(),
  sharedFileUrl: vi.fn(),
  stopDemoServer: vi.fn(),
  writeFile: vi.fn(),
}));

vi.mock('./FileEditor', () => ({
  FileEditor: ({
    value,
    onChange,
    onSave,
  }: {
    value: string;
    onChange: (value: string) => void;
    onSave: () => void;
  }) => (
    <textarea
      aria-label="Code editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        if ((event.metaKey || event.ctrlKey) && event.key === 's') {
          event.preventDefault();
          onSave();
        }
      }}
    />
  ),
}));

const rootEntries: FileEntry[] = [
  { name: 'src', path: 'src', directory: true },
  { name: 'note.txt', path: 'note.txt', directory: false },
];

function renderPanel(
  tab: 'files' | 'changes' = 'files',
  onSelectTab = vi.fn(),
  onClose = vi.fn(),
  session = makeSession(),
) {
  return render(
    <SessionSidePanel
      session={session}
      tab={tab}
      onSelectTab={onSelectTab}
      onClose={onClose}
    />,
  );
}

beforeEach(() => {
  const browser: BrowserState = {
    sessionId: 's1',
    started: false,
    available: true,
    tabs: [],
    activeTabId: null,
    screenshotSeq: 0,
    error: null,
    setup: {
      phase: 'idle',
      downloaded: 0,
      total: null,
      version: null,
      source: null,
      error: null,
    },
  };
  vi.mocked(getBrowserState).mockResolvedValue(browser);
  vi.mocked(browserScreenshotUrl).mockImplementation(
    (id, tabId, seq) =>
      `/api/sessions/${id}/browser/screenshot?tab=${tabId}&v=${seq}`,
  );
  vi.mocked(openBrowserUrl).mockResolvedValue(browser);
  vi.mocked(activateBrowserTab).mockResolvedValue(browser);
  vi.mocked(closeBrowserTab).mockResolvedValue(browser);
  vi.mocked(createBrowserTab).mockResolvedValue(browser);
  vi.mocked(browserBack).mockResolvedValue(browser);
  vi.mocked(browserForward).mockResolvedValue(browser);
  vi.mocked(browserReload).mockResolvedValue(browser);
  vi.mocked(listFiles).mockImplementation((_id, path = '') =>
    Promise.resolve({
      path,
      entries:
        path === 'src'
          ? [{ name: 'lib.rs', path: 'src/lib.rs', directory: false }]
          : rootEntries,
      truncated: false,
    }),
  );
  vi.mocked(readFile).mockResolvedValue({
    path: 'note.txt',
    content: 'hello\n',
    size: 6,
  });
  vi.mocked(writeFile).mockResolvedValue({
    path: 'note.txt',
    content: 'changed',
    size: 7,
  });
  vi.mocked(listSharedFiles).mockResolvedValue([]);
  vi.mocked(sharedFileUrl).mockImplementation(
    (id, fileId) => `/api/sessions/${id}/shared/${fileId}`,
  );
  vi.mocked(sessionChanges).mockResolvedValue({
    git: true,
    base: 'abc123',
    files: [
      {
        path: 'src/redirect.ts',
        oldPath: null,
        status: 'modified',
        additions: 2,
        deletions: 1,
        binary: false,
        diff: '--- a/src/redirect.ts\n+++ b/src/redirect.ts\n@@ -1 +1 @@\n-old\n+new',
        diffTruncated: false,
      },
    ],
    truncated: false,
    error: null,
  });
});

describe('SessionSidePanel files', () => {
  it('lists folders, opens a file, marks edits and saves', async () => {
    const user = userEvent.setup();
    renderPanel();

    const note = await screen.findByRole('button', { name: /note\.txt/ });
    await user.click(note);

    const editor = await screen.findByLabelText('Code editor');
    expect(editor).toHaveValue('hello\n');
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();

    await user.clear(editor);
    await user.type(editor, 'changed');
    expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled();

    await user.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() =>
      expect(writeFile).toHaveBeenCalledWith('s1', 'note.txt', 'changed'),
    );
    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
  });

  it('loads a folder when it is expanded', async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(await screen.findByRole('button', { name: 'src' }));

    expect(await screen.findByRole('button', { name: /lib\.rs/ })).toBeInTheDocument();
    expect(listFiles).toHaveBeenCalledWith('s1', 'src');
  });

  it('shows a read error and keeps the tree usable', async () => {
    const user = userEvent.setup();
    vi.mocked(readFile).mockRejectedValueOnce(
      new Error('The file is not UTF-8 text.'),
    );
    renderPanel();

    await user.click(await screen.findByRole('button', { name: /note\.txt/ }));

    expect(
      await screen.findByRole('alert'),
    ).toHaveTextContent('The file is not UTF-8 text.');
    expect(screen.getByRole('button', { name: 'src' })).toBeInTheDocument();
  });

  it('lists a shared file and previews its text', async () => {
    const shared: SharedFile = {
      id: 'shared-1',
      sessionId: 's1',
      sessionName: 'Session',
      name: 'shared.txt',
      path: 'shared.txt',
      caption: 'From the agent',
      size: 12,
      contentType: 'text/plain; charset=utf-8',
      image: false,
      createdAt: '2026-01-01T00:00:00Z',
    };
    vi.mocked(listSharedFiles).mockResolvedValue([shared]);
    vi.mocked(readSharedFile).mockResolvedValue('shared text\n');
    const user = userEvent.setup();
    renderPanel();

    await user.click(
      await screen.findByRole('button', { name: /shared\.txt/ }),
    );

    expect(readSharedFile).toHaveBeenCalledWith('s1', 'shared-1');
    expect(await screen.findByLabelText('shared.txt')).toHaveTextContent(
      'shared text',
    );
    expect(
      screen.getByRole('link', { name: 'Download' }),
    ).toHaveAttribute('href', '/api/sessions/s1/shared/shared-1');
  });

  it('switches tabs and closes the panel', async () => {
    const user = userEvent.setup();
    const onSelectTab = vi.fn();
    const onClose = vi.fn();
    renderPanel('files', onSelectTab, onClose);

    await user.click(screen.getByRole('tab', { name: 'Changes' }));
    expect(onSelectTab).toHaveBeenCalledWith('changes');

    await user.click(screen.getByRole('tab', { name: 'Browser' }));
    expect(onSelectTab).toHaveBeenCalledWith('browser');

    await user.click(screen.getByRole('button', { name: 'Close side panel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('SessionSidePanel demo servers', () => {
  const preview: DemoServer = {
    id: 'demo-1',
    sessionId: 's1',
    name: 'Preview',
    port: 3000,
    address: 'http://localhost:3000',
    state: 'active',
    createdAt: '2026-01-01T00:00:00Z',
  };

  it('shows the address, state and open link for a registered server', async () => {
    renderPanel('files', vi.fn(), vi.fn(), makeSession({ demoServers: [preview] }));

    expect(await screen.findByText('Preview')).toBeInTheDocument();
    expect(screen.getByText('Active')).toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: 'Open http://localhost:3000 for Preview' }),
    ).toHaveAttribute('href', 'http://localhost:3000');
    expect(
      screen.getByRole('button', { name: 'Stop Preview' }),
    ).toBeInTheDocument();
  });

  it('stops a server and shows its stopped state without an open link', async () => {
    vi.mocked(stopDemoServer).mockResolvedValue(
      makeSession({
        demoServers: [{ ...preview, state: 'stopped' }],
      }),
    );
    const user = userEvent.setup();
    renderPanel('files', vi.fn(), vi.fn(), makeSession({ demoServers: [preview] }));

    await user.click(
      await screen.findByRole('button', { name: 'Stop Preview' }),
    );

    expect(stopDemoServer).toHaveBeenCalledWith('s1', 'demo-1');
    expect(await screen.findByText('Stopped')).toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: /localhost:3000/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Stop Preview' }),
    ).not.toBeInTheDocument();
  });

  it('shows and opens the local and tailnet addresses', async () => {
    const tailscaleAddress = 'https://studio.tailnet.ts.net:3000';
    const user = userEvent.setup();
    renderPanel(
      'files',
      vi.fn(),
      vi.fn(),
      makeSession({
        demoServers: [{ ...preview, tailscaleAddress }],
      }),
    );

    const local = await screen.findByRole('link', {
      name: 'Open http://localhost:3000 for Preview',
    });
    const tailnet = screen.getByRole('link', {
      name: `Open ${tailscaleAddress} for Preview`,
    });
    expect(local).toHaveAttribute('href', 'http://localhost:3000');
    expect(tailnet).toHaveAttribute('href', tailscaleAddress);

    await user.click(local);
    expect(openUrl).toHaveBeenCalledWith('http://localhost:3000');
    await user.click(tailnet);
    expect(openUrl).toHaveBeenCalledWith(tailscaleAddress);
  });

  it('never opens an address outside loopback and the tailnet', async () => {
    const user = userEvent.setup();
    renderPanel(
      'files',
      vi.fn(),
      vi.fn(),
      makeSession({
        demoServers: [
          {
            ...preview,
            tailscaleAddress: 'https://example.com:3000',
          },
        ],
      }),
    );

    const shown = await screen.findByText('https://example.com:3000');
    expect(shown).not.toHaveAttribute('href');
    await user.click(shown);
    expect(openUrl).not.toHaveBeenCalledWith('https://example.com:3000');
  });

  it('reports a failed stop and keeps the server active', async () => {
    vi.mocked(stopDemoServer).mockRejectedValueOnce(
      new Error('The session is closed.'),
    );
    const user = userEvent.setup();
    renderPanel('files', vi.fn(), vi.fn(), makeSession({ demoServers: [preview] }));

    await user.click(
      await screen.findByRole('button', { name: 'Stop Preview' }),
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The session is closed.',
    );
    expect(
      screen.getByRole('link', { name: 'Open http://localhost:3000 for Preview' }),
    ).toBeInTheDocument();
  });

  it('asks before making a server public and shows the public address', async () => {
    const publicAddress = 'https://studio.tailnet.ts.net/';
    vi.mocked(enableDemoFunnel).mockResolvedValue(
      makeSession({
        demoServers: [{ ...preview, funnelAddress: publicAddress }],
      }),
    );
    const user = userEvent.setup();
    renderPanel('files', vi.fn(), vi.fn(), makeSession({ demoServers: [preview] }));

    await user.click(
      await screen.findByRole('button', {
        name: 'Make Preview public on the internet',
      }),
    );

    const dialog = screen.getByRole('dialog', { name: 'Make this server public?' });
    expect(dialog).toHaveTextContent(
      'Anyone on the internet can open Preview',
    );
    expect(enableDemoFunnel).not.toHaveBeenCalled();

    await user.click(
      within(dialog).getByRole('button', { name: 'Make public' }),
    );

    expect(enableDemoFunnel).toHaveBeenCalledWith('s1', 'demo-1');
    expect(
      await screen.findByRole('link', {
        name: `Open public address ${publicAddress} for Preview`,
      }),
    ).toHaveAttribute('href', publicAddress);
    expect(
      screen.getByRole('button', { name: 'Stop the public address for Preview' }),
    ).toBeInTheDocument();
  });

  it('stops the public address and keeps the server local', async () => {
    const publicAddress = 'https://studio.tailnet.ts.net/';
    vi.mocked(disableDemoFunnel).mockResolvedValue(
      makeSession({ demoServers: [preview] }),
    );
    const user = userEvent.setup();
    renderPanel(
      'files',
      vi.fn(),
      vi.fn(),
      makeSession({
        demoServers: [{ ...preview, funnelAddress: publicAddress }],
      }),
    );

    await user.click(
      await screen.findByRole('button', {
        name: 'Stop the public address for Preview',
      }),
    );

    expect(disableDemoFunnel).toHaveBeenCalledWith('s1', 'demo-1');
    expect(
      await screen.findByRole('button', {
        name: 'Make Preview public on the internet',
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText(publicAddress)).not.toBeInTheDocument();
    expect(
      screen.getByRole('link', { name: 'Open http://localhost:3000 for Preview' }),
    ).toBeInTheDocument();
  });

  it('leaves the server local when Tailscale refuses', async () => {
    vi.mocked(enableDemoFunnel).mockRejectedValue(
      new ApiError(422, 'Tailscale is not installed, not running or not signed in. The server stays local.'),
    );
    const user = userEvent.setup();
    renderPanel('files', vi.fn(), vi.fn(), makeSession({ demoServers: [preview] }));

    await user.click(
      await screen.findByRole('button', {
        name: 'Make Preview public on the internet',
      }),
    );
    const dialog = screen.getByRole('dialog', { name: 'Make this server public?' });
    await user.click(
      within(dialog).getByRole('button', { name: 'Make public' }),
    );

    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'Tailscale is not installed, not running or not signed in.',
    );
    expect(
      screen.getByRole('link', { name: 'Open http://localhost:3000 for Preview' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: /Open public address/ }),
    ).not.toBeInTheDocument();
  });
});

describe('SessionSidePanel changes', () => {
  it('shows changed files with counts and a diff preview', async () => {
    renderPanel('changes');

    expect(
      await screen.findByRole('button', { name: /src\/redirect\.ts/ }),
    ).toBeInTheDocument();
    expect(screen.getByText('+2')).toBeInTheDocument();
    expect(screen.getByText('−1')).toBeInTheDocument();
    expect(screen.getByText('-old')).toBeInTheDocument();
    expect(screen.getByText('+new')).toBeInTheDocument();
  });

  it('refreshes the changes on demand', async () => {
    const user = userEvent.setup();
    renderPanel('changes');

    await screen.findByRole('button', { name: /src\/redirect\.ts/ });
    await user.click(screen.getByRole('button', { name: 'Refresh' }));

    await waitFor(() => expect(sessionChanges).toHaveBeenCalledTimes(2));
  });

  it('reports a plain folder', async () => {
    vi.mocked(sessionChanges).mockResolvedValueOnce({
      git: false,
      base: null,
      files: [],
      truncated: false,
      error: null,
    });
    renderPanel('changes');

    expect(
      await screen.findByText('This folder is not a Git repository.'),
    ).toBeInTheDocument();
  });
});
