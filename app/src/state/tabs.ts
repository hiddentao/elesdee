export interface ProjectTabs {
  open: string[];
  active: string | null;
}

export type TabsByProject = Record<string, ProjectTabs>;

export function openSessionTab(
  tabs: TabsByProject,
  projectId: string,
  sessionId: string,
): TabsByProject {
  const current = tabs[projectId] ?? { open: [], active: null };
  const open = current.open.includes(sessionId)
    ? current.open
    : [...current.open, sessionId];

  if (open === current.open && current.active === sessionId) {
    return tabs;
  }

  return { ...tabs, [projectId]: { open, active: sessionId } };
}

export function closeSessionTab(
  tabs: TabsByProject,
  projectId: string,
  sessionId: string,
): TabsByProject {
  const current = tabs[projectId];
  if (!current) {
    return tabs;
  }

  const index = current.open.indexOf(sessionId);
  if (index === -1) {
    if (current.active !== sessionId) {
      return tabs;
    }
    return { ...tabs, [projectId]: { ...current, active: null } };
  }

  const open = current.open.filter((id) => id !== sessionId);
  const active =
    current.active === sessionId
      ? open[Math.min(index, open.length - 1)] ?? null
      : current.active;

  return { ...tabs, [projectId]: { open, active } };
}

/** Carries an open tab to the project that now groups its session. */
export function moveSessionTab(
  tabs: TabsByProject,
  fromProjectId: string,
  toProjectId: string,
  sessionId: string,
): TabsByProject {
  const from = tabs[fromProjectId];
  if (
    fromProjectId === toProjectId ||
    !from ||
    !from.open.includes(sessionId)
  ) {
    return tabs;
  }

  const wasActive = from.active === sessionId;
  const next = closeSessionTab(tabs, fromProjectId, sessionId);
  if (wasActive) {
    return openSessionTab(next, toProjectId, sessionId);
  }

  const target = next[toProjectId] ?? { open: [], active: null };
  return {
    ...next,
    [toProjectId]: {
      ...target,
      open: target.open.includes(sessionId)
        ? target.open
        : [...target.open, sessionId],
    },
  };
}
