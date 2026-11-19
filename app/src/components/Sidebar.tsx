import { useState } from 'react';
import type { Account, Project, Session } from '../api/types';
import {
  sessionKindLabel,
  statusText,
  statusTone,
  tabStateLabel,
} from '../state/sessionStatus';
import { accountUsage, tightestAccount } from '../state/usage';
import {
  AlertIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  LayersIcon,
  ListIcon,
  PlusIcon,
} from './icons';

type SessionView = 'grouped' | 'flat';
type Filter = 'active' | 'closed';

interface SidebarProps {
  projects: Project[];
  sessions: Session[];
  accounts: Account[];
  activeProjectId: string | null;
  activeSessionId: string | null;
  onSelectProject: (project: Project) => void;
  onSelectSession: (session: Session) => void;
  onSessionMenu: (
    session: Session,
    point: { x: number; y: number },
    trigger: HTMLElement,
  ) => void;
  onAddProject: () => void;
  onOpenUsage: () => void;
}

interface SessionRowProps {
  session: Session;
  isActive: boolean;
  projectName?: string;
  onSelect: (session: Session) => void;
  onMenu: SidebarProps['onSessionMenu'];
}

function SessionRow({
  session,
  isActive,
  projectName,
  onSelect,
  onMenu,
}: SessionRowProps) {
  const waiting = Boolean(session.waitingFor);
  const stateLabel =
    session.status === 'closed'
      ? 'Closed'
      : session.waitingFor
        ? null
        : tabStateLabel(session);
  const classes = [
    'session-row',
    isActive ? 'is-active' : '',
    session.status === 'closed' ? 'is-closed' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <button
      type="button"
      className={classes}
      aria-current={isActive ? 'true' : undefined}
      title={statusText(session)}
      onClick={() => onSelect(session)}
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
          {waiting ? (
            <>
              <AlertIcon className="waiting-icon" />
              Waiting for you
            </>
          ) : (
            session.preview || 'No output yet.'
          )}
        </span>
        <span className="session-meta">
          {sessionKindLabel(session)}
          {stateLabel ? ` · ${stateLabel}` : ''}
        </span>
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
  accounts,
  activeProjectId,
  activeSessionId,
  onSelectProject,
  onSelectSession,
  onSessionMenu,
  onAddProject,
  onOpenUsage,
}: SidebarProps) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [manualCollapsed, setManualCollapsed] = useState<Set<string>>(new Set());
  const [showActive, setShowActive] = useState(true);
  const [showClosed, setShowClosed] = useState(false);
  const [view, setView] = useState<SessionView>('grouped');

  const activeCount = sessions.filter((session) => session.status === 'active').length;
  const closedCount = sessions.filter((session) => session.status === 'closed').length;
  const tightest = tightestAccount(accounts);
  const tightestUsage = tightest ? accountUsage(tightest) : null;
  const projectNames = new Map(
    projects.map((project) => [project.id, project.name]),
  );

  const filtersOn = showActive || showClosed;
  const visibleSessions = sessions.filter((session) =>
    session.status === 'active' ? showActive : showClosed,
  );

  const allByProject = groupSessions(sessions);
  const byProject = groupSessions(visibleSessions);

  function matchesFilter(
    session: Session,
    active: boolean,
    closed: boolean,
  ): boolean {
    return session.status === 'active' ? active : closed;
  }

  function projectMatches(projectId: string): boolean {
    return sessions.some(
      (session) =>
        session.projectId === projectId &&
        matchesFilter(session, showActive, showClosed),
    );
  }

  function toggleFilter(filter: Filter) {
    const nextActive = filter === 'active' ? !showActive : showActive;
    const nextClosed = filter === 'closed' ? !showClosed : showClosed;
    const wasOn = showActive || showClosed;
    const nextOn = nextActive || nextClosed;

    if (!wasOn || !nextOn) {
      setManualCollapsed(new Set());
    }

    setShowActive(nextActive);
    setShowClosed(nextClosed);
  }

  function toggleProject(projectId: string) {
    const filtersOn = showActive || showClosed;
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
    if (all.length === 0 || showActive === showClosed) {
      return 'No sessions';
    }

    return showActive ? 'No active sessions' : 'No closed sessions';
  }

  return (
    <aside className="sidebar" aria-label="Sessions">
      <div className="sidebar-head">
        <h2 className="sidebar-title">
          Sessions <span className="count">{visibleSessions.length}</span>
        </h2>

        {projects.length > 0 ? (
          <div
            className="seg-mini"
            role="radiogroup"
            aria-label="Session list view"
          >
            <label className="seg-option" title="Grouped">
              <input
                type="radio"
                name="sidebar-view"
                value="grouped"
                checked={view === 'grouped'}
                onChange={() => setView('grouped')}
              />
              <span className="seg-face">
                <LayersIcon />
                <span className="sr-only">Grouped</span>
              </span>
            </label>
            <label className="seg-option" title="Flat">
              <input
                type="radio"
                name="sidebar-view"
                value="flat"
                checked={view === 'flat'}
                onChange={() => setView('flat')}
              />
              <span className="seg-face">
                <ListIcon />
                <span className="sr-only">Flat</span>
              </span>
            </label>
          </div>
        ) : null}

        {projects.length > 0 ? (
          <button
            type="button"
            className="sidebar-add"
            aria-label="Add project"
            title="Add project"
            onClick={onAddProject}
          >
            <PlusIcon />
          </button>
        ) : null}
      </div>

      <div className="sidebar-filters" role="group" aria-label="Session filter">
        <button
          type="button"
          className={showActive ? 'chip is-on' : 'chip'}
          aria-pressed={showActive}
          onClick={() => toggleFilter('active')}
        >
          Active <span className="chip-count">{activeCount}</span>
        </button>
        <button
          type="button"
          className={showClosed ? 'chip is-on' : 'chip'}
          aria-pressed={showClosed}
          onClick={() => toggleFilter('closed')}
        >
          Closed <span className="chip-count">{closedCount}</span>
        </button>
      </div>

      <div className="session-list">
        {projects.length === 0 ? (
          <p className="sidebar-empty">
            No projects yet. Sessions appear here after a project is added.
          </p>
        ) : sessions.length === 0 ? (
          <p className="sidebar-empty">No sessions yet. Start one from the top of the window.</p>
        ) : view === 'flat' ? (
          visibleSessions.length === 0 ? (
            <p className="sidebar-empty">No sessions match this filter.</p>
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
                onSelect={onSelectSession}
                onMenu={onSessionMenu}
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
                className={
                  isActiveProject ? 'project-group is-active' : 'project-group'
                }
              >
                <div className="project-head">
                  <button
                    type="button"
                    className="project-caret-button"
                    aria-label={`${isCollapsed ? 'Expand' : 'Collapse'} ${project.name}`}
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
                    aria-label={`${project.name}, ${projectSessions.length} ${
                      projectSessions.length === 1 ? 'session' : 'sessions'
                    }`}
                    title={`Show ${project.name} sessions`}
                    onClick={() => onSelectProject(project)}
                  >
                    <span className="project-name">{project.name}</span>
                    <span className="project-count">
                      {projectSessions.length}
                    </span>
                  </button>
                </div>

                {isCollapsed && blots.length > 0 ? (
                  <div
                    className="project-blots"
                    role="group"
                    aria-label={`${project.name} active sessions`}
                  >
                    {blots.map((session) => (
                      <button
                        key={session.id}
                        type="button"
                        className="project-blot"
                        data-status={statusTone(session)}
                        aria-label={`${session.name}, ${statusText(session)}`}
                        title={`${session.name} — ${statusText(session)}`}
                        onClick={() => onSelectSession(session)}
                      >
                        <span className="project-blot-dot" aria-hidden="true" />
                      </button>
                    ))}
                  </div>
                ) : null}

                <div
                  className="project-sessions"
                  id={`project-${project.id}-sessions`}
                  hidden={isCollapsed}
                >
                  {projectSessions.length === 0 ? (
                    <p className="project-empty">
                      {emptyText(project.id)}
                    </p>
                  ) : (
                    projectSessions.map((session) => (
                      <SessionRow
                        key={session.id}
                        session={session}
                        isActive={
                          isActiveProject && session.id === activeSessionId
                        }
                        onSelect={onSelectSession}
                        onMenu={onSessionMenu}
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
        <button
          type="button"
          className="usage-summary-button"
          aria-haspopup="dialog"
          onClick={onOpenUsage}
        >
          <span className="usage-summary-head">
            <span className="usage-summary-title">Usage</span>
            {tightest ? (
              <span className="usage-summary-account">{tightest.name}</span>
            ) : null}
          </span>

          {tightest && tightestUsage ? (
            <span className="usage-summary-row">
              {tightestUsage.tightestRow ? (
                <span className="usage-summary-label">
                  {tightestUsage.tightestRow.label}
                </span>
              ) : null}
              {tightestUsage.remaining !== null ? (
                <span
                  className={
                    tightestUsage.warn
                      ? 'usage-mini-bar is-warn'
                      : 'usage-mini-bar'
                  }
                  aria-hidden="true"
                >
                  <span
                    className="usage-mini-fill"
                    style={{ width: `${tightestUsage.remaining}%` }}
                  />
                </span>
              ) : null}
              <span
                className={
                  tightestUsage.warn
                    ? 'usage-summary-value is-warn'
                    : 'usage-summary-value'
                }
              >
                {tightestUsage.summary}
              </span>
            </span>
          ) : (
            <span className="usage-summary-value">No usage data</span>
          )}
        </button>
      </div>
    </aside>
  );
}
