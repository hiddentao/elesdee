import type { Provider, Session, WaitingFor } from '../api/types';
import { englishT, type Translate } from '../i18n';

export type StatusTone = 'running' | 'waiting' | 'input' | 'paused' | 'done' | 'closed';

export type StatusShape =
  | 'dot'
  | 'diamond'
  | 'square'
  | 'square-ring'
  | 'thick-ring'
  | 'hollow';

/** A marker shape that tells each work state apart without colour. */
export function statusShape(tone: StatusTone): StatusShape {
  switch (tone) {
    case 'waiting':
      return 'diamond';
    case 'input':
      return 'square';
    case 'paused':
      return 'square-ring';
    case 'done':
      return 'thick-ring';
    case 'closed':
      return 'hollow';
    default:
      return 'dot';
  }
}

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

export function workLabel(
  session: Session,
  t: Translate = englishT,
): string | null {
  if (session.status === 'closed') {
    return null;
  }
  if (session.waitingFor) {
    return t('status.waitingForYou');
  }

  switch (session.workState) {
    case 'working':
      return t('status.working');
    case 'waiting':
      return t('status.waiting');
    case 'input':
      return t('status.waitingForYou');
    case 'paused':
      return t('status.paused');
    case 'done':
      return t('status.finished');
    default:
      return null;
  }
}

export function tabStateLabel(
  session: Session,
  t: Translate = englishT,
): string {
  if (session.status === 'closed') {
    return t('status.closed');
  }
  if (session.waitingFor) {
    return t('status.waiting');
  }

  switch (session.workState) {
    case 'working':
      return t('status.working');
    case 'waiting':
      return t('status.waiting');
    case 'input':
      return t('status.input');
    case 'paused':
      return t('status.paused');
    case 'done':
      return t('status.done');
    default:
      return t('status.active');
  }
}

export function statusText(
  session: Session,
  t: Translate = englishT,
): string {
  if (session.status === 'closed') {
    return t('status.closed');
  }
  const detail = workLabel(session, t);
  return detail
    ? t('status.activeWith', { detail })
    : t('status.active');
}

export function waitingForDetail(
  reason: WaitingFor,
  t: Translate = englishT,
): string {
  switch (reason) {
    case 'permission':
      return t('status.reasonPermission');
    case 'input':
      return t('status.reasonInput');
    case 'start':
      return t('status.reasonStart');
    case 'question':
      return t('status.reasonQuestion');
  }
}

export function sessionKindLabel(
  session: Session,
  t: Translate = englishT,
): string {
  if (session.kind === 'terminal') {
    return t('status.terminal');
  }
  return session.client
    ? t('status.agentWithClient', { client: session.client })
    : t('status.agent');
}

export function providerFor(
  providers: Provider[],
  providerId: string | null,
): Provider | null {
  if (!providerId) {
    return null;
  }
  return providers.find((provider) => provider.id === providerId) ?? null;
}
