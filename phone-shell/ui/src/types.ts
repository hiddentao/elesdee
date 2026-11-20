export type SessionState = 'active' | 'closed';
export type WorkState = 'working' | 'waiting' | 'input' | 'paused' | 'done';
export type WaitingFor = 'permission' | 'input' | 'start' | 'question';
export type SessionKind = 'agent' | 'terminal';

export interface Session {
  id: string;
  name: string;
  short: string;
  projectId: string;
  kind: SessionKind;
  status: SessionState;
  workState: WorkState | null;
  preview: string;
  waitingFor: WaitingFor | null;
  pendingPermission: unknown | null;
  startedAt: string;
}

export interface Project {
  id: string;
  name: string;
}

export interface BootstrapResponse {
  projects: Project[];
  accounts: unknown[];
  sessions: Session[];
  questions?: unknown[];
  sessionRequests?: unknown[];
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
