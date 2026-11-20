export type SessionState = 'active' | 'closed';
export type WorkState = 'working' | 'waiting' | 'input' | 'paused' | 'done';
export type WaitingFor = 'permission' | 'input' | 'start' | 'question';
export type SessionKind = 'agent' | 'terminal';
export type Client = 'Claude Code' | 'Codex' | 'OpenCode 2' | 'Antigravity';

export interface Shortcut {
  key: string;
  label: string;
  name: string;
  description: string;
}

export interface PendingPermission {
  requestId: string;
  tool: string;
  summary: string;
}

export interface UsageRow {
  label: string;
  window?: string | null;
  used?: number;
  value?: string;
  min?: string;
  warn?: boolean;
  resetsAt?: number | null;
}

export interface Provider {
  id: string;
  name: string;
  vendor: string;
  auth: 'subscription' | 'key' | 'local';
  server?: string | null;
  sample?: boolean;
  plan: string;
  rows: UsageRow[];
  foot: string;
  usageReadAt?: string | null;
  usageCheckedAt?: string | null;
  usageStatus?: 'fresh' | 'stale' | 'unknown' | null;
  usageSource?: string | null;
  supportedClients?: Client[];
}

export interface AiConfig {
  id: string;
  name: string;
  providerId: string;
  client: Client;
  model?: string | null;
  effort?: string | null;
  args?: string[];
  env?: { name: string; value: string }[];
  config?: Record<string, string>;
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
  configId: string;
  providerId: string;
  client: Client;
  status: 'pending' | 'approved' | 'denied';
  childSessionId: string | null;
  createdAt: string;
}

export type DemoServerState = 'active' | 'stopped';

export interface DemoServer {
  id: string;
  sessionId: string;
  name: string;
  port: number;
  address: string;
  tailscaleAddress?: string;
  state: DemoServerState;
  createdAt: string;
}

export interface Session {
  id: string;
  name: string;
  short: string;
  projectId: string;
  kind: SessionKind;
  status: SessionState;
  workState: WorkState | null;
  client?: Client | null;
  configId?: string | null;
  allowedConfigIds?: string[];
  providerId?: string | null;
  branch?: string | null;
  folder?: string;
  contextLeft?: number | null;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  firstPrompt?: string | null;
  demoServers?: DemoServer[];
  startedAt: string;
  shortcuts?: Shortcut[];
}

export interface ForwardAddress {
  port: number;
  localPort: number;
  address: string;
}

export interface Project {
  id: string;
  name: string;
  folder: string;
  defaultConfigId?: string | null;
  defaultSourceBranch?: string | null;
}

export interface Branch {
  name: string;
  current: boolean;
}

export interface BootstrapResponse {
  projects: Project[];
  providers: Provider[];
  configs: AiConfig[];
  sessions: Session[];
  questions: Question[];
  sessionRequests: SessionRequest[];
}

export interface ComputerRecord {
  id: string;
  name: string;
  addr: unknown;
  protocolVersion: number;
  minBridgeVersion: number;
  pairedAt: number;
}

export interface ComputerView {
  computer: ComputerRecord;
  connected: boolean;
}

export interface PhoneInfo {
  phoneId: string;
  platform: string;
  bridgeVersion: number;
  protocolVersion: number;
}

export interface PairConfirm {
  code: string;
  computerId: string;
  computerName: string;
}

export interface CommandError {
  code: string;
  status?: string;
  message: string;
}

export type ConnectionPhase =
  | 'connected'
  | 'disconnected'
  | 'reconnecting'
  | 'stopped';

export interface ConnectionState {
  computerId: string;
  state: ConnectionPhase;
  error?: CommandError;
}

export interface SubscriptionValue {
  id: string;
  event?: unknown;
  error?: CommandError;
  ended?: boolean;
}

export interface SubscriptionOpened {
  id: string;
}

export interface UploadedFile {
  name: string;
  path: string;
  size: number;
}

export interface ImagePayload {
  contentType: string;
  data: string;
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
