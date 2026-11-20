import { useEffect, useRef, useState } from 'react';
import {
  listFiles,
  listSharedFiles,
  readFile,
  readSharedFile,
  sessionChanges,
  sharedFileUrl,
  stopDemoServer,
  writeFile,
} from '../api/client';
import type {
  ChangeStatus,
  ChangedFile,
  DemoServer,
  FileEntry,
  Session,
  SessionChanges,
  SharedFile,
} from '../api/types';
import type { Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { openUrl } from '../api/shell';
import { BrowserPanel } from './BrowserPanel';
import { isOpenableDemoAddress } from './demoAddress';
import { FileEditor } from './FileEditor';
import {
  ChevronDownIcon,
  ChevronRightIcon,
  CloseIcon,
  FileIcon,
  FolderIcon,
} from './icons';

export type DockTab = 'files' | 'changes' | 'browser';

interface SessionSidePanelProps {
  session: Session;
  tab: DockTab;
  sharedVersion?: number;
  onSelectTab: (tab: DockTab) => void;
  onClose: () => void;
}

interface OpenFile {
  path: string;
  content: string;
}

function statusLabel(status: ChangeStatus, t: Translate): string {
  switch (status) {
    case 'added':
      return t('changeStatus.added');
    case 'modified':
      return t('changeStatus.modified');
    case 'deleted':
      return t('changeStatus.deleted');
    case 'renamed':
      return t('changeStatus.renamed');
    case 'untracked':
      return t('changeStatus.untracked');
    case 'conflicted':
      return t('changeStatus.conflicted');
  }
}

function messageFor(error: unknown, t: Translate): string {
  return error instanceof Error && error.message
    ? error.message
    : t('panel.requestFailed');
}

export function SessionSidePanel({
  session,
  tab,
  sharedVersion = 0,
  onSelectTab,
  onClose,
}: SessionSidePanelProps) {
  const { t } = useLanguage();
  const [children, setChildren] = useState<Record<string, FileEntry[]>>({});
  const [truncated, setTruncated] = useState<Record<string, boolean>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [loadingDirs, setLoadingDirs] = useState<Set<string>>(new Set());
  const [filesError, setFilesError] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [file, setFile] = useState<OpenFile | null>(null);
  const [originals, setOriginals] = useState<Record<string, string>>({});
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [loadingFile, setLoadingFile] = useState(false);
  const [saving, setSaving] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [changes, setChanges] = useState<SessionChanges | null>(null);
  const [changesLoading, setChangesLoading] = useState(false);
  const [changesError, setChangesError] = useState<string | null>(null);
  const [selectedChange, setSelectedChange] = useState<string | null>(null);

  const [shared, setShared] = useState<SharedFile[]>([]);
  const [sharedLoading, setSharedLoading] = useState(false);
  const [sharedError, setSharedError] = useState<string | null>(null);
  const [sharedPreview, setSharedPreview] = useState<SharedFile | null>(null);
  const [sharedText, setSharedText] = useState<string | null>(null);
  const [sharedFileLoading, setSharedFileLoading] = useState(false);
  const [sharedFileError, setSharedFileError] = useState<string | null>(null);

  const [demoServers, setDemoServers] = useState<DemoServer[]>(
    session.demoServers ?? [],
  );
  const [stoppingServer, setStoppingServer] = useState<string | null>(null);
  const [demoError, setDemoError] = useState<string | null>(null);

  const request = useRef(0);

  async function loadDirectory(path: string) {
    setLoadingDirs((current) => new Set(current).add(path));
    setFilesError(null);
    try {
      const listing = await listFiles(session.id, path);
      setChildren((current) => ({ ...current, [path]: listing.entries }));
      setTruncated((current) => ({ ...current, [path]: listing.truncated }));
    } catch (error) {
      setFilesError(messageFor(error, t));
    } finally {
      setLoadingDirs((current) => {
        const next = new Set(current);
        next.delete(path);
        return next;
      });
    }
  }

  async function loadShared() {
    setSharedLoading(true);
    setSharedError(null);
    try {
      setShared(await listSharedFiles(session.id));
    } catch (error) {
      setSharedError(messageFor(error, t));
    } finally {
      setSharedLoading(false);
    }
  }

  async function loadChanges() {    setChangesLoading(true);
    setChangesError(null);
    try {
      const next = await sessionChanges(session.id);
      setChanges(next);
      setSelectedChange((current) =>
        current && next.files.some((item) => item.path === current)
          ? current
          : next.files[0]?.path ?? null,
      );
    } catch (error) {
      setChangesError(messageFor(error, t));
    } finally {
      setChangesLoading(false);
    }
  }

  async function stopServer(server: DemoServer) {
    setStoppingServer(server.id);
    setDemoError(null);
    try {
      const updated = await stopDemoServer(session.id, server.id);
      setDemoServers(updated.demoServers ?? []);
    } catch (error) {
      setDemoError(messageFor(error, t));
    } finally {
      setStoppingServer(null);
    }
  }

  useEffect(() => {
    setDemoServers(session.demoServers ?? []);
  }, [session.demoServers]);

  useEffect(() => {
    setChildren({});
    setTruncated({});
    setExpanded(new Set());
    setSelectedPath(null);
    setFile(null);
    setOriginals({});
    setDrafts({});
    setFileError(null);
    setNotice(null);
    setChanges(null);
    setSelectedChange(null);
    setShared([]);
    setSharedPreview(null);
    setSharedText(null);
    setSharedFileError(null);
    setSharedError(null);
    void loadDirectory('');
    // The panel is recreated for each session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id]);

  useEffect(() => {
    if (tab === 'changes') {
      void loadChanges();
    }
    if (tab === 'files') {
      void loadShared();
    }
    // Refresh when the tab is opened or a file is shared.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, session.id, sharedVersion]);

  async function openFile(entry: FileEntry) {
    setSelectedPath(entry.path);
    setFileError(null);
    setNotice(null);
    setSharedPreview(null);
    setSharedText(null);
    setSharedFileError(null);

    const draft = drafts[entry.path];
    if (draft !== undefined) {
      setFile({ path: entry.path, content: draft });
      return;
    }

    const token = request.current + 1;
    request.current = token;
    setLoadingFile(true);
    try {
      const result = await readFile(session.id, entry.path);
      if (request.current !== token) {
        return;
      }
      setOriginals((current) => ({ ...current, [entry.path]: result.content }));
      setFile({ path: entry.path, content: result.content });
    } catch (error) {
      if (request.current === token) {
        setFile(null);
        setFileError(messageFor(error, t));
      }
    } finally {
      if (request.current === token) {
        setLoadingFile(false);
      }
    }
  }

  async function openShared(file: SharedFile) {
    setSelectedPath(null);
    setFile(null);
    setFileError(null);
    setNotice(null);
    setSharedPreview(file);
    setSharedText(null);
    setSharedFileError(null);
    if (file.image) {
      return;
    }
    if (!file.contentType.startsWith('text/')) {
      setSharedFileError(t('panel.sharedNoPreview'));
      return;
    }
    setSharedFileLoading(true);
    try {
      setSharedText(await readSharedFile(session.id, file.id));
    } catch (error) {
      setSharedFileError(messageFor(error, t));
    } finally {
      setSharedFileLoading(false);
    }
  }

  function toggleFolder(path: string) {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
    if (!children[path] && !loadingDirs.has(path)) {
      void loadDirectory(path);
    }
  }

  function changeFile(value: string) {
    if (!file) {
      return;
    }
    const path = file.path;
    setNotice(null);
    setFile({ path, content: value });
    setDrafts((current) => {
      const next = { ...current };
      if (value === originals[path]) {
        delete next[path];
      } else {
        next[path] = value;
      }
      return next;
    });
  }

  async function saveFile() {
    if (!file || drafts[file.path] === undefined) {
      return;
    }
    const path = file.path;
    const content = file.content;
    setSaving(true);
    setFileError(null);
    try {
      await writeFile(session.id, path, content);
      setOriginals((current) => ({ ...current, [path]: content }));
      setDrafts((current) => {
        const next = { ...current };
        delete next[path];
        return next;
      });
      setNotice(t('panel.saved'));
      if (tab === 'changes') {
        void loadChanges();
      }
    } catch (error) {
      setFileError(messageFor(error, t));
    } finally {
      setSaving(false);
    }
  }

  const dirty = file ? drafts[file.path] !== undefined : false;
  const activeChange = changes?.files.find(
    (item) => item.path === selectedChange,
  );

  function renderEntries(path: string, depth: number) {
    const entries = children[path];
    if (!entries) {
      return loadingDirs.has(path) ? (
        <li className="tree-note" role="none">
          {t('panel.loading')}
        </li>
      ) : null;
    }
    if (entries.length === 0) {
      return (
        <li className="tree-note" role="none">
          {t('panel.emptyFolder')}
        </li>
      );
    }

    return entries.map((entry) => {
      const isOpen = entry.directory && expanded.has(entry.path);
      const isDirty = !entry.directory && drafts[entry.path] !== undefined;
      return (
        <li key={entry.path} role="treeitem" aria-expanded={entry.directory ? isOpen : undefined}>
          <button
            type="button"
            className={`tree-row${entry.directory ? ' is-folder' : ' is-file'}${
              selectedPath === entry.path ? ' is-on' : ''
            }`}
            style={{ paddingLeft: 7 + depth * 14 }}
            aria-current={selectedPath === entry.path ? 'true' : undefined}
            onClick={() =>
              entry.directory ? toggleFolder(entry.path) : void openFile(entry)
            }
          >
            {entry.directory ? (
              isOpen ? (
                <ChevronDownIcon />
              ) : (
                <ChevronRightIcon />
              )
            ) : (
              <span className="tree-spacer" aria-hidden="true" />
            )}
            {entry.directory ? <FolderIcon /> : <FileIcon />}
            <span className="tree-name">{entry.name}</span>
            {isDirty ? (
              <span
                className="tree-dirty"
                title={t('panel.unsavedChanges')}
                aria-label={t('panel.unsavedChanges')}
              />
            ) : null}
          </button>
          {isOpen ? (
            <ul role="group">{renderEntries(entry.path, depth + 1)}</ul>
          ) : null}
          {isOpen && truncated[entry.path] ? (
            <p className="tree-note" role="note">
              {t('panel.moreEntries')}
            </p>
          ) : null}
        </li>
      );
    });
  }

  return (
    <aside
      className="dock"
      aria-label={t('panel.label', { name: session.name })}
    >
      <div
        className="dock-head"
        role="tablist"
        aria-label={t('panel.tabsLabel')}
      >
        <button
          type="button"
          role="tab"
          id="dock-tab-files"
          aria-selected={tab === 'files'}
          aria-controls="dock-panel-files"
          className={`dock-tab${tab === 'files' ? ' is-on' : ''}`}
          onClick={() => onSelectTab('files')}
        >
          {t('panel.files')}
        </button>
        <button
          type="button"
          role="tab"
          id="dock-tab-changes"
          aria-selected={tab === 'changes'}
          aria-controls="dock-panel-changes"
          className={`dock-tab${tab === 'changes' ? ' is-on' : ''}`}
          onClick={() => onSelectTab('changes')}
        >
          {t('panel.changes')}
          {changes && changes.git && changes.files.length > 0 ? (
            <span className="dock-count">{changes.files.length}</span>
          ) : null}
        </button>
        <button
          type="button"
          role="tab"
          id="dock-tab-browser"
          aria-selected={tab === 'browser'}
          aria-controls="dock-panel-browser"
          className={`dock-tab${tab === 'browser' ? ' is-on' : ''}`}
          onClick={() => onSelectTab('browser')}
        >
          {t('panel.browser')}
        </button>
        <button
          type="button"
          className="icon-btn dock-close"
          aria-label={t('panel.close')}
          title={t('panel.close')}
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>

      <div className="dock-context">
        <span
          className="dock-context-dot"
          data-status={session.status}
          aria-hidden="true"
        />
        <span className="dock-context-name">{session.name}</span>
        <span className="dock-context-branch" title={session.branch ?? session.folder}>
          {session.branch ?? session.folder}
        </span>
      </div>

      <div className="dock-body">
        <section
          role="tabpanel"
          id="dock-panel-files"
          aria-labelledby="dock-tab-files"
          className="dock-view"
          hidden={tab !== 'files'}
        >
          {filesError ? (
            <p className="dock-error" role="alert">
              {filesError}
            </p>
          ) : null}
          {fileError ? (
            <p className="dock-error" role="alert">
              {fileError}
            </p>
          ) : null}
          {demoError ? (
            <p className="dock-error" role="alert">
              {demoError}
            </p>
          ) : null}
          {demoServers.length > 0 ? (
            <div className="demo-block">
              <div className="dock-toolbar">
                <span className="dock-toolbar-title">
                  {t('panel.demoServers')}
                </span>
              </div>
              <ul
                className="demo-list"
                aria-label={t('panel.demoServersLabel')}
              >
                {demoServers.map((server) => {
                  const active = server.state === 'active';
                  const addresses = [
                    server.address,
                    server.tailscaleAddress,
                  ].filter((value): value is string => Boolean(value));
                  return (
                    <li key={server.id} className="demo-row">
                      <span className="demo-name" title={server.name}>
                        {server.name}
                      </span>
                      <span
                        className={`demo-state is-${server.state}`}
                      >
                        {active
                          ? t('panel.demoServerActive')
                          : t('panel.demoServerStopped')}
                      </span>
                      <span className="demo-addresses">
                        {addresses.map((address) =>
                          active && isOpenableDemoAddress(address) ? (
                            <a
                              key={address}
                              className="demo-address"
                              href={address}
                              target="_blank"
                              rel="noreferrer"
                              aria-label={t('panel.demoServerOpenLabel', {
                                name: server.name,
                                address,
                              })}
                              onClick={(event) => {
                                event.preventDefault();
                                void openUrl(address);
                              }}
                            >
                              {address}
                            </a>
                          ) : (
                            <span
                              key={address}
                              className="demo-address is-inert"
                            >
                              {address}
                            </span>
                          ),
                        )}
                      </span>
                      {active ? (
                        <button
                          type="button"
                          className="btn btn-ghost dock-action"
                          disabled={stoppingServer === server.id}
                          aria-label={t('panel.demoServerStopLabel', {
                            name: server.name,
                          })}
                          onClick={() => void stopServer(server)}
                        >
                          {stoppingServer === server.id
                            ? t('panel.demoServerStopping')
                            : t('panel.demoServerStop')}
                        </button>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            </div>
          ) : null}
          {shared.length > 0 || sharedError ? (
            <div className="shared-block">
              <div className="dock-toolbar">
                <span className="dock-toolbar-title">
                  {t('panel.sharedFiles')}
                </span>
                <button
                  type="button"
                  className="btn btn-ghost dock-action"
                  disabled={sharedLoading}
                  onClick={() => void loadShared()}
                >
                  {sharedLoading ? t('panel.refreshing') : t('panel.refresh')}
                </button>
              </div>
              {sharedError ? (
                <p className="dock-error" role="alert">
                  {sharedError}
                </p>
              ) : null}
              <ul
                className="shared-list"
                aria-label={t('panel.sharedFilesLabel')}
              >
                {shared.map((item) => (
                  <li key={item.id}>
                    <button
                      type="button"
                      className={`tree-row is-file${
                        sharedPreview?.id === item.id ? ' is-on' : ''
                      }`}
                      aria-current={
                        sharedPreview?.id === item.id ? 'true' : undefined
                      }
                      onClick={() => void openShared(item)}
                    >
                      <span className="tree-spacer" aria-hidden="true" />
                      <FileIcon />
                      <span className="tree-name">{item.name}</span>
                      {item.caption ? (
                        <span className="shared-meta" title={item.caption}>
                          {item.caption}
                        </span>
                      ) : null}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          <ul
            className="file-tree"
            role="tree"
            aria-label={t('panel.filesLabel')}
          >
            {renderEntries('', 0)}
          </ul>

          {file || loadingFile ? (
            <div className="editor">
              <div className="editor-head">
                {dirty ? (
                  <span
                    className="dirty"
                    title={t('panel.unsavedChanges')}
                  />
                ) : null}
                <span className="editor-path" title={file?.path ?? ''}>
                  {file?.path ?? ''}
                </span>
                {notice ? <span className="editor-note">{notice}</span> : null}
                <button
                  type="button"
                  className="btn btn-ghost editor-save"
                  disabled={!dirty || saving}
                  onClick={() => void saveFile()}
                >
                  {saving ? t('panel.saving') : t('panel.save')}
                </button>
              </div>
              {loadingFile ? (
                <p className="dock-note">{t('panel.loadingFile')}</p>
              ) : file ? (
                <FileEditor
                  key={`${session.id}:${file.path}`}
                  filename={file.path}
                  value={file.content}
                  onChange={changeFile}
                  onSave={() => void saveFile()}
                />
              ) : null}
            </div>
          ) : sharedPreview ? (
            <div className="editor shared-preview">
              <div className="editor-head">
                <span className="editor-path" title={sharedPreview.path}>
                  {sharedPreview.name}
                </span>
                <a
                  className="btn btn-ghost editor-save"
                  href={sharedFileUrl(session.id, sharedPreview.id)}
                  download={sharedPreview.name}
                >
                  {t('panel.download')}
                </a>
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => setSharedPreview(null)}
                >
                  {t('common.close')}
                </button>
              </div>
              {sharedFileError ? (
                <p className="dock-error" role="alert">
                  {sharedFileError}
                </p>
              ) : sharedPreview.image ? (
                <img
                  className="shared-image"
                  src={sharedFileUrl(session.id, sharedPreview.id)}
                  alt={sharedPreview.name}
                />
              ) : sharedFileLoading ? (
                <p className="dock-note">{t('panel.sharedLoading')}</p>
              ) : (
                <pre className="shared-text" aria-label={sharedPreview.name}>
                  {sharedText ?? ''}
                </pre>
              )}
            </div>
          ) : (
            <p className="dock-note">{t('panel.selectFile')}</p>
          )}
        </section>

        <section
          role="tabpanel"
          id="dock-panel-changes"
          aria-labelledby="dock-tab-changes"
          className="dock-view"
          hidden={tab !== 'changes'}
        >
          <div className="dock-toolbar">
            <span className="dock-toolbar-title">
              {t('panel.sinceBranch')}
            </span>
            <button
              type="button"
              className="btn btn-ghost dock-action"
              disabled={changesLoading}
              onClick={() => void loadChanges()}
            >
              {changesLoading ? t('panel.refreshing') : t('panel.refresh')}
            </button>
          </div>

          {changesError ? (
            <p className="dock-error" role="alert">
              {changesError}
            </p>
          ) : null}

          {changes && changes.error ? (
            <p className="dock-error" role="alert">
              {changes.error}
            </p>
          ) : null}

          {!changes && changesLoading ? (
            <p className="dock-note">{t('panel.loadingChanges')}</p>
          ) : null}

          {changes && !changes.git ? (
            <p className="dock-note">{t('panel.notGit')}</p>
          ) : null}

          {changes?.git && changes.files.length === 0 ? (
            <p className="dock-note">{t('panel.noChanges')}</p>
          ) : null}

          {changes?.git && changes.files.length > 0 ? (
            <div className="diff-list">
              <ul
                className="change-list"
                aria-label={t('panel.changedFiles')}
              >
                {changes.files.map((item) => (
                  <li key={item.path}>
                    <button
                      type="button"
                      className={`change-file${
                        selectedChange === item.path ? ' is-on' : ''
                      }`}
                      aria-current={selectedChange === item.path ? 'true' : undefined}
                      onClick={() => setSelectedChange(item.path)}
                    >
                      <span className="change-main">
                        <code title={item.path}>{item.path}</code>
                        <span className={`change-status is-${item.status}`}>
                          {statusLabel(item.status, t)}
                        </span>
                      </span>
                      <span className="diff-stat">
                        <b>+{item.additions}</b> <i>−{item.deletions}</i>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>

              {activeChange ? (
                <DiffPreview file={activeChange} />
              ) : (
                <p className="dock-note">{t('panel.selectDiff')}</p>
              )}

              {changes.truncated ? (
                <p className="dock-note">{t('panel.largeChanges')}</p>
              ) : null}
            </div>
          ) : null}
        </section>

        <section
          role="tabpanel"
          id="dock-panel-browser"
          aria-labelledby="dock-tab-browser"
          className="dock-view"
          hidden={tab !== 'browser'}
        >
          {tab === 'browser' ? <BrowserPanel session={session} /> : null}
        </section>
      </div>
    </aside>
  );
}

function DiffPreview({ file }: { file: ChangedFile }) {
  const { t } = useLanguage();
  const lines = file.diff.split('\n');
  return (
    <div className="diff-preview">
      <div className="diff-head">
        <code>{file.path}</code>
        {file.oldPath ? (
          <span>{t('panel.from', { path: file.oldPath })}</span>
        ) : null}
      </div>
      {file.binary ? (
        <p className="dock-note">{t('panel.binary')}</p>
      ) : file.diff === '' ? (
        <p className="dock-note">{t('panel.noTextChanges')}</p>
      ) : (
        <pre
          className="diff-snippet"
          aria-label={t('panel.diffLabel', { path: file.path })}
        >
          {lines.map((line, index) => (
            <span key={index} className={diffClass(line)}>
              {line || ' '}
            </span>
          ))}
        </pre>
      )}
      {file.diffTruncated ? (
        <p className="dock-note">{t('panel.diffShortened')}</p>
      ) : null}
    </div>
  );
}

function diffClass(line: string): string {
  if (
    line.startsWith('+++') ||
    line.startsWith('---') ||
    line.startsWith('@@') ||
    line.startsWith('diff ')
  ) {
    return 'diff-line diff-meta';
  }
  if (line.startsWith('+')) {
    return 'diff-line diff-add';
  }
  if (line.startsWith('-')) {
    return 'diff-line diff-del';
  }
  return 'diff-line';
}
