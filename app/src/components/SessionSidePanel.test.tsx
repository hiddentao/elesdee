import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  listFiles,
  listSharedFiles,
  readFile,
  readSharedFile,
  sessionChanges,
  sharedFileUrl,
  writeFile,
} from '../api/client';
import type { FileEntry, SharedFile } from '../api/types';
import { makeSession } from '../test/fixtures';
import { SessionSidePanel } from './SessionSidePanel';

vi.mock('../api/client', () => ({
  listFiles: vi.fn(),
  listSharedFiles: vi.fn(),
  readFile: vi.fn(),
  readSharedFile: vi.fn(),
  sessionChanges: vi.fn(),
  sharedFileUrl: vi.fn(),
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
) {
  return render(
    <SessionSidePanel
      session={makeSession()}
      tab={tab}
      onSelectTab={onSelectTab}
      onClose={onClose}
    />,
  );
}

beforeEach(() => {
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

    await user.click(screen.getByRole('button', { name: 'Close side panel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
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
