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

export type DemoServerState = 'active' | 'stopped';

export interface DemoServer {
  id: string;
  sessionId: string;
  name: string;
  port: number;
  address: string;
  tailscaleAddress?: string;
  funnelAddress?: string;
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
  client: Client | null;
  configId: string | null;
  allowedConfigIds?: string[];
  providerId: string | null;
  branch: string | null;
  handoffNote?: string | null;
  contextLeft?: number | null;
  folder: string;
  sourceRepository?: string | null;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: PendingPermission | null;
  firstPrompt?: string | null;
  parentSessionId?: string | null;
  parentSessionName?: string | null;
  envOverrides?: EnvOverride[];
  model?: string | null;
  effort?: string | null;
  args?: string[];
  config?: Record<string, string>;
  demoServers?: DemoServer[];
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
  defaultConfigId?: string | null;
  defaultSourceBranch?: string | null;
}

export interface UpdateProjectDefaultsInput {
  defaultConfigId?: string | null;
  defaultSourceBranch?: string | null;
}

export interface UsageRow {
  label: string;
  window?: 'five_hour' | 'seven_day' | 'balance' | string | null;
  used?: number;
  value?: string;
  warn?: boolean;
  resetsAt?: number | null;
}

export type UsageStatus = 'fresh' | 'stale' | 'unknown';

export interface Provider {
  id: string;
  name: string;
  vendor: string;
  auth: 'subscription' | 'key' | 'local';
  server: string | null;
  apiStyle?: string | null;
  sample: boolean;
  connected: boolean;
  plan: string;
  rows: UsageRow[];
  minimumBalance?: number | null;
  foot: string;
  usageReadAt?: string | null;
  usageCheckedAt?: string | null;
  usageStatus?: UsageStatus | null;
  usageSource?: string | null;
  supportedClients: Client[];
}

export type ClientCopy = 'app' | 'user' | 'bundled';

export interface AgentCheck {
  name: Client;
  installed: boolean;
  version: string | null;
  path: string | null;
  copy: ClientCopy | null;
}

export interface ClientUpdateCheck {
  client: Client;
  copy: ClientCopy | null;
  installed: boolean;
  currentVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  canUpdate: boolean;
  error: string | null;
}

export type InstallState = 'running' | 'done' | 'failed';

export type InstallStage =
  | 'preparing'
  | 'downloading'
  | 'installing'
  | 'verifying'
  | 'finishing';

export interface ClientInstall {
  id: string;
  client: Client;
  state: InstallState;
  stage: InstallStage;
  step: number;
  total: number;
  version: string | null;
  path: string | null;
  error: string | null;
}

export interface ProviderOption {
  id: string;
  label: string;
  auth: Array<'subscription' | 'key' | 'local'>;
  supportedClients: Client[];
  efforts: Record<string, string[]>;
  defaultModel: string | null;
  custom: boolean;
  signInClient: Client | null;
  balanceSource: boolean;
  minimumBalanceDefault: number | null;
}

export interface ModelChoice {
  id: string;
  label: string;
}

export interface ModelList {
  models: ModelChoice[];
  source: 'provider' | 'cache' | 'known' | string;
  fallback: boolean;
}

export interface AiConfig {
  id: string;
  name: string;
  providerId: string;
  client: Client;
  model: string | null;
  effort: string | null;
  args: string[];
  env: EnvOverride[];
  config: Record<string, string>;
  createdAt: string;
  updatedAt: string;
}

export interface SwitchOptions {
  configs: AiConfig[];
  allowedConfigIds: string[];
  nextConfigId: string | null;
}

export interface CreateProviderInput {
  name?: string;
  vendor: string;
  auth: 'subscription' | 'key' | 'local';
  key?: string;
  server?: string;
  apiStyle?: 'openai' | 'anthropic';
  minimumBalance?: number | null;
}

export type ProviderSignInMode = 'browser' | 'code';
export type ProviderSignInState =
  | 'starting'
  | 'waiting'
  | 'verifying'
  | 'success'
  | 'failed'
  | 'cancelled';
export type ProviderSignInReason =
  | 'clientNotFound'
  | 'clientError'
  | 'notSignedIn'
  | 'notSubscription'
  | 'timeout'
  | 'cancelled'
  | 'verificationFailed'
  | 'internal';

export interface ProviderSignIn {
  id: string;
  vendor: string;
  client: Client;
  mode: ProviderSignInMode;
  state: ProviderSignInState;
  url: string | null;
  code: string | null;
  reason: ProviderSignInReason | null;
  provider: Provider | null;
}

export interface StartProviderSignInInput {
  vendor: string;
  name?: string;
  providerId?: string;
  mode?: ProviderSignInMode;
}

export interface UpdateProviderInput {
  name?: string;
  vendor?: string;
  auth?: 'subscription' | 'key' | 'local';
  key?: string;
  server?: string | null;
  apiStyle?: 'openai' | 'anthropic' | null;
  minimumBalance?: number | null;
}

export interface CreateConfigInput {
  name?: string;
  providerId: string;
  client: Client;
  model?: string | null;
  effort?: string | null;
  args?: string[];
  env?: EnvOverride[];
  config?: Record<string, string>;
}

export interface UpdateConfigInput {
  name?: string;
  providerId?: string;
  client?: Client;
  model?: string | null;
  effort?: string | null;
  args?: string[];
  env?: EnvOverride[];
  config?: Record<string, string>;
}

export interface AppSettings {
  startAtLogin: boolean;
  keepAwake: boolean;
}

export interface SettingsPatch {
  startAtLogin?: boolean;
  keepAwake?: boolean;
}

export interface PairedDeviceRecord {
  endpointId: string;
  name: string;
  platform: string;
  pairedAt: string;
  lastSeen?: string | null;
}

export interface PairedDevice extends PairedDeviceRecord {
  connected: boolean;
}

export type PairingState =
  | 'idle'
  | 'waiting'
  | 'confirm'
  | 'paired'
  | 'denied'
  | 'expired'
  | 'cancelled';

export interface PairingStatus {
  state: PairingState;
  expiresAt?: number;
  phoneName?: string;
  code?: string;
}

export interface PairingCreated {
  expiresAt: number;
  relayReady: boolean;
  ticket?: string;
}

export interface DiagnosticsExport {
  path: string;
}

export interface BootstrapResponse {
  projects: Project[];
  providers: Provider[];
  configs: AiConfig[];
  sessions: Session[];
  questions?: Question[];
  sessionRequests?: SessionRequest[];
  settings: AppSettings;
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
  kind: 'message' | 'done';
  createdAt: string;
}

export type AuxEvent =
  | { type: 'question.updated'; question: Question }
  | { type: 'request.updated'; request: SessionRequest }
  | { type: 'sharedfile.created'; file: SharedFile }
  | { type: 'notification'; notification: Notification }
  | { type: 'provider.updated'; provider: Provider }
  | { type: 'history.appended'; sessionId: string; item: HistoryItem };

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

export interface UploadedFile {
  name: string;
  path: string;
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
  configId?: string;
  allowedConfigIds?: string[];
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

export interface GitHubStatus {
  connected: boolean;
  login: string | null;
  name: string | null;
  avatarUrl: string | null;
  defaultCloneFolder: string;
}

export type GitHubDeviceState =
  | 'starting'
  | 'waiting'
  | 'success'
  | 'failed'
  | 'cancelled';

export type GitHubDeviceReason =
  | 'notConfigured'
  | 'network'
  | 'denied'
  | 'expired'
  | 'keychain'
  | 'error';

export interface GitHubDevice {
  id: string;
  state: GitHubDeviceState;
  userCode: string | null;
  verificationUri: string | null;
  interval: number;
  expiresIn: number;
  login: string | null;
  reason: GitHubDeviceReason | null;
}

export interface GitHubRepo {
  id: number;
  fullName: string;
  name: string;
  owner: string;
  private: boolean;
  cloneUrl: string;
  htmlUrl: string;
  description: string | null;
  updatedAt: string | null;
  added: boolean;
  /** The project folder whose git remote matches the repository. */
  localFolder: string | null;
}

export interface CloneGitHubRepoInput {
  fullName: string;
  name?: string;
  parentFolder?: string;
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

export type HistoryKind = 'prompt' | 'reply' | 'tool';
export type HistoryState = 'streaming' | 'running' | 'done' | 'failed';

export interface HistoryItem {
  seq: number;
  id: string;
  kind: HistoryKind;
  tool?: string;
  text: string;
  state: HistoryState;
  gap?: OutputGap;
}

export interface HistoryStatus {
  type: 'history.state';
  structured: boolean;
  client: Client | null;
}

export interface SessionUpdatedEvent {
  type: 'session.updated';
  session: Session;
}

export interface BrowserTab {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
}

export type BrowserSetupPhase =
  | 'idle'
  | 'checking'
  | 'downloading'
  | 'verifying'
  | 'unpacking'
  | 'ready'
  | 'failed';

export interface BrowserSetup {
  phase: BrowserSetupPhase;
  downloaded: number;
  total: number | null;
  version: string | null;
  source: 'system' | 'download' | null;
  error: string | null;
}

export interface BrowserState {
  sessionId: string;
  started: boolean;
  available: boolean;
  tabs: BrowserTab[];
  activeTabId: string | null;
  screenshotSeq: number;
  error: string | null;
  setup: BrowserSetup;
}
