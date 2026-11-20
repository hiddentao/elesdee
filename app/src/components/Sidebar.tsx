import { useEffect, useRef, useState } from 'react';
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import type { Project, Provider, Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import type { Translate } from '../i18n';
import {
  sessionKindLabel,
  statusText,
  statusTone,
  tabStateLabel,
} from '../state/sessionStatus';
import { providerUsage } from '../state/usage';
import {
  AgentIcon,
  AlertIcon,
  ChatIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  LayersIcon,
  ListIcon,
  PlusIcon,
  SettingsIcon,
} from './icons';
import { UsagePopover } from './UsagePopover';

type SessionView = 'grouped' | 'flat';
type Filter = 'active' | 'closed';

const DRAG_THRESHOLD = 6;

interface SidebarProps {
  projects: Project[];
  sessions: Session[];
  providers: Provider[];
  activeProjectId: string | null;
  activeSessionId: string | null;
  onSelectProject: (project: Project) => void;
  onSelectSession: (session: Session) => void;
  onSessionMenu: (
    session: Session,
    point: { x: number; y: number },
    trigger: HTMLElement,
  ) => void;
  onMoveSession: (session: Session, projectId: string) => Promise<boolean>;
  onAddProject: () => void;
  onNewSession: (projectId: string) => void;
  onProjectSettings: (project: Project) => void;
  onOpenSettings: () => void;
  questions?: number;
  onOpenQuestions?: () => void;
  requests?: number;
  onOpenRequests?: () => void;
}

interface SessionRowProps {
  session: Session;
  isActive: boolean;
  projectName?: string;
  canMove: boolean;
  dragging: boolean;
  onSelect: (session: Session) => void;
  onMenu: SidebarProps['onSessionMenu'];
  onPointerDown: (session: Session, event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  t: Translate;
}

interface DragState {
  sessionId: string;
  sessionName: string;
  pointerId: number;
  startX: number;
  startY: number;
  active: boolean;
  targetProjectId: string | null;
}

interface KeyboardMove {
  sessionId: string;
  targetProjectId: string;
}

function SessionRow({
  session,
  isActive,
  projectName,
  canMove,
  dragging,
  onSelect,
  onMenu,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onPointerCancel,
  t,
}: SessionRowProps) {
  const waiting = Boolean(session.waitingFor);
  const classes = [
    'session-row',
    isActive ? 'is-active' : '',
    session.status === 'closed' ? 'is-closed' : '',
    dragging ? 'is-dragging' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <button
      type="button"
      className={classes}
      data-session-id={session.id}
      aria-current={isActive ? 'true' : undefined}
      aria-keyshortcuts={canMove ? 'M' : undefined}
      title={t('session.tabLabel', {
        name: session.name,
        status: statusText(session, t),
      })}
      onClick={() => onSelect(session)}
      onPointerDown={(event) => onPointerDown(session, event)}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(
          session,
          { x: event.clientX, y: event.clientY },
          event.currentTarget,
        );
      }}
    >
      <span
        className="status-marker"
        data-status={statusTone(session)}
        aria-hidden="true"
      />
      <span className="session-text">
        <span className="session-name">{session.name}</span>
        <span className={waiting ? 'session-sub is-waiting' : 'session-sub'}>
          {projectName ? (
            <>
              <span className="session-project">{projectName}</span>
              {' · '}
            </>
          ) : null}
          {session.parentSessionName ? (
            <span className="session-parent">
              {t('sidebar.from', { name: session.parentSessionName })} ·{' '}
            </span>
          ) : null}
          {waiting ? (
            <>
              <AlertIcon className="waiting-icon" />
              {t('status.waitingForYou')}
            </>
          ) : (
            session.preview || t('sidebar.noOutputYet')
          )}
        </span>
        <span className="sr-only">
          {sessionKindLabel(session, t)} · {tabStateLabel(session, t)}
        </span>
      </span>
      <span className="session-status-label">
        {session.status === 'closed' ? t('status.closed') : t('status.active')}
      </span>
    </button>
  );
}

function groupSessions(sessions: Session[]): Map<string, Session[]> {
  const map = new Map<string, Session[]>();
  for (const session of sessions) {
    const list = map.get(session.projectId) ?? [];
    list.push(session);
    map.set(session.projectId, list);
  }
  return map;
}

export function Sidebar({
  projects,
  sessions,
  providers,
  activeProjectId,
  activeSessionId,
  onSelectProject,
  onSelectSession,
  onSessionMenu,
  onMoveSession,
  onAddProject,
  onNewSession,
  onProjectSettings,
  onOpenSettings,
  questions = 0,
  onOpenQuestions,
  requests = 0,
  onOpenRequests,
}: SidebarProps) {
  const { t, language } = useLanguage();
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [manualCollapsed, setManualCollapsed] = useState<Set<string>>(new Set());
  const [activeOnly, setActiveOnly] = useState(false);
  const [showClosed, setShowClosed] = useState(false);
  const [view, setView] = useState<SessionView>('grouped');
  const [usageOpen, setUsageOpen] = useState(false);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [keyboardMove, setKeyboardMove] = useState<KeyboardMove | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const usageButtonRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const suppressClickRef = useRef(false);

  const hasProjects = projects.length > 0;
  const canMoveSessions = view === 'grouped' && projects.length > 1;
  const activeCount = sessions.filter(
    (session) => session.status === 'active',
  ).length;
  const projectNames = new Map(
    projects.map((project) => [project.id, project.name]),
  );
  const sessionById = new Map(sessions.map((session) => [session.id, session]));

  function setDragState(next: DragState | null) {
    dragRef.current = next;
    setDrag(next);
  }

  function otherProjects(sessionId: string): Project[] {
    const source = sessionById.get(sessionId)?.projectId;
    return projects.filter((project) => project.id !== source);
  }

  function projectAtPoint(x: number, y: number): string | null {
    const element = document.elementFromPoint(x, y);
    const group = element?.closest<HTMLElement>('[data-project-id]');
    return group?.dataset.projectId ?? null;
  }

  async function commitMove(session: Session, projectId: string) {
    const projectName = projectNames.get(projectId) ?? projectId;
    const moved = await onMoveSession(session, projectId);
    setAnnouncement(
      moved
        ? t('sidebar.moved', { name: session.name, project: projectName })
        : t('sidebar.moveFailed', { name: session.name }),
    );
  }

  function cancelDrag() {
    if (!dragRef.current) {
      return;
    }
    setDragState(null);
    setAnnouncement(t('sidebar.moveCancelled'));
  }

  function selectSession(session: Session) {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    onSelectSession(session);
  }

  function handleRowPointerDown(
    session: Session,
    event: ReactPointerEvent<HTMLButtonElement>,
  ) {
    if (event.button !== 0 || !canMoveSessions) {
      return;
    }
    suppressClickRef.current = false;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDragState({
      sessionId: session.id,
      sessionName: session.name,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      active: false,
      targetProjectId: null,
    });
  }

  function handleRowPointerMove(
    event: ReactPointerEvent<HTMLButtonElement>,
  ) {
    const current = dragRef.current;
    if (!current || current.pointerId !== event.pointerId) {
      return;
    }

    let next = current;
    if (!next.active) {
      const distance = Math.hypot(
        event.clientX - next.startX,
        event.clientY - next.startY,
      );
      if (distance < DRAG_THRESHOLD) {
        return;
      }
      next = { ...next, active: true };
      setAnnouncement(t('sidebar.moveStarted', { name: next.sessionName }));
    }

    event.preventDefault();
    const list = listRef.current;
    if (list) {
      const rect = list.getBoundingClientRect();
      if (event.clientY < rect.top + 40) {
        list.scrollBy?.(0, -24);
      } else if (event.clientY > rect.bottom - 40) {
        list.scrollBy?.(0, 24);
      }
    }
    const target = projectAtPoint(event.clientX, event.clientY);
    const session = sessionById.get(next.sessionId);
    const targetProjectId =
      target && target !== session?.projectId ? target : null;
    if (
      !current.active ||
      targetProjectId !== current.targetProjectId
    ) {
      setDragState({ ...next, targetProjectId });
    }
  }

  function handleRowPointerUp(event: ReactPointerEvent<HTMLButtonElement>) {
    const current = dragRef.current;
    if (!current || current.pointerId !== event.pointerId) {
      return;
    }
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    setDragState(null);
    if (!current.active) {
      return;
    }

    suppressClickRef.current = true;
    const session = sessionById.get(current.sessionId);
    if (session && current.targetProjectId) {
      void commitMove(session, current.targetProjectId);
    } else {
      setAnnouncement(t('sidebar.moveCancelled'));
    }
  }

  function handleRowPointerCancel(event: ReactPointerEvent<HTMLButtonElement>) {
    if (dragRef.current?.pointerId !== event.pointerId) {
      return;
    }
    cancelDrag();
  }

  useEffect(() => {
    if (!drag?.active) {
      return;
    }

    function handleEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault();
        cancelDrag();
      }
    }

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [drag?.active, t]);

  function startKeyboardMove(session: Session) {
    const targets = otherProjects(session.id);
    if (targets.length === 0) {
      return;
    }
    setKeyboardMove({
      sessionId: session.id,
      targetProjectId: targets[0].id,
    });
    setAnnouncement(
      t('sidebar.moveInstructions', {
        name: session.name,
        project: projectNames.get(targets[0].id) ?? targets[0].id,
      }),
    );
  }

  function stepKeyboardMove(direction: number) {
    if (!keyboardMove) {
      return;
    }
    const targets = otherProjects(keyboardMove.sessionId);
    if (targets.length === 0) {
      return;
    }
    const index = targets.findIndex(
      (project) => project.id === keyboardMove.targetProjectId,
    );
    const target =
      targets[(index + direction + targets.length) % targets.length];
    setKeyboardMove({ ...keyboardMove, targetProjectId: target.id });
    setAnnouncement(
      t('sidebar.moveTarget', {
        project: projectNames.get(target.id) ?? target.id,
      }),
    );
  }

  function cancelKeyboardMove() {
    setKeyboardMove(null);
    setAnnouncement(t('sidebar.moveCancelled'));
  }

  async function confirmKeyboardMove() {
    if (!keyboardMove) {
      return;
    }
    const session = sessionById.get(keyboardMove.sessionId);
    const projectId = keyboardMove.targetProjectId;
    setKeyboardMove(null);
    if (!session) {
      return;
    }
    await commitMove(session, projectId);
    window.setTimeout(() => {
      const rows = document.querySelectorAll<HTMLElement>('[data-session-id]');
      for (const row of rows) {
        if (row.dataset.sessionId === session.id) {
          row.focus();
          return;
        }
      }
    }, 0);
  }

  function handleKeyDown(event: ReactKeyboardEvent<HTMLElement>) {
    if (keyboardMove) {
      switch (event.key) {
        case 'Escape':
          event.preventDefault();
          cancelKeyboardMove();
          return;
        case 'Tab':
          setKeyboardMove(null);
          return;
        case 'ArrowDown':
          event.preventDefault();
          stepKeyboardMove(1);
          return;
        case 'ArrowUp':
          event.preventDefault();
          stepKeyboardMove(-1);
          return;
        case 'Enter':
        case ' ':
          event.preventDefault();
          void confirmKeyboardMove();
          return;
        default:
          return;
      }
    }

    if (
      event.key.toLowerCase() !== 'm' ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      !canMoveSessions ||
      dragRef.current
    ) {
      return;
    }
    const row = (event.target as HTMLElement).closest<HTMLElement>(
      '[data-session-id]',
    );
    const session = row?.dataset.sessionId
      ? sessionById.get(row.dataset.sessionId)
      : undefined;
    if (!session) {
      return;
    }
    event.preventDefault();
    startKeyboardMove(session);
  }

  function isDropTarget(projectId: string): boolean {
    if (drag?.active) {
      return drag.targetProjectId === projectId;
    }
    return keyboardMove?.targetProjectId === projectId;
  }

  const showClosedSessions = showClosed && !activeOnly;
  const filtersOn = activeOnly || showClosed;

  function isVisible(session: Session): boolean {
    return session.status === 'active' || showClosedSessions;
  }

  const visibleSessions = sessions.filter(isVisible);
  const allByProject = groupSessions(sessions);
  const byProject = groupSessions(visibleSessions);

  function projectMatches(projectId: string): boolean {
    return sessions.some(
      (session) => session.projectId === projectId && isVisible(session),
    );
  }

  function toggleFilter(filter: Filter) {
    const nextActiveOnly = filter === 'active' ? !activeOnly : activeOnly;
    const nextShowClosed = filter === 'closed' ? !showClosed : showClosed;
    const wasOn = activeOnly || showClosed;
    const nextOn = nextActiveOnly || nextShowClosed;

    if (!wasOn || !nextOn) {
      setManualCollapsed(new Set());
    }

    setActiveOnly(nextActiveOnly);
    setShowClosed(nextShowClosed);
  }

  function toggleProject(projectId: string) {
    const matching = filtersOn && projectMatches(projectId);
    const autoExpanded = matching && !manualCollapsed.has(projectId);
    const isCollapsed = Boolean(collapsed[projectId]) && !autoExpanded;
    const nextCollapsed = !isCollapsed;

    setCollapsed((current) => ({ ...current, [projectId]: nextCollapsed }));

    if (nextCollapsed && matching) {
      setManualCollapsed((current) => {
        if (current.has(projectId)) {
          return current;
        }
        const next = new Set(current);
        next.add(projectId);
        return next;
      });
    } else {
      setManualCollapsed((current) => {
        if (!current.has(projectId)) {
          return current;
        }
        const next = new Set(current);
        next.delete(projectId);
        return next;
      });
    }
  }

  function emptyText(projectId: string): string {
    const all = allByProject.get(projectId) ?? [];
    if (all.length === 0) {
      return t('sidebar.noSessions');
    }

    return t('sidebar.noActiveSessionsShort');
  }

  return (
    <aside
      className="sidebar"
      aria-label={t('sidebar.sessions')}
      onKeyDown={handleKeyDown}
    >
      <p className="sr-only" role="status" aria-live="polite">
        {announcement}
      </p>
      <div className="sidebar-head">
        <h2 className="sidebar-title">
          {t('sidebar.sessions')}{' '}
          <span className="count">{visibleSessions.length}</span>
        </h2>

        {hasProjects ? (
          <div
            className="seg-mini"
            role="radiogroup"
            aria-label={t('sidebar.sessionList')}
          >
            <label className="seg-option" title={t('sidebar.grouped')}>
              <input
                type="radio"
                name="sidebar-view"
                value="grouped"
                checked={view === 'grouped'}
                onChange={() => setView('grouped')}
              />
              <span className="seg-face">
                <LayersIcon />
                <span className="sr-only">{t('sidebar.grouped')}</span>
              </span>
            </label>
            <label className="seg-option" title={t('sidebar.flat')}>
              <input
                type="radio"
                name="sidebar-view"
                value="flat"
                checked={view === 'flat'}
                onChange={() => setView('flat')}
              />
              <span className="seg-face">
                <ListIcon />
                <span className="sr-only">{t('sidebar.flat')}</span>
              </span>
            </label>
          </div>
        ) : null}

        {hasProjects ? (
          <button
            type="button"
            className="sidebar-add"
            aria-label={t('sidebar.addProject')}
            title={t('sidebar.addProject')}
            onClick={onAddProject}
          >
            <PlusIcon />
          </button>
        ) : null}
      </div>

      {questions > 0 || requests > 0 ? (
        <div className="sidebar-questions">
          {questions > 0 ? (
            <button
              type="button"
              className="chip questions-chip"
              onClick={onOpenQuestions}
            >
              <ChatIcon />
              {t('sidebar.questions')}{' '}
              <span className="chip-count">{questions}</span>
            </button>
          ) : null}
          {requests > 0 ? (
            <button
              type="button"
              className="chip questions-chip"
              onClick={onOpenRequests}
            >
              <AgentIcon />
              {t('sidebar.requests')}{' '}
              <span className="chip-count">{requests}</span>
            </button>
          ) : null}
        </div>
      ) : null}

      {hasProjects ? (
        <div
          className="sidebar-filters"
          role="group"
          aria-label={t('sidebar.filter')}
        >
          <button
            type="button"
            className={activeOnly ? 'chip is-on' : 'chip'}
            aria-pressed={activeOnly}
            onClick={() => toggleFilter('active')}
          >
            {t('sidebar.active')}{' '}
            <span className="chip-count">{activeCount}</span>
          </button>
          <button
            type="button"
            className={showClosed ? 'chip is-on' : 'chip'}
            aria-pressed={showClosed}
            onClick={() => toggleFilter('closed')}
          >
            {t('sidebar.showClosed')}
          </button>
        </div>
      ) : null}

      <div className="session-list" ref={listRef}>
        {!hasProjects ? (
          <p className="sidebar-empty">{t('sidebar.noSessionsYet')}</p>
        ) : view === 'flat' ? (
          sessions.length === 0 ? (
            <p className="sidebar-empty">{t('sidebar.noSessionsHint')}</p>
          ) : visibleSessions.length === 0 ? (
            <p className="sidebar-empty">{t('sidebar.noActiveSessions')}</p>
          ) : (
            visibleSessions.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                isActive={
                  session.projectId === activeProjectId &&
                  session.id === activeSessionId
                }
                projectName={
                  projectNames.get(session.projectId) ?? session.projectId
                }
                canMove={false}
                dragging={false}
                onSelect={selectSession}
                onMenu={onSessionMenu}
                onPointerDown={handleRowPointerDown}
                onPointerMove={handleRowPointerMove}
                onPointerUp={handleRowPointerUp}
                onPointerCancel={handleRowPointerCancel}
                t={t}
              />
            ))
          )
        ) : (
          projects.map((project) => {
            const projectSessions = byProject.get(project.id) ?? [];
            const autoExpanded =
              filtersOn &&
              !manualCollapsed.has(project.id) &&
              projectMatches(project.id);
            const isCollapsed =
              Boolean(collapsed[project.id]) && !autoExpanded;
            const isActiveProject = project.id === activeProjectId;
            const blots = projectSessions.filter(
              (session) => session.status === 'active',
            );

            return (
              <div
                key={project.id}
                className={[
                  'project-group',
                  isActiveProject ? 'is-active' : '',
                  isDropTarget(project.id) ? 'is-drop-target' : '',
                ]
                  .filter(Boolean)
                  .join(' ')}
                data-project-id={project.id}
              >
                <div className="project-head">
                  <button
                    type="button"
                    className="project-caret-button"
                    aria-label={
                      isCollapsed
                        ? t('sidebar.expand', { name: project.name })
                        : t('sidebar.collapse', { name: project.name })
                    }
                    aria-expanded={!isCollapsed}
                    aria-controls={`project-${project.id}-sessions`}
                    onClick={() => toggleProject(project.id)}
                  >
                    <span className="project-caret" aria-hidden="true">
                      {isCollapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
                    </span>
                  </button>

                  <button
                    type="button"
                    className="project-toggle"
                    aria-current={isActiveProject ? 'true' : undefined}
                    aria-label={t(
                      projectSessions.length === 1
                        ? 'sidebar.projectSessionsOne'
                        : 'sidebar.projectSessionsMany',
                      {
                        name: project.name,
                        count: projectSessions.length,
                      },
                    )}
                    title={t('sidebar.showProjectSessions', {
                      name: project.name,
                    })}
                    onClick={() => onSelectProject(project)}
                  >
                    <span className="project-name">{project.name}</span>
                    <span className="project-count">
                      {projectSessions.length}
                    </span>
                  </button>

                  <button
                    type="button"
                    className="project-add"
                    aria-label={t('sidebar.projectSettingsFor', {
                      name: project.name,
                    })}
                    title={t('sidebar.projectSettings')}
                    onClick={() => onProjectSettings(project)}
                  >
                    <SettingsIcon />
                  </button>

                  <button
                    type="button"
                    className="project-add"
                    aria-label={t('sidebar.newSessionIn', {
                      name: project.name,
                    })}
                    title={t('sidebar.newSession')}
                    onClick={() => onNewSession(project.id)}
                  >
                    <PlusIcon />
                  </button>
                </div>

                {isCollapsed && blots.length > 0 ? (
                  <div
                    className="project-blots"
                    role="group"
                    aria-label={t(
                      blots.length === 1
                        ? 'sidebar.activeSessionsOne'
                        : 'sidebar.activeSessionsMany',
                      { name: project.name, count: blots.length },
                    )}
                  >
                    {blots.map((session) => {
                      const waiting = Boolean(session.waitingFor);
                      return (
                        <button
                          key={session.id}
                          type="button"
                          className="project-blot"
                          data-status={statusTone(session)}
                          aria-label={`${session.name}, ${
                            waiting
                              ? t('status.waitingForYou')
                              : t('sidebar.noAttentionNeeded')
                          }`}
                          onClick={() => onSelectSession(session)}
                        >
                          <span
                            className="project-blot-dot"
                            aria-hidden="true"
                          />
                        </button>
                      );
                    })}
                    <span className="project-blots-count" aria-hidden="true">
                      {blots.length}
                    </span>
                    <span
                      className="project-blots-tip"
                      role="tooltip"
                      aria-hidden="true"
                    >
                      <span className="blot-tip-title">{project.name}</span>
                      {blots.map((session) => {
                        const waiting = Boolean(session.waitingFor);
                        return (
                          <span className="blot-tip-row" key={session.id}>
                            <span className="blot-tip-name">
                              {session.name}
                            </span>
                            <span
                              className={
                                waiting
                                  ? 'blot-tip-state is-waiting'
                                  : 'blot-tip-state'
                              }
                            >
                              {waiting
                                ? t('status.waitingForYou')
                                : t('sidebar.noAttentionNeeded')}
                            </span>
                          </span>
                        );
                      })}
                    </span>
                  </div>
                ) : null}

                <div
                  className="project-sessions"
                  id={`project-${project.id}-sessions`}
                  hidden={isCollapsed}
                >
                  {projectSessions.length === 0 ? (
                    <p className="project-empty">{emptyText(project.id)}</p>
                  ) : (
                    projectSessions.map((session) => (
                      <SessionRow
                        key={session.id}
                        session={session}
                        isActive={
                          isActiveProject && session.id === activeSessionId
                        }
                        canMove={canMoveSessions}
                        dragging={
                          drag?.active === true && drag.sessionId === session.id
                        }
                        onSelect={selectSession}
                        onMenu={onSessionMenu}
                        onPointerDown={handleRowPointerDown}
                        onPointerMove={handleRowPointerMove}
                        onPointerUp={handleRowPointerUp}
                        onPointerCancel={handleRowPointerCancel}
                        t={t}
                      />
                    ))
                  )}
                </div>
              </div>
            );
          })
        )}
      </div>

      <div className="sidebar-foot">
        {hasProjects ? (
          <>
            <button
              ref={usageButtonRef}
              type="button"
              className="usage-summary-button"
              aria-haspopup="dialog"
              aria-expanded={usageOpen}
              onClick={() => setUsageOpen((open) => !open)}
            >
              <span className="usage-summary-head">
                <span className="usage-summary-title">
                  {t('sidebar.usage')}
                </span>
              </span>

              {providers.length === 0 ? (
                <span className="usage-summary-value">{t('usage.none')}</span>
              ) : (
                providers.slice(0, 3).map((provider) => {
                  const usage = providerUsage(provider, language);
                  return (
                    <span
                      className="usage-summary-row"
                      key={provider.id}
                      title={provider.name}
                    >
                      <span className="usage-summary-label">
                        {provider.name}
                      </span>
                      {usage.remaining !== null ? (
                        <span
                          className={
                            usage.warn
                              ? 'usage-mini-bar is-warn'
                              : 'usage-mini-bar'
                          }
                          aria-hidden="true"
                        >
                          <span
                            className="usage-mini-fill"
                            style={{ width: `${usage.remaining}%` }}
                          />
                        </span>
                      ) : null}
                      <span
                        className={
                          usage.warn
                            ? 'usage-summary-value is-warn'
                            : 'usage-summary-value'
                        }
                      >
                        {usage.summary}
                      </span>
                    </span>
                  );
                })
              )}
            </button>

            {usageOpen ? (
              <UsagePopover
                providers={providers}
                trigger={usageButtonRef.current}
                onClose={() => setUsageOpen(false)}
              />
            ) : null}
          </>
        ) : null}

        <button
          type="button"
          className="sidebar-settings"
          onClick={onOpenSettings}
        >
          <SettingsIcon />
          {t('sidebar.settings')}
        </button>
      </div>
    </aside>
  );
}
