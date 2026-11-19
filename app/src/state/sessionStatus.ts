import type { Account, Session, WaitingFor } from '../api/types';

export type StatusTone = 'running' | 'waiting' | 'input' | 'paused' | 'done' | 'closed';

export function statusTone(session: Session): StatusTone {
  if (session.status === 'closed') {
    return 'closed';
  }
  if (session.waitingFor) {
    return 'waiting';
  }

  switch (session.workState) {
    case 'working':
      return 'running';
    case 'waiting':
      return 'waiting';
    case 'input':
      return 'input';
    case 'paused':
      return 'paused';
    case 'done':
      return 'done';
    default:
      return 'running';
  }
}

export function workLabel(session: Session): string | null {
  if (session.status === 'closed') {
    return null;
  }
  if (session.waitingFor) {
    return 'Waiting for you';
  }

  switch (session.workState) {
    case 'working':
      return 'Working';
    case 'waiting':
      return 'Waiting';
    case 'input':
      return 'Waiting for you';
    case 'paused':
      return 'Paused';
    case 'done':
      return 'Finished';
    default:
      return null;
  }
}

export function tabStateLabel(session: Session): string {
  if (session.status === 'closed') {
    return 'Closed';
  }
  if (session.waitingFor) {
    return 'Waiting';
  }

  switch (session.workState) {
    case 'working':
      return 'Working';
    case 'waiting':
      return 'Waiting';
    case 'input':
      return 'Input';
    case 'paused':
      return 'Paused';
    case 'done':
      return 'Done';
    default:
      return 'Active';
  }
}

export function statusText(session: Session): string {
  if (session.status === 'closed') {
    return 'Closed';
  }
  const detail = workLabel(session);
  return detail ? `Active, ${detail}` : 'Active';
}

export function waitingForDetail(reason: WaitingFor): string {
  switch (reason) {
    case 'permission':
      return 'The agent needs your permission to continue.';
    case 'input':
      return 'The agent is waiting for your input.';
    case 'start':
      return 'The session is ready to start.';
    case 'question':
      return 'The agent asked you a question.';
  }
}

export function sessionKindLabel(session: Session): string {
  if (session.kind === 'terminal') {
    return 'Terminal';
  }
  return session.client ? `Agent · ${session.client}` : 'Agent';
}

export function accountFor(accounts: Account[], accountId: string | null): Account | null {
  if (!accountId) {
    return null;
  }
  return accounts.find((account) => account.id === accountId) ?? null;
}
