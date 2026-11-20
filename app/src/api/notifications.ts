import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from './shell';

export type DesktopNotificationKind = 'waiting' | 'done' | 'message';

export interface DesktopNotification {
  sessionId: string;
  title: string;
  body: string;
  kind: DesktopNotificationKind;
  /** True when the app already shows this session's card or notice. */
  sessionVisible: boolean;
}

const CLICKED_EVENT = 'elesdee://notification-clicked';

/**
 * Asks the shell to show a desktop notification. The shell suppresses a
 * notification that repeats what the focused window already shows.
 */
export async function showDesktopNotification(
  notification: DesktopNotification,
): Promise<boolean> {
  if (!isTauri()) {
    return false;
  }
  try {
    return await invoke<boolean>('show_notification', { ...notification });
  } catch {
    return false;
  }
}

/** Checks the notification permission and asks for it when the system needs it. */
export async function ensureNotificationPermission(): Promise<void> {
  if (!isTauri()) {
    return;
  }
  try {
    const state = await invoke<string>('notification_permission_state');
    if (state === 'prompt' || state === 'promptWithRationale') {
      await invoke<string>('request_notification_permission');
    }
  } catch {
    // Notifications stay off when the shell cannot answer.
  }
}

/** Calls back with the session id when the user clicks a notification. */
export async function subscribeNotificationClicks(
  onSession: (sessionId: string) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  return listen<string>(CLICKED_EVENT, (event) => onSession(event.payload));
}
