import { API_BASE } from './base';
import type {
  ApiErrorPayload,
  BootstrapResponse,
  CreateProjectInput,
  CreateSessionInput,
  HealthResponse,
  PermissionDecision,
  Project,
  Session,
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

export function createProject(input: CreateProjectInput): Promise<Project> {
  return request<Project>('/api/projects', post(input));
}

export function createSession(input: CreateSessionInput): Promise<Session> {
  return request<Session>('/api/sessions', post(input));
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
