/**
 * Data rules for the phone session actions: start, close, switch and use.
 */

export interface ConfigLike {
  id: string;
  name: string;
  providerId: string;
  client: string;
  model?: string | null;
}

export interface SessionLike {
  id: string;
  name: string;
  kind: 'agent' | 'terminal';
  status: 'active' | 'closed';
  client?: string | null;
  configId?: string | null;
  allowedConfigIds?: string[];
  pendingPermission?: unknown | null;
}

export interface ProviderLike {
  id: string;
  name: string;
  plan?: string;
  sample?: boolean;
  rows?: UsageRowLike[];
  foot?: string;
}

export interface UsageRowLike {
  label: string;
  window?: string | null;
  used?: number;
  value?: string;
  min?: string;
  warn?: boolean;
  resetsAt?: number | null;
}

export interface QuestionLike {
  id: string;
  sessionId: string;
  status: string;
}

export interface RequestLike {
  id: string;
  parentSessionId: string;
  status: string;
}

export interface StartFields {
  projectId: string | null;
  configId: string | null;
  sourceBranch?: string | null;
  name?: string;
  firstPrompt?: string;
}

/** A readable label for an AI config in a picker. */
export function configChoiceLabel(config: ConfigLike): string {
  return `${config.name} · ${config.client}`;
}

/** Configs the session may switch to, without the current config. */
export function switchableConfigs<T extends ConfigLike>(
  configs: readonly T[],
  session: SessionLike,
): T[] {
  const allowed = new Set(session.allowedConfigIds ?? []);
  return configs.filter(
    (config) => allowed.has(config.id) && config.id !== session.configId,
  );
}

/** Filters a picker by a typed query across the useful text fields. */
export function filterChoices<T extends { id: string }>(
  items: readonly T[],
  query: string,
  label: (item: T) => string,
): T[] {
  const value = query.trim().toLowerCase();
  if (!value) {
    return [...items];
  }
  return items.filter(
    (item) =>
      label(item).toLowerCase().includes(value) ||
      item.id.toLowerCase().includes(value),
  );
}

/**
 * Builds the create-session request. Empty optional values are left out so
 * the computer applies its own project defaults.
 */
export function buildStartInput(
  fields: StartFields,
): { projectId: string; configId: string; kind: 'agent'; sourceBranch?: string; name?: string; firstPrompt?: string } | null {
  if (!fields.projectId || !fields.configId) {
    return null;
  }
  const input: {
    projectId: string;
    configId: string;
    kind: 'agent';
    sourceBranch?: string;
    name?: string;
    firstPrompt?: string;
  } = {
    projectId: fields.projectId,
    configId: fields.configId,
    kind: 'agent',
  };
  const branch = fields.sourceBranch?.trim();
  if (branch) {
    input.sourceBranch = branch;
  }
  const name = fields.name?.trim();
  if (name) {
    input.name = name;
  }
  const prompt = fields.firstPrompt?.trim();
  if (prompt) {
    input.firstPrompt = prompt;
  }
  return input;
}

/** The confirmation body key for the session kind. */
export function closeBodyKey(kind: 'agent' | 'terminal'): 'close.agentBody' | 'close.terminalBody' {
  return kind === 'agent' ? 'close.agentBody' : 'close.terminalBody';
}

/** The pending question for one session, if any. */
export function pendingQuestion<T extends QuestionLike>(
  questions: readonly T[],
  sessionId: string,
): T | null {
  return (
    questions.find(
      (question) =>
        question.sessionId === sessionId && question.status === 'pending',
    ) ?? null
  );
}

/** The pending agent-started session request for one session, if any. */
export function pendingRequest<T extends RequestLike>(
  requests: readonly T[],
  parentSessionId: string,
): T | null {
  return (
    requests.find(
      (request) =>
        request.parentSessionId === parentSessionId &&
        request.status === 'pending',
    ) ?? null
  );
}

/** A one-line usage summary for a provider, or null when it has no rows. */
export function usageSummary(provider: ProviderLike): string | null {
  const rows = provider.rows ?? [];
  if (rows.length === 0) {
    return null;
  }
  return rows
    .map((row) => {
      if (typeof row.used === 'number') {
        return `${row.label} ${row.used}%`;
      }
      if (row.value) {
        return `${row.label} ${row.value}`;
      }
      return row.label;
    })
    .join(' · ');
}

/** True when the switch control applies to the session. */
export function canSwitch(session: SessionLike): boolean {
  return (
    session.kind === 'agent' &&
    session.status === 'active' &&
    (session.allowedConfigIds?.length ?? 0) > 1
  );
}

/** True when a permission card applies to the session. */
export function hasPermission(session: SessionLike): boolean {
  return session.pendingPermission !== null && session.pendingPermission !== undefined;
}
