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
  parentSessionId?: string | null;
  parentSessionName?: string | null;
  envOverrides?: EnvOverride[];
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
  defaultAccountId?: string | null;
  defaultSourceBranch?: string | null;
}

export interface UpdateProjectDefaultsInput {
  defaultAccountId?: string | null;
  defaultSourceBranch?: string | null;
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
  questions?: Question[];
  sessionRequests?: SessionRequest[];
}

export interface SessionRequest {
  id: string;
  parentSessionId: string;
  parentSessionName: string;
  projectId: string;
  projectName: string;
  name: string | null;
  folder: string;
  sourceBranch: string | null;
  firstPrompt: string | null;
  accountId: string;
  client: Client;
  status: 'pending' | 'approved' | 'denied';
  childSessionId: string | null;
  createdAt: string;
}

export interface ApproveSessionResult {
  request: SessionRequest;
  session: Session | null;
}

export interface Question {
  id: string;
  sessionId: string;
  sessionName: string;
  text: string;
  options: string[];
  status: 'pending' | 'answered' | 'withdrawn';
  answer: string | null;
  createdAt: string;
}

export interface Notification {
  id: string;
  sessionId: string;
  sessionName: string;
  message: string;
  createdAt: string;
}

export type AuxEvent =
  | { type: 'question.updated'; question: Question }
  | { type: 'request.updated'; request: SessionRequest }
  | { type: 'notification'; notification: Notification };

export interface HealthResponse {
  ok: boolean;
  version: string;
  agents: AgentCheck[];
}

export interface EnvOverride {
  name: string;
  value: string;
}

export interface CreateSessionInput {
  projectId: string;
  kind: SessionKind;
  accountId?: string;
  name?: string;
  folder?: string;
  sourceBranch?: string;
  firstPrompt?: string;
  env?: EnvOverride[];
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
