import { useRef, useState } from 'react';
import type { Account, Project, Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import type { Translate } from '../i18n';
import {
  sessionKindLabel,
  statusText,
  statusTone,
  tabStateLabel,
} from '../state/sessionStatus';
import { accountUsage } from '../state/usage';
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
  onNewSession: (projectId: string) => void;
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
  onSelect: (session: Session) => void;
  onMenu: SidebarProps['onSessionMenu'];
  t: Translate;
}

function SessionRow({
  session,
  isActive,
  projectName,
  onSelect,
  onMenu,
  t,
}: SessionRowProps) {
  const waiting = Boolean(session.waitingFor);
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
      title={statusText(session, t)}
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
  accounts,
  activeProjectId,
  activeSessionId,
  onSelectProject,
  onSelectSession,
  onSessionMenu,
  onAddProject,
  onNewSession,
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
  const usageButtonRef = useRef<HTMLButtonElement>(null);

  const hasProjects = projects.length > 0;
  const activeCount = sessions.filter(
    (session) => session.status === 'active',
  ).length;
  const projectNames = new Map(
    projects.map((project) => [project.id, project.name]),
  );

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
    <aside className="sidebar" aria-label={t('sidebar.sessions')}>
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

      <div className="session-list">
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
                onSelect={onSelectSession}
                onMenu={onSessionMenu}
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
                className={
                  isActiveProject ? 'project-group is-active' : 'project-group'
                }
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
                        onSelect={onSelectSession}
                        onMenu={onSessionMenu}
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

              {accounts.length === 0 ? (
                <span className="usage-summary-value">{t('usage.none')}</span>
              ) : (
                accounts.slice(0, 3).map((account) => {
                  const usage = accountUsage(account, language);
                  return (
                    <span
                      className="usage-summary-row"
                      key={account.id}
                      title={account.name}
                    >
                      <span className="usage-summary-label">
                        {account.name}
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
                accounts={accounts}
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
