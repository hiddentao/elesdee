export type SessionState = 'active' | 'closed';
export type WorkState = 'working' | 'waiting' | 'input' | 'paused' | 'done';
export type WaitingFor = 'permission' | 'input' | 'start' | 'question';
export type SessionKind = 'agent' | 'terminal';
export type PermissionDecision = 'allow' | 'deny';
export type Client = 'Claude Code' | 'Codex' | 'OpenCode 2' | 'Antigravity';

export interface PendingPermission {
  requestId: string;
  tool: string;
  summary: string;
}

export const CLIENTS: readonly Client[] = [
  'Claude Code',
  'Codex',
  'OpenCode 2',
  'Antigravity',
];

export interface Shortcut {
  key: string;
  label: string;
  name: string;
  description: string;
}

export interface Session {
  id: string;
  name: string;
  short: string;
  projectId: string;
  kind: SessionKind;
  status: SessionState;
  workState: WorkState | null;
  client: Client | null;
  accountId: string | null;
  branch: string | null;
  folder: string;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  firstPrompt?: string | null;
  startedAt: string;
  shortcuts: Shortcut[];
}

export interface Branch {
  name: string;
  current: boolean;
}

export interface Project {
  id: string;
  name: string;
  folder: string;
}

export interface UsageRow {
  label: string;
  used?: number;
  value?: string;
  min?: string;
  warn?: boolean;
}

export interface Account {
  id: string;
  name: string;
  plan: string;
  client: Client;
  provider: string;
  auth: 'subscription' | 'key' | 'local';
  sample: boolean;
  rows: UsageRow[];
  foot: string;
  server: string | null;
  model: string | null;
  apiStyle?: string | null;
}

export interface ProviderInfo {
  id: string;
  label: string;
  client: Client;
  auth: Array<'subscription' | 'key' | 'local'>;
  defaultModel: string | null;
  custom: boolean;
}

export interface AgentCheck {
  name: Client;
  installed: boolean;
  version: string | null;
}

export interface BootstrapResponse {
  projects: Project[];
  accounts: Account[];
  sessions: Session[];
}

export interface HealthResponse {
  ok: boolean;
  version: string;
  agents: AgentCheck[];
}

export interface CreateSessionInput {
  projectId: string;
  kind: SessionKind;
  accountId?: string;
  name?: string;
  folder?: string;
  sourceBranch?: string;
  firstPrompt?: string;
}

export interface CreateProjectInput {
  name: string;
  folder: string;
}

export interface CreateAccountInput {
  name: string;
  providerId: string;
  client?: Client;
  auth: 'subscription' | 'key' | 'local';
  key?: string;
  server?: string;
  model?: string;
  apiStyle?: 'openai' | 'anthropic';
}

export interface ApiErrorPayload {
  error: string;
}

export interface OutputGap {
  after: number;
  oldestSeq: number;
  newestSeq: number;
}

export interface OutputChunk {
  seq: number;
  data: string;
  gap?: OutputGap;
}

export interface SessionUpdatedEvent {
  type: 'session.updated';
  session: Session;
}
