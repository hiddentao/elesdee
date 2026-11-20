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
  handoffNote?: string | null;
  folder: string;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  firstPrompt?: string | null;
  parentSessionId?: string | null;
  parentSessionName?: string | null;
  envOverrides?: EnvOverride[];
  presetId?: string | null;
  model?: string | null;
  config?: Record<string, string>;
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

export interface Preset {
  id: string;
  name: string;
  accountId: string;
  model: string | null;
  env: EnvOverride[];
  config: Record<string, string>;
  createdAt: string;
  updatedAt: string;
}

export interface LaunchPreset {
  id: string;
  name: string;
  accountId: string;
  model: string | null;
  env: EnvOverride[];
  config: Record<string, string>;
  stored: boolean;
  createdAt: string | null;
  updatedAt: string | null;
}

export interface CreatePresetInput {
  name?: string;
  accountId: string;
  model?: string | null;
  env?: EnvOverride[];
  config?: Record<string, string>;
}

export interface UpdatePresetInput {
  name?: string;
  accountId?: string;
  model?: string | null;
  env?: EnvOverride[];
  config?: Record<string, string>;
}

export interface BootstrapResponse {
  projects: Project[];
  accounts: Account[];
  presets?: Preset[];
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
  | { type: 'sharedfile.created'; file: SharedFile }
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

export interface FileEntry {
  name: string;
  path: string;
  directory: boolean;
}

export interface FileList {
  path: string;
  entries: FileEntry[];
  truncated: boolean;
}

export interface FileContent {
  path: string;
  content: string;
  size: number;
}

export interface SharedFile {
  id: string;
  sessionId: string;
  sessionName: string;
  name: string;
  path: string;
  caption: string | null;
  size: number;
  contentType: string;
  image: boolean;
  createdAt: string;
}

export type ChangeStatus =
  | 'added'
  | 'modified'
  | 'deleted'
  | 'renamed'
  | 'untracked'
  | 'conflicted';

export interface ChangedFile {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
  additions: number;
  deletions: number;
  binary: boolean;
  diff: string;
  diffTruncated: boolean;
}

export interface SessionChanges {
  git: boolean;
  base: string | null;
  files: ChangedFile[];
  truncated: boolean;
  error: string | null;
}

export interface CreateSessionInput {
  projectId: string;
  kind: SessionKind;
  accountId?: string;
  presetId?: string;
  name?: string;
  folder?: string;
  sourceBranch?: string;
  copyUncommittedChanges?: boolean;
  firstPrompt?: string;
  env?: EnvOverride[];
}

export interface GitStatus {
  git: boolean;
  dirty: boolean;
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
