import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ApiError,
  answerQuestion,
  approveSessionRequest,
  bootstrap,
  closeSession,
  createAccount,
  createProject,
  createSession,
  deleteSession,
  denySessionRequest,
  health,
  renameSession,
  reopenSession,
  respondPermission,
  resumeSession,
  switchSessionAccount,
  updateProjectDefaults,
} from './api/client';
import { subscribeAux, subscribeEvents } from './api/events';
import { isTauri, openFolder, openTerminal } from './api/shell';
import type {
  Account,
  AgentCheck,
  PermissionDecision,
  Project,
  Question,
  Session,
  SessionRequest,
  UpdateProjectDefaultsInput,
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
import { ProjectSettingsDialog } from './components/ProjectSettingsDialog';
import { QuitDialog } from './components/QuitDialog';
import { QuestionsDialog } from './components/QuestionsDialog';
import { RenameSessionDialog } from './components/RenameSessionDialog';
import { RequestsDialog } from './components/RequestsDialog';
import {
  SessionActionsMenu,
  type SessionMenuAction,
} from './components/SessionActionsMenu';
import { SessionHeader } from './components/SessionHeader';
import {
  SessionSidePanel,
  type DockTab,
} from './components/SessionSidePanel';
import { SessionView } from './components/SessionView';
import { SettingsDialog } from './components/SettingsDialog';
import { Sidebar } from './components/Sidebar';
import { SwitchAccountDialog } from './components/SwitchAccountDialog';
import { TabStrip } from './components/TabStrip';
import { PlusIcon, SparkIcon, TerminalIcon } from './components/icons';
import { useLanguage } from './i18n/LanguageProvider';
import type { Translate } from './i18n';
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

function messageFor(error: unknown, t: Translate): string {
  return error instanceof Error && error.message
    ? error.message
    : t('app.requestFailed');
}

export default function App() {
  const { t } = useLanguage();
  const tRef = useRef(t);
  tRef.current = t;
  const [loadState, setLoadState] = useState<LoadState>('loading');
  const [loadError, setLoadError] = useState('');
  const [projects, setProjects] = useState<Project[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [questionsOpen, setQuestionsOpen] = useState(false);
  const [sharedVersion, setSharedVersion] = useState(0);
  const [sessionRequests, setSessionRequests] = useState<SessionRequest[]>([]);
  const [requestsOpen, setRequestsOpen] = useState(false);
  const [agents, setAgents] = useState<AgentCheck[]>([]);
  const [agentsChecked, setAgentsChecked] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  const [tabsByProject, setTabsByProject] = useState<TabsByProject>({});
  const [docks, setDocks] = useState<Record<string, DockTab | null>>({});
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [projectDialogOpen, setProjectDialogOpen] = useState(false);
  const [projectSettingsId, setProjectSettingsId] = useState<string | null>(null);
  const [closeSessionId, setCloseSessionId] = useState<string | null>(null);
  const [renameSessionId, setRenameSessionId] = useState<string | null>(null);
  const [deleteSessionId, setDeleteSessionId] = useState<string | null>(null);
  const [switchSessionId, setSwitchSessionId] = useState<string | null>(null);
  const [sessionMenu, setSessionMenu] = useState<SessionMenuState | null>(null);
  const [branchMenu, setBranchMenu] = useState<BranchMenuState | null>(null);
  const [dialogProjectId, setDialogProjectId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [quitOpen, setQuitOpen] = useState(false);
  const [toast, setToast] = useState<ToastState | null>(null);
  const [eventsPaused, setEventsPaused] = useState(false);
  const [eventAttempt, setEventAttempt] = useState(0);
  const [announcement, setAnnouncement] = useState('');
  const [focusSessionChangeTick, setFocusSessionChangeTick] = useState(0);
  const tauri = isTauri();

  const mainRef = useRef<HTMLElement>(null);
  const focusMainRef = useRef(false);
  const sessionsRef = useRef<Session[]>([]);
  sessionsRef.current = sessions;
  const activeSessionIdRef = useRef<string | null>(null);
  const activeStatusRef = useRef<Session['status'] | null>(null);
  const allowCloseRef = useRef(false);
  const activeSessionCount = sessions.filter(
    (session) => session.status === 'active',
  ).length;

  function setDock(sessionId: string, tab: DockTab | null) {
    setDocks((current) => ({ ...current, [sessionId]: tab }));
  }

  function toggleDock(sessionId: string) {
    setDocks((current) => ({
      ...current,
      [sessionId]: current[sessionId] ? null : 'files',
    }));
  }

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
      setQuestions(data.questions ?? []);
      setSessionRequests(data.sessionRequests ?? []);
      setAgents(report?.agents ?? []);
      setAgentsChecked(report !== null);
      setVersion(report?.version ?? null);
      setActiveProjectId(
        data.sessions[0]?.projectId ?? data.projects[0]?.id ?? null,
      );
      setLoadState('ready');
    } catch (error) {
      setLoadError(messageFor(error, tRef.current));
      setLoadState('error');
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      const data = await bootstrap();
      setProjects(data.projects);
      setAccounts(data.accounts);
      setSessions(data.sessions);
      setQuestions(data.questions ?? []);
      setSessionRequests(data.sessionRequests ?? []);
    } catch {
      // The event stream reports a broken connection.
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
  const activeDock = activeSession
    ? docks[activeSession.id] ?? null
    : null;
  const activeQuestion = activeSession
    ? questions.find((question) => question.sessionId === activeSession.id) ??
      null
    : null;
  const activeRequest = activeSession
    ? sessionRequests.find(
        (request) => request.parentSessionId === activeSession.id,
      ) ?? null
    : null;
  const closeTarget = closeSessionId
    ? sessions.find((session) => session.id === closeSessionId) ?? null
    : null;
  const renameTarget = renameSessionId
    ? sessions.find((session) => session.id === renameSessionId) ?? null
    : null;
  const deleteTarget = deleteSessionId
    ? sessions.find((session) => session.id === deleteSessionId) ?? null
    : null;
  const switchTarget = switchSessionId
    ? sessions.find((session) => session.id === switchSessionId) ?? null
    : null;
  const projectSettingsTarget = projectSettingsId
    ? projects.find((project) => project.id === projectSettingsId) ?? null
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
    projectSettingsTarget !== null ||
    closeTarget !== null ||
    renameTarget !== null ||
    deleteTarget !== null ||
    switchTarget !== null ||
    settingsOpen ||
    questionsOpen ||
    requestsOpen ||
    quitOpen;

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
            setAnnouncement(
              tRef.current('app.announcementClosed', { name: session.name }),
            );
          } else if (
            !previous.waitingFor &&
            session.waitingFor &&
            session.id !== activeSessionIdRef.current
          ) {
            setAnnouncement(
              tRef.current('app.announcementWaiting', { name: session.name }),
            );
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
      () => {
        setEventsPaused(false);
        void refresh();
      },
    );
  }, [loadState, eventAttempt, refresh]);

  useEffect(() => {
    if (loadState !== 'ready') {
      return;
    }

    return subscribeAux((event) => {
      if (event.type === 'question.updated') {
        setQuestions((current) => {
          const others = current.filter(
            (question) => question.id !== event.question.id,
          );
          return event.question.status === 'pending'
            ? [...others, event.question]
            : others;
        });
        return;
      }
      if (event.type === 'request.updated') {
        setSessionRequests((current) => {
          const others = current.filter(
            (request) => request.id !== event.request.id,
          );
          return event.request.status === 'pending'
            ? [...others, event.request]
            : others;
        });
        return;
      }
      if (event.type === 'sharedfile.created') {
        setSharedVersion((value) => value + 1);
        return;
      }
      if (event.type === 'account.updated') {
        setAccounts((current) => {
          const index = current.findIndex(
            (account) => account.id === event.account.id,
          );
          if (index === -1) {
            return [...current, event.account];
          }
          const next = current.slice();
          next[index] = event.account;
          return next;
        });
        return;
      }
      setToast({
        message: `${event.notification.sessionName}: ${event.notification.message}`,
        error: false,
      });
    }, undefined, () => void refresh());
  }, [loadState, eventAttempt, refresh]);

  const quitNow = useCallback(async () => {
    allowCloseRef.current = true;
    try {
      await invoke('quit_app');
    } catch {
      await getCurrentWindow().destroy();
    }
  }, []);

  useEffect(() => {
    if (!tauri) {
      return;
    }

    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const requestQuit = () => {
      const running = sessionsRef.current.some(
        (session) => session.status === 'active',
      );
      if (running) {
        setQuitOpen(true);
      } else {
        void quitNow();
      }
    };

    void (async () => {
      try {
        const window = getCurrentWindow();
        const stopClose = await window.onCloseRequested((event) => {
          if (allowCloseRef.current) {
            return;
          }
          event.preventDefault();
          requestQuit();
        });
        const stopQuit = await listen('elesdee://quit-requested', requestQuit);

        if (disposed) {
          stopClose();
          stopQuit();
          return;
        }
        unlisteners.push(stopClose, stopQuit);
      } catch {
        // The Tauri window API is unavailable outside the desktop shell.
      }
    })();

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, [tauri, quitNow]);

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

  async function saveProjectDefaults(input: UpdateProjectDefaultsInput) {
    if (!projectSettingsTarget) {
      return;
    }

    const updated = await updateProjectDefaults(
      projectSettingsTarget.id,
      input,
    );
    setProjects((current) =>
      current.map((project) =>
        project.id === updated.id ? updated : project,
      ),
    );
    setToast({ message: t('app.projectDefaultsSaved'), error: false });
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

  async function runShell(action: () => Promise<void>) {
    try {
      await action();
    } catch (error) {
      const message =
        typeof error === 'string' && error.trim()
          ? error
          : error instanceof Error && error.message
            ? error.message
            : tRef.current('app.actionFailed');
      setToast({ message, error: true });
    }
  }

  function handleBranchAction(action: BranchMenuAction, session: Session) {
    switch (action) {
      case 'copy-branch':
        if (session.branch) {
          void copyText(session.branch, t('app.branchCopied'));
        }
        break;
      case 'copy-folder':
        if (session.folder) {
          void copyText(session.folder, t('app.folderCopied'));
        }
        break;
      case 'open-folder':
        void runShell(() => openFolder(session.folder));
        break;
      case 'open-terminal':
        void runShell(() => openTerminal(session.folder));
        break;
    }
  }

  async function copyText(value: string, message: string) {
    try {
      await navigator.clipboard.writeText(value);
      setToast({ message, error: false });
    } catch {
      setToast({ message: t('app.clipboardUnavailable'), error: true });
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
      setToast({ message: t('app.sessionReopened'), error: false });
    } catch (error) {
      setToast({
        message:
          error instanceof ApiError
            ? error.message
            : t('app.sessionNotReopened'),
        error: true,
      });
    }
  }

  async function handleResume(session: Session) {
    try {
      const resumed = await resumeSession(session.id);
      setSessions((current) =>
        current.map((item) => (item.id === resumed.id ? resumed : item)),
      );
      setToast({ message: t('app.sessionResumed'), error: false });
    } catch (error) {
      setToast({
        message:
          error instanceof ApiError
            ? error.message
            : t('app.sessionNotResumed'),
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
      case 'switch-account':
        setSwitchSessionId(session.id);
        break;
      case 'copy-folder':
        void copyText(session.folder, t('app.folderCopied'));
        break;
      case 'copy-branch':
        if (session.branch) {
          void copyText(session.branch, t('app.branchCopied'));
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

  async function performSwitch(accountId: string) {
    if (!switchTarget) {
      return;
    }

    const updated = await switchSessionAccount(switchTarget.id, accountId);
    setSessions((current) =>
      current.map((item) => (item.id === updated.id ? updated : item)),
    );
    setToast({ message: t('app.accountSwitched'), error: false });
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
          message: t('app.permissionResolved'),
          error: false,
        });
        return;
      }

      throw error;
    }
  }

  async function answerOpenQuestion(id: string, answer: string) {
    await answerQuestion(id, answer);
    setQuestions((current) =>
      current.filter((question) => question.id !== id),
    );
    setToast({ message: t('app.answerSent'), error: false });
  }

  async function approveRequest(id: string, allowFurther: boolean) {
    const result = await approveSessionRequest(id, allowFurther);
    setSessionRequests((current) =>
      current.filter((request) => request.id !== id),
    );
    if (result.session) {
      setSessions((current) =>
        current.some((session) => session.id === result.session?.id)
          ? current
          : [...current, result.session as Session],
      );
      openSession(result.session);
    }
    setToast({ message: t('app.sessionApproved'), error: false });
  }

  async function denyRequest(id: string) {
    await denySessionRequest(id);
    setSessionRequests((current) =>
      current.filter((request) => request.id !== id),
    );
    setToast({ message: t('app.requestDenied'), error: false });
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
    setDocks((current) => {
      const next = { ...current };
      delete next[deleteTarget.id];
      return next;
    });
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
              {loadState === 'loading'
                ? t('app.starting')
                : t('app.cannotReach')}
            </h1>
            <p className="empty-text">
              {loadState === 'loading'
                ? t('app.connecting')
                : loadError}
            </p>
            {loadState === 'error' ? (
              <button type="button" className="btn btn-primary boot-retry" onClick={() => void load()}>
                {t('common.retry')}
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
          {t('app.skipToSession')}
        </a>

        <header
          className={tauri ? 'titlebar is-tauri' : 'titlebar'}
          data-tauri-drag-region={tauri ? 'true' : undefined}
        >
          <div className="brand">
            <span className="brand-mark" aria-hidden="true" />
            elesdee
            {activeProject ? (
              <span className="titlebar-project">
                {' · '}
                {activeProject.name}
              </span>
            ) : null}
          </div>
          <div className="titlebar-spacer" />
          {projects.length > 0 ? (
            <button
              type="button"
              className="btn titlebar-new"
              onClick={() => openNewSession()}
            >
              <PlusIcon />
              {t('app.newSession')}
            </button>
          ) : null}
        </header>

        {eventsPaused ? (
          <div className="events-note">
            <span role="status">{t('app.liveUpdatesPaused')}</span>
            <button
              type="button"
              className="link-button"
              onClick={() => setEventAttempt((attempt) => attempt + 1)}
            >
              {t('common.retry')}
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
            onProjectSettings={(project) => setProjectSettingsId(project.id)}
            onOpenSettings={() => setSettingsOpen(true)}
            questions={questions.length}
            onOpenQuestions={() => setQuestionsOpen(true)}
            requests={sessionRequests.length}
            onOpenRequests={() => setRequestsOpen(true)}
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
                  panelOpen={activeDock !== null}
                  onTogglePanel={() => toggleDock(activeSession.id)}
                  onOpenSwitch={() => setSwitchSessionId(activeSession.id)}
                />
                <div
                  className={`session-workspace${activeDock ? ' has-dock' : ''}`}
                >
                  <SessionView
                    key={activeSession.id}
                    session={activeSession}
                    onRespondPermission={(decision) =>
                      answerPermission(activeSession, decision)
                    }
                    onReopen={() => void handleReopen(activeSession)}
                    onResume={() => void handleResume(activeSession)}
                    question={activeQuestion}
                    onAnswerQuestion={(answer) =>
                      activeQuestion
                        ? answerOpenQuestion(activeQuestion.id, answer)
                        : Promise.resolve()
                    }
                    request={activeRequest}
                    onApproveRequest={(allowFurther) =>
                      activeRequest
                        ? approveRequest(activeRequest.id, allowFurther)
                        : Promise.resolve()
                    }
                    onDenyRequest={() =>
                      activeRequest
                        ? denyRequest(activeRequest.id)
                        : Promise.resolve()
                    }
                  />
                  {activeDock ? (
                    <SessionSidePanel
                      key={activeSession.id}
                      session={activeSession}
                      tab={activeDock}
                      sharedVersion={sharedVersion}
                      onSelectTab={(tab) => setDock(activeSession.id, tab)}
                      onClose={() => setDock(activeSession.id, null)}
                    />
                  ) : null}
                </div>
              </div>
            ) : (
              <div className="empty-main">
                <div className="empty-card">
                  <div className="empty-mark">
                    <TerminalIcon />
                  </div>
                  <h1 className="empty-title">{t('app.chooseSession')}</h1>
                  <p className="empty-text">
                    {t('app.chooseSessionHint')}
                  </p>
                  <button
                    type="button"
                    className="btn btn-primary"
                    onClick={() => openNewSession()}
                  >
                    {t('app.newSession')}
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

      {projectSettingsTarget ? (
        <ProjectSettingsDialog
          project={projectSettingsTarget}
          accounts={accounts}
          onSave={saveProjectDefaults}
          onClose={() => setProjectSettingsId(null)}
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

      {switchTarget ? (
        <SwitchAccountDialog
          session={switchTarget}
          onSwitch={performSwitch}
          onClose={() => setSwitchSessionId(null)}
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

      {quitOpen ? (
        <QuitDialog
          sessionCount={activeSessionCount}
          onConfirm={quitNow}
          onClose={() => setQuitOpen(false)}
        />
      ) : null}

      {questionsOpen ? (
        <QuestionsDialog
          questions={questions}
          onAnswer={answerOpenQuestion}
          onClose={() => setQuestionsOpen(false)}
        />
      ) : null}

      {requestsOpen ? (
        <RequestsDialog
          requests={sessionRequests}
          onApprove={approveRequest}
          onDeny={denyRequest}
          onClose={() => setRequestsOpen(false)}
        />
      ) : null}

      {branchMenu && branchSession ? (
        <BranchActionsMenu
          session={branchSession}
          trigger={branchMenu.trigger}
          canOpen={tauri}
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
