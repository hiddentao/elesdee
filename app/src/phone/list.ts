/**
 * Data rules for the phone session list, shared by the browser layout and the
 * phone shell.
 *
 * A session has two states, Active and Closed. Working, waiting, paused and
 * done are details inside Active, and the Running filter selects the first two
 * only. Nothing here draws or translates text.
 */

export type PhoneFilter = 'all' | 'running' | 'waiting';

export interface FilterSession {
  status: 'active' | 'closed';
  workState: string | null;
  waitingFor: string | null;
}

export interface PreviewSession {
  preview?: string | null;
  projectId: string;
}

export interface ProjectRef {
  id: string;
}

export type StartBlocker = 'noProjects' | 'incomplete' | null;

export function isWaiting(session: Pick<FilterSession, 'waitingFor'>): boolean {
  return session.waitingFor !== null;
}

export function matchesPhoneFilter(
  session: FilterSession,
  filter: PhoneFilter,
): boolean {
  if (filter === 'waiting') {
    return isWaiting(session);
  }
  if (filter === 'all') {
    return true;
  }
  return (
    session.status === 'active' &&
    (isWaiting(session) ||
      session.workState === 'working' ||
      session.workState === 'waiting')
  );
}

/**
 * The line under a session name. A waiting session keeps the preview that
 * explains what it needs; the project name is the fallback.
 */
export function rowPreview(session: PreviewSession): string {
  return session.preview || session.projectId;
}

export function hasProjects(projects: readonly ProjectRef[]): boolean {
  return projects.length > 0;
}

/**
 * The reason a new session cannot start, or null when it can.
 */
export function startBlocker(
  projects: readonly ProjectRef[],
  projectId: string | null,
  configId: string | null,
): StartBlocker {
  if (!hasProjects(projects)) {
    return 'noProjects';
  }
  if (!projectId || !configId) {
    return 'incomplete';
  }
  return null;
}
