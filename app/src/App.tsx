import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  ApiError,
  bootstrap,
  closeSession,
  createAccount,
  createProject,
  createSession,
  deleteSession,
  health,
  renameSession,
  reopenSession,
  respondPermission,
} from './api/client';
import { subscribeEvents } from './api/events';
import type {
  Account,
  AgentCheck,
  PermissionDecision,
  Project,
  Session,
} from './api/types';
import { AddProjectDialog } from './components/AddProjectDialog';
import {
  BranchActionsMenu,
  type BranchMenuAction,
} from './components/BranchActionsMenu';
import { CloseSessionDialog } from './components/CloseSessionDialog';
import { DeleteSessionDialog } from './components/DeleteSessionDialog';
import { FirstLaunch } from './components/FirstLaunch';
import { NewSessionDialog } from './components/NewSessionDialog';
import { RenameSessionDialog } from './components/RenameSessionDialog';
import {
  SessionActionsMenu,
  type SessionMenuAction,
} from './components/SessionActionsMenu';
import { SessionHeader } from './components/SessionHeader';
import { SessionView } from './components/SessionView';
import { SettingsDialog } from './components/SettingsDialog';
import { Sidebar } from './components/Sidebar';
import { TabStrip } from './components/TabStrip';
import { PlusIcon, SparkIcon, TerminalIcon } from './components/icons';
import {
  closeSessionTab,
  openSessionTab,
  type TabsByProject,
} from './state/tabs';

type LoadState = 'loading' | 'ready' | 'error';

interface SessionMenuState {
  sessionId: string;
  point: { x: number; y: number };
  placement: 'below-end' | 'at-point';
  trigger: HTMLElement | null;
  fromHeader: boolean;
}

interface ToastState {
  message: string;
  error: boolean;
}

interface BranchMenuState {
  sessionId: string;
  trigger: HTMLElement;
}

function messageFor(error: unknown): string {
  return error instanceof Error && error.message
    ? error.message
    : 'The request failed.';
}

export default function App() {
  const [loadState, setLoadState] = useState<LoadState>('loading');
  const [loadError, setLoadError] = useState('');
  const [projects, setProjects] = useState<Project[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [agents, setAgents] = useState<AgentCheck[]>([]);
  const [agentsChecked, setAgentsChecked] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  const [tabsByProject, setTabsByProject] = useState<TabsByProject>({});
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [projectDialogOpen, setProjectDialogOpen] = useState(false);
  const [closeSessionId, setCloseSessionId] = useState<string | null>(null);
  const [renameSessionId, setRenameSessionId] = useState<string | null>(null);
  const [deleteSessionId, setDeleteSessionId] = useState<string | null>(null);
  const [sessionMenu, setSessionMenu] = useState<SessionMenuState | null>(null);
  const [branchMenu, setBranchMenu] = useState<BranchMenuState | null>(null);
  const [dialogProjectId, setDialogProjectId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toast, setToast] = useState<ToastState | null>(null);
  const [eventsPaused, setEventsPaused] = useState(false);
  const [eventAttempt, setEventAttempt] = useState(0);
  const [announcement, setAnnouncement] = useState('');
  const [focusSessionChangeTick, setFocusSessionChangeTick] = useState(0);

  const mainRef = useRef<HTMLElement>(null);
  const focusMainRef = useRef(false);
  const sessionsRef = useRef<Session[]>([]);
  sessionsRef.current = sessions;
  const activeSessionIdRef = useRef<string | null>(null);
  const activeStatusRef = useRef<Session['status'] | null>(null);

  const load = useCallback(async () => {
    setLoadState('loading');
    setLoadError('');
    try {
      const [data, report] = await Promise.all([
        bootstrap(),
        health().catch(() => null),
      ]);
      setProjects(data.projects);
      setAccounts(data.accounts);
      setSessions(data.sessions);
      setAgents(report?.agents ?? []);
      setAgentsChecked(report !== null);
      setVersion(report?.version ?? null);
      setTabsByProject({});
      setCloseSessionId(null);
      setRenameSessionId(null);
      setDeleteSessionId(null);
      setSessionMenu(null);
      setBranchMenu(null);
      setSettingsOpen(false);
      setProjectDialogOpen(false);
      setActiveProjectId(
        data.sessions[0]?.projectId ?? data.projects[0]?.id ?? null,
      );
      setLoadState('ready');
    } catch (error) {
      setLoadError(messageFor(error));
      setLoadState('error');
    }
  }, []);

  const activeProject =
    projects.find((project) => project.id === activeProjectId) ?? null;
  const projectTabs = activeProjectId
    ? tabsByProject[activeProjectId]
    : undefined;
  const tabSessions = (projectTabs?.open ?? [])
    .map((id) => sessions.find((session) => session.id === id))
    .filter((session): session is Session => session !== undefined);
  const activeSessionId = projectTabs?.active ?? null;
  const activeSession =
    tabSessions.find((session) => session.id === activeSessionId) ?? null;
  const closeTarget = closeSessionId
    ? sessions.find((session) => session.id === closeSessionId) ?? null
    : null;
  const renameTarget = renameSessionId
    ? sessions.find((session) => session.id === renameSessionId) ?? null
    : null;
  const deleteTarget = deleteSessionId
    ? sessions.find((session) => session.id === deleteSessionId) ?? null
    : null;
  const menuSession = sessionMenu
    ? sessions.find((session) => session.id === sessionMenu.sessionId) ?? null
    : null;
  const branchSession = branchMenu
    ? sessions.find((session) => session.id === branchMenu.sessionId) ?? null
    : null;
  const anyDialogOpen =
    dialogOpen ||
    projectDialogOpen ||
    closeTarget !== null ||
    renameTarget !== null ||
    deleteTarget !== null ||
    settingsOpen;

  activeSessionIdRef.current = activeSessionId;

  useLayoutEffect(() => {
    if (focusMainRef.current) {
      focusMainRef.current = false;
      mainRef.current?.focus();
    }
  }, [projects, activeProjectId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (loadState !== 'ready') {
      return;
    }

    setEventsPaused(false);

    return subscribeEvents(
      (session) => {
        const previous = sessionsRef.current.find(
          (item) => item.id === session.id,
        );

        if (previous) {
          if (previous.status === 'active' && session.status === 'closed') {
            setAnnouncement(`${session.name} closed.`);
          } else if (
            !previous.waitingFor &&
            session.waitingFor &&
            session.id !== activeSessionIdRef.current
          ) {
            setAnnouncement(`${session.name} is waiting for you.`);
          }
        }

        setSessions((current) => {
          const index = current.findIndex((item) => item.id === session.id);
          if (index === -1) {
            return [...current, session];
          }
          const next = current.slice();
          next[index] = session;
          return next;
        });
      },
      () => setEventsPaused(true),
      () => setEventsPaused(false),
    );
  }, [loadState, eventAttempt]);

  useLayoutEffect(() => {
    const status = activeSession?.status ?? null;

    if (status === 'closed' && activeStatusRef.current === 'active') {
      if (document.activeElement === document.body) {
        document
          .querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')
          ?.focus();
      }
    }

    activeStatusRef.current = status;
  }, [activeSession?.status]);

  useLayoutEffect(() => {
    if (focusSessionChangeTick === 0) {
      return;
    }

    const activeTab = document.querySelector<HTMLElement>(
      '[role="tab"][aria-selected="true"]',
    );
    if (activeTab) {
      activeTab.focus();
      return;
    }

    document.querySelector<HTMLElement>('.titlebar-new')?.focus();
  }, [focusSessionChangeTick]);

  useEffect(() => {
    if (!toast) {
      return;
    }

    const timer = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const openProject = useCallback((project: Project) => {
    setActiveProjectId(project.id);
  }, []);

  function openNewSession(projectId?: string | null) {
    setDialogProjectId(projectId ?? activeProjectId ?? null);
    setDialogOpen(true);
  }

  const openSession = useCallback((session: Session) => {
    setActiveProjectId(session.projectId);
    setTabsByProject((current) =>
      openSessionTab(current, session.projectId, session.id),
    );
  }, []);

  function handleCreated(session: Session) {
    setSessions((current) =>
      current.some((item) => item.id === session.id) ? current : [...current, session],
    );
    setDialogOpen(false);
    openSession(session);
  }

  function handleAccountCreated(account: Account) {
    setAccounts((current) =>
      current.some((item) => item.id === account.id)
        ? current
        : [...current, account],
    );
  }

  function handleProjectCreated(project: Project) {
    const isFirstProject = projects.length === 0;

    setProjects((current) =>
      current.some((item) => item.id === project.id)
        ? current
        : [...current, project],
    );
    setActiveProjectId(project.id);
    setProjectDialogOpen(false);
    focusMainRef.current = isFirstProject;
  }

  async function confirmClose() {
    const target = closeSessionId
      ? sessions.find((session) => session.id === closeSessionId)
      : undefined;
    if (!target) {
      return;
    }

    const closed = await closeSession(target.id);
    setSessions((current) =>
      current.map((session) => (session.id === closed.id ? closed : session)),
    );
    setTabsByProject((current) =>
      closeSessionTab(current, target.projectId, target.id),
    );
    setCloseSessionId(null);
    setFocusSessionChangeTick((tick) => tick + 1);
  }

  function openHeaderMenu(session: Session, trigger: HTMLElement) {
    setBranchMenu(null);
    const rect = trigger.getBoundingClientRect();
    setSessionMenu({
      sessionId: session.id,
      point: { x: rect.right, y: rect.bottom + 4 },
      placement: 'below-end',
      trigger,
      fromHeader: true,
    });
  }

  function openSessionMenu(
    session: Session,
    point: { x: number; y: number },
    trigger: HTMLElement,
  ) {
    setBranchMenu(null);
    setSessionMenu({
      sessionId: session.id,
      point,
      placement: 'at-point',
      trigger,
      fromHeader: false,
    });
  }

  function openBranchMenu(session: Session, trigger: HTMLElement) {
    setSessionMenu(null);
    setBranchMenu({ sessionId: session.id, trigger });
  }

  function handleBranchAction(action: BranchMenuAction, session: Session) {
    switch (action) {
      case 'copy-branch':
        if (session.branch) {
          void copyText(session.branch, 'Branch name copied.');
        }
        break;
      case 'copy-folder':
        if (session.folder) {
          void copyText(session.folder, 'Folder path copied.');
        }
        break;
    }
  }

  async function copyText(value: string, message: string) {
    try {
      await navigator.clipboard.writeText(value);
      setToast({ message, error: false });
    } catch {
      setToast({ message: 'The clipboard is not available.', error: true });
    }
  }

  async function handleReopen(session: Session) {
    try {
      const reopened = await reopenSession(session.id);
      setSessions((current) =>
        current.map((item) => (item.id === reopened.id ? reopened : item)),
      );
      openSession(reopened);
      setFocusSessionChangeTick((tick) => tick + 1);
      setToast({ message: 'Session reopened.', error: false });
    } catch (error) {
      setToast({
        message:
          error instanceof ApiError
            ? error.message
            : 'The session was not reopened.',
        error: true,
      });
    }
  }

  function handleMenuAction(action: SessionMenuAction, session: Session) {
    setSessionMenu(null);

    switch (action) {
      case 'rename':
        setRenameSessionId(session.id);
        break;
      case 'copy-folder':
        void copyText(session.folder, 'Folder path copied.');
        break;
      case 'copy-branch':
        if (session.branch) {
          void copyText(session.branch, 'Branch name copied.');
        }
        break;
      case 'close':
        setCloseSessionId(session.id);
        break;
      case 'reopen':
        void handleReopen(session);
        break;
      case 'delete':
        setDeleteSessionId(session.id);
        break;
    }
  }

  async function performRename(name: string) {
    if (!renameTarget) {
      return;
    }

    const updated = await renameSession(renameTarget.id, name);
    setSessions((current) =>
      current.map((item) =>
        item.id === updated.id
          ? { ...updated, short: item.short, branch: item.branch }
          : item,
      ),
    );
  }

  async function answerPermission(
    session: Session,
    decision: PermissionDecision,
  ) {
    const pending = session.pendingPermission ?? null;
    if (!pending) {
      return;
    }

    try {
      await respondPermission(session.id, pending.requestId, decision);
    } catch (error) {
      if (error instanceof ApiError && error.status === 409) {
        setSessions((current) =>
          current.map((item) =>
            item.id === session.id
              ? {
                  ...item,
                  pendingPermission: null,
                  waitingFor:
                    item.waitingFor === 'permission' ? null : item.waitingFor,
                }
              : item,
          ),
        );
        setToast({
          message: 'That permission request was already resolved.',
          error: false,
        });
        return;
      }

      throw error;
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) {
      return;
    }

    await deleteSession(deleteTarget.id);
    setSessions((current) =>
      current.filter((item) => item.id !== deleteTarget.id),
    );
    setTabsByProject((current) =>
      closeSessionTab(current, deleteTarget.projectId, deleteTarget.id),
    );
    setDeleteSessionId(null);
    setFocusSessionChangeTick((tick) => tick + 1);
  }

  if (loadState !== 'ready') {
    return (
      <div className="app">
        <main className="boot-state">
          <div className="empty-card">
            <div className="empty-mark">
              <SparkIcon />
            </div>
            <h1 className="empty-title">
              {loadState === 'loading' ? 'Starting elesdee' : 'Cannot reach the core service'}
            </h1>
            <p className="empty-text">
              {loadState === 'loading'
                ? 'Connecting to the local service.'
                : loadError}
            </p>
            {loadState === 'error' ? (
              <button type="button" className="btn btn-primary boot-retry" onClick={() => void load()}>
                Retry
              </button>
            ) : null}
          </div>
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <div className="app-frame" inert={anyDialogOpen}>
        <a className="skip-link" href="#session-content">
          Skip to session
        </a>

        <header className="titlebar">
          <div className="brand">
            <span className="brand-mark" aria-hidden="true" />
            elesdee
          </div>
          <div className="titlebar-spacer" />
          {projects.length > 0 ? (
            <button
              type="button"
              className="btn titlebar-new"
              onClick={() => openNewSession()}
            >
              <PlusIcon />
              New session
            </button>
          ) : null}
        </header>

        {eventsPaused ? (
          <div className="events-note">
            <span role="status">Live updates paused.</span>
            <button
              type="button"
              className="link-button"
              onClick={() => setEventAttempt((attempt) => attempt + 1)}
            >
              Retry
            </button>
          </div>
        ) : null}

        <div className="shell">
          <Sidebar
            projects={projects}
            sessions={sessions}
            accounts={accounts}
            activeProjectId={activeProjectId}
            activeSessionId={activeSessionId}
            onSelectProject={openProject}
            onSelectSession={openSession}
            onSessionMenu={openSessionMenu}
            onAddProject={() => setProjectDialogOpen(true)}
            onNewSession={(projectId) => openNewSession(projectId)}
            onOpenSettings={() => setSettingsOpen(true)}
          />

          <main
            className="main"
            id="session-content"
            ref={mainRef}
            tabIndex={-1}
          >
            {activeProject ? (
              <TabStrip
                project={activeProject}
                sessions={tabSessions}
                activeSessionId={activeSessionId}
                onSelectSession={openSession}
                onSessionMenu={openSessionMenu}
                onNewSession={() => openNewSession(activeProjectId)}
              />
            ) : null}

            {projects.length === 0 ? (
              <FirstLaunch
                agents={agents}
                agentsChecked={agentsChecked}
                onAddProject={() => setProjectDialogOpen(true)}
              />
            ) : activeSession ? (
              <div
                id="session-panel"
                className="session-tabpanel"
                role="tabpanel"
                aria-labelledby={`tab-${activeSession.id}`}
              >
                <SessionHeader
                  session={activeSession}
                  accounts={accounts}
                  actionsOpen={
                    sessionMenu?.fromHeader === true &&
                    sessionMenu.sessionId === activeSession.id
                  }
                  onOpenMenu={(trigger) =>
                    openHeaderMenu(activeSession, trigger)
                  }
                  branchOpen={branchMenu?.sessionId === activeSession.id}
                  onOpenBranchMenu={(trigger) =>
                    openBranchMenu(activeSession, trigger)
                  }
                />
                <SessionView
                  key={activeSession.id}
                  session={activeSession}
                  onRespondPermission={(decision) =>
                    answerPermission(activeSession, decision)
                  }
                  onReopen={() => void handleReopen(activeSession)}
                />
              </div>
            ) : (
              <div className="empty-main">
                <div className="empty-card">
                  <div className="empty-mark">
                    <TerminalIcon />
                  </div>
                  <h1 className="empty-title">Choose a session</h1>
                  <p className="empty-text">
                    Open a session from the sidebar to see its terminal, or start a
                    new one.
                  </p>
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => openNewSession()}
                  >
                    New session
                  </button>
                </div>
              </div>
            )}
          </main>
        </div>
      </div>

      {dialogOpen ? (
        <NewSessionDialog
          projects={projects}
          accounts={accounts}
          initialProjectId={dialogProjectId ?? activeProjectId}
          onCreate={createSession}
          onCreated={handleCreated}
          onClose={() => setDialogOpen(false)}
        />
      ) : null}

      {projectDialogOpen ? (
        <AddProjectDialog
          onCreate={createProject}
          onCreated={handleProjectCreated}
          onClose={() => setProjectDialogOpen(false)}
        />
      ) : null}

      {closeTarget ? (
        <CloseSessionDialog
          session={closeTarget}
          onConfirm={confirmClose}
          onClose={() => setCloseSessionId(null)}
        />
      ) : null}

      {renameTarget ? (
        <RenameSessionDialog
          session={renameTarget}
          onRename={performRename}
          onClose={() => setRenameSessionId(null)}
        />
      ) : null}

      {deleteTarget ? (
        <DeleteSessionDialog
          session={deleteTarget}
          onConfirm={confirmDelete}
          onClose={() => setDeleteSessionId(null)}
        />
      ) : null}

      {settingsOpen ? (
        <SettingsDialog
          accounts={accounts}
          version={version}
          onCreateAccount={createAccount}
          onAccountCreated={handleAccountCreated}
          onClose={() => setSettingsOpen(false)}
        />
      ) : null}

      {branchMenu && branchSession ? (
        <BranchActionsMenu
          session={branchSession}
          trigger={branchMenu.trigger}
          onAction={(action) => handleBranchAction(action, branchSession)}
          onClose={() => setBranchMenu(null)}
        />
      ) : null}

      {sessionMenu && menuSession ? (
        <SessionActionsMenu
          session={menuSession}
          point={sessionMenu.point}
          placement={sessionMenu.placement}
          trigger={sessionMenu.trigger}
          onAction={(action) => handleMenuAction(action, menuSession)}
          onClose={() => setSessionMenu(null)}
        />
      ) : null}

      {toast ? (
        <div
          className={toast.error ? 'toast is-error' : 'toast'}
          role={toast.error ? 'alert' : 'status'}
        >
          {toast.message}
        </div>
      ) : null}

      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>
    </div>
  );
}
