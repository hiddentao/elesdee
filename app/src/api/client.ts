import { API_BASE } from './base';
import type {
  Account,
  ApiErrorPayload,
  ApproveSessionResult,
  BootstrapResponse,
  Branch,
  CreateAccountInput,
  CreatePresetInput,
  CreateProjectInput,
  CreateSessionInput,
  FileContent,
  FileList,
  GitStatus,
  HealthResponse,
  LaunchPreset,
  PermissionDecision,
  Project,
  ProviderInfo,
  Question,
  RemoveAccountResult,
  Session,
  SessionChanges,
  SessionRequest,
  SharedFile,
  SwitchOptions,
  UpdatePresetInput,
  UpdateProjectDefaultsInput,
} from './types';

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

async function errorMessage(response: Response): Promise<string> {
  try {
    const payload = (await response.json()) as Partial<ApiErrorPayload>;
    if (payload && typeof payload.error === 'string' && payload.error.trim() !== '') {
      return payload.error;
    }
  } catch {
    // The response carries no JSON body.
  }
  return `The request failed with status ${response.status}.`;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(API_BASE + path, init);
  if (!response.ok) {
    throw new ApiError(response.status, await errorMessage(response));
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

function post(body: unknown): RequestInit {
  return {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  };
}

export function bootstrap(): Promise<BootstrapResponse> {
  return request<BootstrapResponse>('/api/bootstrap');
}

export function health(): Promise<HealthResponse> {
  return request<HealthResponse>('/api/health');
}

export function listProviders(): Promise<ProviderInfo[]> {
  return request<ProviderInfo[]>('/api/providers');
}

export function listQuestions(): Promise<Question[]> {
  return request<Question[]>('/api/questions');
}

export function answerQuestion(id: string, answer: string): Promise<Question> {
  return request<Question>(
    `/api/questions/${encodeURIComponent(id)}/answer`,
    post({ answer }),
  );
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
  return request<ApproveSessionResult>(
    `/api/session-requests/${encodeURIComponent(id)}/approve`,
    post({ allowFurther }),
  );
}

export function denySessionRequest(id: string): Promise<SessionRequest> {
  return request<SessionRequest>(
    `/api/session-requests/${encodeURIComponent(id)}/deny`,
    post({}),
  );
}

export function listProjects(
  query: string,
  signal?: AbortSignal,
): Promise<Project[]> {
  const params = new URLSearchParams({ q: query });
  return request<Project[]>(
    `/api/projects?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function listAccounts(
  query: string,
  signal?: AbortSignal,
): Promise<Account[]> {
  const params = new URLSearchParams({ q: query });
  return request<Account[]>(
    `/api/accounts?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function listPresets(
  query: string,
  signal?: AbortSignal,
): Promise<LaunchPreset[]> {
  const params = new URLSearchParams({ q: query });
  return request<LaunchPreset[]>(
    `/api/presets?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

export function createPreset(input: CreatePresetInput): Promise<LaunchPreset> {
  return request<LaunchPreset>('/api/presets', post(input));
}

export function updatePreset(
  id: string,
  input: UpdatePresetInput,
): Promise<LaunchPreset> {
  return request<LaunchPreset>(`/api/presets/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function deletePreset(id: string): Promise<void> {
  return request<void>(`/api/presets/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function listBranches(
  folder: string,
  query: string,
  signal?: AbortSignal,
): Promise<Branch[]> {
  const params = new URLSearchParams({ folder, q: query });
  return request<Branch[]>(
    `/api/branches?${params.toString()}`,
    signal ? { signal } : undefined,
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
  return request<Session>('/api/sessions', post(input));
}

export function createAccount(input: CreateAccountInput): Promise<Account> {
  return request<Account>('/api/accounts', post(input));
}

export function updateAccount(
  id: string,
  input: CreateAccountInput,
): Promise<Account> {
  return request<Account>(`/api/accounts/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export function removeAccount(id: string): Promise<RemoveAccountResult> {
  return request<RemoveAccountResult>(
    `/api/accounts/${encodeURIComponent(id)}`,
    { method: 'DELETE' },
  );
}

export function closeSession(id: string): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}/close`, post({}));
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

export function switchSessionAccount(
  id: string,
  accountId: string,
): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}/account`, post({
    accountId,
  }));
}

export function getSwitchOptions(id: string): Promise<SwitchOptions> {
  return request<SwitchOptions>(
    `/api/sessions/${encodeURIComponent(id)}/switch-options`,
  );
}

export function resumeSession(id: string): Promise<Session> {
  return request<Session>(`/api/sessions/${encodeURIComponent(id)}/resume`, post({}));
}

export function deleteSession(id: string): Promise<void> {
  return request<void>(`/api/sessions/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function sendInput(id: string, data: string): Promise<void> {
  return request<void>(`/api/sessions/${encodeURIComponent(id)}/input`, post({ data }));
}

export function respondPermission(
  id: string,
  requestId: string,
  decision: PermissionDecision,
): Promise<void> {
  return request<void>(
    `/api/sessions/${encodeURIComponent(id)}/permission`,
    post({ requestId, decision }),
  );
}

export function resizeSession(
  id: string,
  cols: number,
  rows: number,
): Promise<void> {
  return request<void>(`/api/sessions/${encodeURIComponent(id)}/resize`, post({ cols, rows }));
}

export function listFiles(
  id: string,
  path = '',
  signal?: AbortSignal,
): Promise<FileList> {
  const params = new URLSearchParams(path ? { path } : {});
  const query = params.toString();
  return request<FileList>(
    `/api/sessions/${encodeURIComponent(id)}/files${query ? `?${query}` : ''}`,
    signal ? { signal } : undefined,
  );
}

export function readFile(
  id: string,
  path: string,
  signal?: AbortSignal,
): Promise<FileContent> {
  const params = new URLSearchParams({ path });
  return request<FileContent>(
    `/api/sessions/${encodeURIComponent(id)}/file?${params.toString()}`,
    signal ? { signal } : undefined,
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

export function sessionChanges(
  id: string,
  signal?: AbortSignal,
): Promise<SessionChanges> {
  return request<SessionChanges>(
    `/api/sessions/${encodeURIComponent(id)}/changes`,
    signal ? { signal } : undefined,
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
