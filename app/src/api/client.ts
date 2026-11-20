import { API_BASE } from './base';
import {
  ApiError,
  errorMessage,
  httpRequest as request,
  transport,
} from './transport';
import type {
  AiConfig,
  AppSettings,
  ApproveSessionResult,
  BootstrapResponse,
  Branch,
  BrowserState,
  Client,
  ClientInstall,
  ClientUpdateCheck,
  CloneGitHubRepoInput,
  CreateConfigInput,
  CreateProjectInput,
  CreateProviderInput,
  CreateSessionInput,
  DiagnosticsExport,
  FileContent,
  FileList,
  GitHubDevice,
  GitHubRepo,
  GitHubStatus,
  GitStatus,
  HealthResponse,
  ModelList,
  PairedDevice,
  PairedDeviceRecord,
  PairingCreated,
  PairingStatus,
  PermissionDecision,
  Project,
  Provider,
  ProviderOption,
  ProviderSignIn,
  Question,
  Session,
  SessionChanges,
  SessionRequest,
  SettingsPatch,
  SharedFile,
  StartProviderSignInInput,
  SwitchOptions,
  UpdateConfigInput,
  UpdateProjectDefaultsInput,
  UpdateProviderInput,
  UploadedFile,
  UsageRow,
} from './types';

export { ApiError } from './transport';

function post(body: unknown): RequestInit {
  return {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  };
}

export function bootstrap(): Promise<BootstrapResponse> {
  return transport.request<BootstrapResponse>('bootstrap');
}

export function health(): Promise<HealthResponse> {
  return transport.request<HealthResponse>('health');
}

const CLIENT_SLUGS: Record<Client, string> = {
  'Claude Code': 'claude-code',
  Codex: 'codex',
  'OpenCode 2': 'opencode-2',
  Antigravity: 'antigravity',
};

export function installClient(client: Client): Promise<ClientInstall> {
  return request<ClientInstall>(
    `/api/clients/${CLIENT_SLUGS[client]}/install`,
    post({}),
  );
}

export function getClientInstall(id: string): Promise<ClientInstall> {
  return request<ClientInstall>(
    `/api/clients/install/${encodeURIComponent(id)}`,
  );
}

export function checkClientUpdate(
  client: Client,
): Promise<ClientUpdateCheck> {
  return request<ClientUpdateCheck>(
    `/api/clients/${CLIENT_SLUGS[client]}/update`,
  );
}

export function updateClient(client: Client): Promise<ClientInstall> {
  return request<ClientInstall>(
    `/api/clients/${CLIENT_SLUGS[client]}/update`,
    post({}),
  );
}

export function listProviderOptions(): Promise<ProviderOption[]> {
  return request<ProviderOption[]>('/api/provider-options');
}

export function listProviders(
  query: string,
  signal?: AbortSignal,
): Promise<Provider[]> {
  const params = new URLSearchParams({ q: query });
  return request<Provider[]>(
    `/api/providers?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function createProvider(input: CreateProviderInput): Promise<Provider> {
  return request<Provider>('/api/providers', post(input));
}

export function updateProvider(
  id: string,
  input: UpdateProviderInput,
): Promise<Provider> {
  return request<Provider>(`/api/providers/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function removeProvider(id: string): Promise<void> {
  return request<void>(`/api/providers/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function startProviderSignIn(
  input: StartProviderSignInInput,
): Promise<ProviderSignIn> {
  return request<ProviderSignIn>('/api/sign-in', post(input));
}

export function getProviderSignIn(id: string): Promise<ProviderSignIn> {
  return request<ProviderSignIn>(`/api/sign-in/${encodeURIComponent(id)}`);
}

export function submitProviderSignInCode(
  id: string,
  code: string,
): Promise<ProviderSignIn> {
  return request<ProviderSignIn>(
    `/api/sign-in/${encodeURIComponent(id)}/code`,
    post({ code }),
  );
}

export function cancelProviderSignIn(id: string): Promise<void> {
  return request<void>(`/api/sign-in/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function listProviderModels(
  id: string,
  client: Client,
  signal?: AbortSignal,
): Promise<ModelList> {
  const params = new URLSearchParams({ client });
  return request<ModelList>(
    `/api/providers/${encodeURIComponent(id)}/models?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function getProviderUsage(id: string): Promise<Provider> {
  return request<Provider>(`/api/providers/${encodeURIComponent(id)}/usage`);
}

export function updateProviderUsage(
  id: string,
  rows: UsageRow[],
): Promise<Provider> {
  return request<Provider>(
    `/api/providers/${encodeURIComponent(id)}/usage`,
    post({ rows }),
  );
}

export function refreshProviderUsage(id: string): Promise<Provider> {
  return request<Provider>(
    `/api/providers/${encodeURIComponent(id)}/usage/refresh`,
    post({}),
  );
}

export function listConfigs(
  query: string,
  signal?: AbortSignal,
): Promise<AiConfig[]> {
  const params = new URLSearchParams({ q: query });
  return request<AiConfig[]>(
    `/api/configs?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function createConfig(input: CreateConfigInput): Promise<AiConfig> {
  return request<AiConfig>('/api/configs', post(input));
}

export function updateConfig(
  id: string,
  input: UpdateConfigInput,
): Promise<AiConfig> {
  return request<AiConfig>(`/api/configs/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function deleteConfig(id: string): Promise<void> {
  return request<void>(`/api/configs/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function listQuestions(): Promise<Question[]> {
  return transport.request<Question[]>('questions.list');
}

export function answerQuestion(id: string, answer: string): Promise<Question> {
  return transport.request<Question>('questions.answer', { id, answer });
}

export function withdrawQuestion(id: string): Promise<Question> {
  return request<Question>(
    `/api/questions/${encodeURIComponent(id)}/withdraw`,
    post({}),
  );
}

export function approveSessionRequest(
  id: string,
  allowFurther: boolean,
): Promise<ApproveSessionResult> {
  return transport.request<ApproveSessionResult>('sessionRequests.approve', {
    id,
    allowFurther,
  });
}

export function denySessionRequest(id: string): Promise<SessionRequest> {
  return transport.request<SessionRequest>('sessionRequests.deny', { id });
}

export function listProjects(
  query: string,
  signal?: AbortSignal,
): Promise<Project[]> {
  return transport.request<Project[]>(
    'projects.list',
    { q: query },
    { signal },
  );
}

export function listBranches(
  folder: string,
  query: string,
  signal?: AbortSignal,
): Promise<Branch[]> {
  return transport.request<Branch[]>(
    'branches.list',
    { folder, q: query },
    { signal },
  );
}

export function getGitStatus(
  folder: string,
  signal?: AbortSignal,
): Promise<GitStatus> {
  const params = new URLSearchParams({ folder });
  return request<GitStatus>(
    `/api/git-status?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function createProject(input: CreateProjectInput): Promise<Project> {
  return request<Project>('/api/projects', post(input));
}

export function getGitHubStatus(): Promise<GitHubStatus> {
  return request<GitHubStatus>('/api/github/status');
}

export function startGitHubSignIn(): Promise<GitHubDevice> {
  return request<GitHubDevice>('/api/github/device', post({}));
}

export function getGitHubSignIn(id: string): Promise<GitHubDevice> {
  return request<GitHubDevice>(
    `/api/github/device/${encodeURIComponent(id)}`,
  );
}

export function cancelGitHubSignIn(id: string): Promise<void> {
  return request<void>(`/api/github/device/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function signOutGitHub(): Promise<void> {
  return request<void>('/api/github/sign-out', post({}));
}

export function listGitHubRepos(
  query: string,
  signal?: AbortSignal,
): Promise<GitHubRepo[]> {
  const params = new URLSearchParams({ q: query });
  return request<GitHubRepo[]>(
    `/api/github/repos?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function cloneGitHubRepo(
  input: CloneGitHubRepoInput,
): Promise<Project> {
  return request<Project>('/api/github/clone', post(input));
}

export function updateProjectDefaults(
  id: string,
  input: UpdateProjectDefaultsInput,
): Promise<Project> {
  return request<Project>(`/api/projects/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function createSession(input: CreateSessionInput): Promise<Session> {
  return transport.request<Session>('sessions.create', input);
}

export function closeSession(id: string): Promise<Session> {
  return transport.request<Session>('sessions.close', { sessionId: id });
}

export function renameSession(id: string, name: string): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
}

export function reopenSession(id: string): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}/reopen`, post({}));
}

export function switchSessionConfig(
  id: string,
  configId: string,
): Promise<Session> {
  return transport.request<Session>('sessions.config', {
    sessionId: id,
    configId,
  });
}

export function getSwitchOptions(id: string): Promise<SwitchOptions> {
  return request<SwitchOptions>(
    `/api/sessions/${encodeURIComponent(id)}/switch-options`,
  );
}

export function updateSessionAllowedConfigs(
  id: string,
  configIds: string[],
): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ allowedConfigIds: configIds }),
  });
}

export function moveSession(id: string, projectId: string): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ projectId }),
  });
}

export function resumeSession(id: string): Promise<Session> {
  return transport.request<Session>('sessions.resume', { sessionId: id });
}

export function deleteSession(id: string): Promise<void> {
  return request<void>(`/api/sessions/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function stopDemoServer(
  sessionId: string,
  serverId: string,
): Promise<Session> {
  return request<Session>(
    `/api/sessions/${encodeURIComponent(sessionId)}/demo-servers/${encodeURIComponent(serverId)}/stop`,
    post({}),
  );
}

export function enableDemoFunnel(
  sessionId: string,
  serverId: string,
): Promise<Session> {
  return request<Session>(
    `/api/sessions/${encodeURIComponent(sessionId)}/demo-servers/${encodeURIComponent(serverId)}/funnel`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ confirmed: true }),
    },
  );
}

export function disableDemoFunnel(
  sessionId: string,
  serverId: string,
): Promise<Session> {
  return request<Session>(
    `/api/sessions/${encodeURIComponent(sessionId)}/demo-servers/${encodeURIComponent(serverId)}/funnel`,
    { method: 'DELETE' },
  );
}

export function sendInput(id: string, data: string): Promise<void> {
  return transport.request<void>('sessions.input', { sessionId: id, data });
}

export function respondPermission(
  id: string,
  requestId: string,
  decision: PermissionDecision,
): Promise<void> {
  return transport.request<void>('sessions.permission', {
    sessionId: id,
    requestId,
    decision,
  });
}

export function resizeSession(
  id: string,
  cols: number,
  rows: number,
): Promise<void> {
  if (transport.kind === 'phone') {
    // The phone bridge does not expose a resize method.
    return Promise.resolve();
  }
  return request<void>(`/api/sessions/${encodeURIComponent(id)}/resize`, post({ cols, rows }));
}

export function listFiles(
  id: string,
  path = '',
  signal?: AbortSignal,
): Promise<FileList> {
  return transport.request<FileList>(
    'files.list',
    { sessionId: id, path },
    { signal },
  );
}

export function readFile(
  id: string,
  path: string,
  signal?: AbortSignal,
): Promise<FileContent> {
  return transport.request<FileContent>(
    'files.read',
    { sessionId: id, path },
    { signal },
  );
}

export function writeFile(
  id: string,
  path: string,
  content: string,
): Promise<FileContent> {
  return request<FileContent>(
    `/api/sessions/${encodeURIComponent(id)}/file`,
    post({ path, content }),
  );
}

export function readSessionImage(
  id: string,
  path: string,
  signal?: AbortSignal,
): Promise<Blob> {
  return transport.readImage(id, path, { signal });
}

export function uploadFile(
  id: string,
  name: string,
  file: Blob,
): Promise<UploadedFile> {
  return transport.upload(id, name, file);
}

export function sessionChanges(
  id: string,
  signal?: AbortSignal,
): Promise<SessionChanges> {
  return transport.request<SessionChanges>(
    'diff.read',
    { sessionId: id },
    { signal },
  );
}

export function listSharedFiles(
  id: string,
  signal?: AbortSignal,
): Promise<SharedFile[]> {
  return request<SharedFile[]>(
    `/api/sessions/${encodeURIComponent(id)}/shared`,
    signal ? { signal } : undefined,
  );
}

export function sharedFileUrl(id: string, fileId: string): string {
  return `${API_BASE}/api/sessions/${encodeURIComponent(id)}/shared/${encodeURIComponent(fileId)}`;
}

export async function readSharedFile(
  id: string,
  fileId: string,
): Promise<string> {
  const response = await fetch(sharedFileUrl(id, fileId));
  if (!response.ok) {
    throw new ApiError(response.status, await errorMessage(response));
  }
  return response.text();
}

export function getBrowserState(
  id: string,
  signal?: AbortSignal,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser`,
    signal ? { signal } : undefined,
  );
}

export function openBrowserUrl(id: string, url: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/open`,
    post({ url }),
  );
}

export function setupBrowser(id: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/setup`,
    post({}),
  );
}

export function browserBack(id: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/back`,
    post({}),
  );
}

export function browserForward(id: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/forward`,
    post({}),
  );
}

export function browserReload(id: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/reload`,
    post({}),
  );
}

export function clickBrowserAt(
  id: string,
  x: number,
  y: number,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/input/click`,
    post({ x, y }),
  );
}

export function typeBrowserText(id: string, text: string): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/input/type`,
    post({ text }),
  );
}

export function pressBrowserKey(
  id: string,
  key: string,
  modifiers = 0,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/input/key`,
    post({ key, modifiers }),
  );
}

export function createBrowserTab(
  id: string,
  url?: string,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/tabs`,
    post(url ? { url } : {}),
  );
}

export function activateBrowserTab(
  id: string,
  tabId: string,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/tabs/${encodeURIComponent(tabId)}/activate`,
    post({}),
  );
}

export function closeBrowserTab(
  id: string,
  tabId: string,
): Promise<BrowserState> {
  return request<BrowserState>(
    `/api/sessions/${encodeURIComponent(id)}/browser/tabs/${encodeURIComponent(tabId)}/close`,
    post({}),
  );
}

export function browserScreenshotUrl(
  id: string,
  tabId: string,
  seq: number,
): string {
  const params = new URLSearchParams({ tab: tabId, v: String(seq) });
  return `${API_BASE}/api/sessions/${encodeURIComponent(id)}/browser/screenshot?${params.toString()}`;
}

export function updateSettings(input: SettingsPatch): Promise<AppSettings> {
  return request<AppSettings>('/api/settings', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function createPairing(): Promise<PairingCreated> {
  return request<PairingCreated>('/api/pairing', post({}));
}

export function getPairingStatus(): Promise<PairingStatus> {
  return request<PairingStatus>('/api/pairing');
}

export async function approvePairing(): Promise<PairedDevice> {
  const device = await request<PairedDeviceRecord>(
    '/api/pairing/approve',
    post({}),
  );
  return { ...device, connected: false };
}

export function denyPairing(): Promise<void> {
  return request<void>('/api/pairing/deny', post({}));
}

export function cancelPairing(): Promise<void> {
  return request<void>('/api/pairing', { method: 'DELETE' });
}

export function pairingQrUrl(cacheBust: number): string {
  return `${API_BASE}/api/pairing/qr?v=${cacheBust}`;
}

export function listDevices(): Promise<PairedDevice[]> {
  return request<PairedDevice[]>('/api/devices');
}

export function removeDevice(endpointId: string): Promise<void> {
  return request<void>(
    `/api/devices/${encodeURIComponent(endpointId)}`,
    { method: 'DELETE' },
  );
}

export function exportDiagnostics(folder: string): Promise<DiagnosticsExport> {
  return request<DiagnosticsExport>('/api/diagnostics/export', post({ folder }));
}
