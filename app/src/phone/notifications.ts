import { useEffect, useRef } from 'react';
import { subscribeAux, subscribeEvents } from '../api/events';
import {
  transport,
  type PhoneNoticeEvent,
  type PhoneNoticeTexts,
} from '../api/transport';
import type { Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import type { Session } from '../api/types';

interface PhoneNotificationsInput {
  sessions: Session[];
  activeSessionId: string | null;
  screen: 'list' | 'session';
}

function texts(
  t: Translate,
  name: string,
  message: string,
): PhoneNoticeTexts {
  return {
    waitingTitle: t('notifications.waitingTitle', { name }),
    requestTitle: t('notifications.requestTitle'),
    requestBody: t('notifications.requestBody', { name }),
    permissionBody: t('notifications.permissionBody'),
    questionBody: t('notifications.questionBody'),
    inputBody: t('notifications.inputBody'),
    doneTitle: t('notifications.doneTitle', { name }),
    doneBody: t('notifications.doneBody'),
    messageTitle: t('notifications.messageTitle', { name }),
    messageBody: message,
  };
}

/**
 * Raises local notifications while the phone interface runs inside the shell.
 *
 * The shell applies the notice rules and the screen suppression rules, so the
 * package sends facts from the event stream, not a finished notification. The
 * browser layout raises no system notification.
 */
export function usePhoneNotifications({
  sessions,
  activeSessionId,
  screen,
}: PhoneNotificationsInput): void {
  const { t } = useLanguage();
  const tRef = useRef(t);
  tRef.current = t;
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const activeRef = useRef(activeSessionId);
  activeRef.current = activeSessionId;
  const screenRef = useRef(screen);
  screenRef.current = screen;
  const previous = useRef(new Map<string, Session>());

  useEffect(() => {
    for (const session of sessions) {
      previous.current.set(session.id, session);
    }
  }, [sessions]);

  useEffect(() => {
    if (transport.kind !== 'phone') {
      return;
    }

    const send = (
      event: PhoneNoticeEvent,
      name: string,
      message = '',
    ): void => {
      const screenSessionId =
        screenRef.current === 'session' ? activeRef.current : null;
      transport.notify({
        event,
        screenSessionId,
        appVisible:
          typeof document === 'undefined' ||
          document.visibilityState !== 'hidden',
        texts: texts(tRef.current, name, message),
      });
    };

    const stopEvents = subscribeEvents((session) => {
      const before = previous.current.get(session.id) ?? null;
      previous.current.set(session.id, session);
      if (
        session.status === 'active' &&
        session.waitingFor !== null &&
        session.waitingFor !== 'question' &&
        before?.waitingFor !== session.waitingFor
      ) {
        send(
          {
            kind: 'session',
            sessionId: session.id,
            sessionName: session.name,
            status: session.status,
            waitingFor: session.waitingFor,
            previousWaitingFor: before?.waitingFor ?? null,
          },
          session.name,
        );
      }
    });

    const stopAux = subscribeAux((event) => {
      if (event.type === 'question.updated') {
        if (event.question.status === 'pending') {
          send(
            {
              kind: 'question',
              sessionId: event.question.sessionId,
              sessionName: event.question.sessionName,
              status: event.question.status,
            },
            event.question.sessionName,
          );
        }
      } else if (event.type === 'request.updated') {
        if (event.request.status === 'pending') {
          send(
            {
              kind: 'request',
              sessionId: event.request.parentSessionId,
              sessionName: event.request.parentSessionName,
              status: event.request.status,
            },
            event.request.parentSessionName,
          );
        }
      } else if (event.type === 'notification') {
        if (event.notification.kind === 'done') {
          send(
            {
              kind: 'notification',
              sessionId: event.notification.sessionId,
              sessionName: event.notification.sessionName,
              notificationKind: event.notification.kind,
              message: '',
            },
            event.notification.sessionName,
          );
        } else if (event.notification.kind === 'message') {
          send(
            {
              kind: 'notification',
              sessionId: event.notification.sessionId,
              sessionName: event.notification.sessionName,
              notificationKind: event.notification.kind,
              message: event.notification.message,
            },
            event.notification.sessionName,
            event.notification.message,
          );
        }
      }
    });

    const stopClicks = transport.notificationClick((sessionId) => {
      window.location.hash = `session=${encodeURIComponent(sessionId)}`;
    });

    return () => {
      stopEvents();
      stopAux();
      stopClicks();
    };
  }, []);
}
