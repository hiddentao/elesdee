import type {
  Notification,
  Question,
  Session,
  SessionRequest,
  WaitingFor,
} from './types';

export type NotificationKind = 'waiting' | 'done' | 'message';

/**
 * One local notification that the shell should raise.
 *
 * The shell fills the title and body from its language catalogs. The agent's
 * own message stays as the agent wrote it.
 */
export interface NotificationRequest {
  kind: NotificationKind;
  sessionId: string;
  sessionName: string;
  waitingFor?: WaitingFor;
  message?: string;
}

/** What the phone screen shows at the moment an event arrives. */
export interface PhoneScreenState {
  /** The session id on the open session screen, or null. */
  sessionId: string | null;
  /** True when the app is in the foreground. */
  appVisible: boolean;
}

/**
 * One parsed event that can ask for the user's attention. The shell parses the
 * event stream once, so this decision does not repeat that parsing.
 */
export type NotificationEvent =
  | { type: 'session'; session: Session; previous: Session | null }
  | { type: 'question'; question: Question }
  | { type: 'request'; request: SessionRequest }
  | { type: 'notification'; notification: Notification };

/**
 * Returns true when the phone screen does not already show this notice.
 *
 * A waiting card and the done status are visible on that session's screen, so
 * the app stays quiet while it is in front. A message has no card or toast in
 * the shell, so it is never on the screen and always raises a notification.
 */
export function shouldShowNotification(
  request: NotificationRequest,
  screen: PhoneScreenState,
): boolean {
  if (request.kind === 'message') {
    return true;
  }
  return !(screen.appVisible && screen.sessionId === request.sessionId);
}

/**
 * Decides which event raises a notification.
 *
 * A session update notifies when its waiting state changes, except for a
 * question: the pending question event that follows carries that notice, so
 * the session update stays quiet and the user gets one notification. A pending
 * session request, a question, a done report and the Notify me message each
 * raise their own notice. Other events raise none.
 */
export function noticeForEvent(
  event: NotificationEvent,
  screen: PhoneScreenState,
): NotificationRequest | null {
  const request = requestFor(event);
  if (!request || !shouldShowNotification(request, screen)) {
    return null;
  }
  return request;
}

function requestFor(event: NotificationEvent): NotificationRequest | null {
  switch (event.type) {
    case 'session':
      return sessionNotice(event.session, event.previous);
    case 'question':
      return event.question.status === 'pending'
        ? {
            kind: 'waiting',
            sessionId: event.question.sessionId,
            sessionName: event.question.sessionName,
            waitingFor: 'question',
          }
        : null;
    case 'request':
      return event.request.status === 'pending'
        ? {
            kind: 'waiting',
            sessionId: event.request.parentSessionId,
            sessionName: event.request.parentSessionName,
            waitingFor: 'start',
          }
        : null;
    case 'notification':
      return notificationNotice(event.notification);
  }
}

function sessionNotice(
  session: Session,
  previous: Session | null,
): NotificationRequest | null {
  if (session.status !== 'active' || !session.waitingFor) {
    return null;
  }
  // The pending question event follows this update and owns its notice.
  if (session.waitingFor === 'question') {
    return null;
  }
  if (previous && previous.waitingFor === session.waitingFor) {
    return null;
  }
  return {
    kind: 'waiting',
    sessionId: session.id,
    sessionName: session.name,
    waitingFor: session.waitingFor,
  };
}

function notificationNotice(
  notification: Notification,
): NotificationRequest | null {
  if (notification.kind === 'done') {
    return {
      kind: 'done',
      sessionId: notification.sessionId,
      sessionName: notification.sessionName,
    };
  }
  if (notification.kind === 'message') {
    return {
      kind: 'message',
      sessionId: notification.sessionId,
      sessionName: notification.sessionName,
      message: notification.message,
    };
  }
  return null;
}
