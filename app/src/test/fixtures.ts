import type { Project, Session } from '../api/types';

export const coreProject: Project = {
  id: 'p1',
  name: 'core',
  folder: '/dev/core',
};

export function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 's1',
    name: 'Session',
    short: 'session',
    projectId: 'p1',
    kind: 'agent',
    status: 'active',
    workState: 'working',
    client: 'Claude Code',
    accountId: 'a1',
    branch: null,
    folder: '/dev/web',
    preview: 'Ready',
    waitingFor: null,
    pendingPermission: null,
    demoServers: [],
    startedAt: '2026-11-19T10:00:00Z',
    shortcuts: [],
    ...overrides,
  };
}
