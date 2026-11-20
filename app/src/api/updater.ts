import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauri } from './shell';

export type UpdateState =
  | 'idle'
  | 'checking'
  | 'downloading'
  | 'ready'
  | 'upToDate'
  | 'deferred'
  | 'installing'
  | 'error'
  | 'unavailable';

export type UpdateError =
  | 'notConfigured'
  | 'network'
  | 'signature'
  | 'checkFailed'
  | 'downloadFailed'
  | 'installFailed';

export interface UpdateSnapshot {
  state: UpdateState;
  currentVersion: string;
  version: string | null;
  progress: number | null;
  error: UpdateError | null;
}

const UPDATE_EVENT = 'elesdee://update-changed';

export async function getUpdateState(): Promise<UpdateSnapshot | null> {
  if (!isTauri()) {
    return null;
  }
  try {
    return await invoke<UpdateSnapshot>('get_update_state');
  } catch {
    return null;
  }
}

export function checkForUpdate(): Promise<UpdateSnapshot> {
  return invoke<UpdateSnapshot>('check_for_update');
}

/** Installs the downloaded update and restarts, or defers while sessions run. */
export function installUpdate(
  sessionsRunning: boolean,
  restartConfirmed: boolean,
): Promise<UpdateSnapshot> {
  return invoke<UpdateSnapshot>('install_update', {
    sessionsRunning,
    restartConfirmed,
  });
}

/** Installs the downloaded update once every session is closed. */
export function deferUpdate(): Promise<UpdateSnapshot> {
  return invoke<UpdateSnapshot>('defer_update');
}

/** Calls back with a new snapshot when the shell checks, downloads or installs. */
export async function subscribeUpdateChanges(
  onChange: (snapshot: UpdateSnapshot) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  try {
    return await listen<UpdateSnapshot>(UPDATE_EVENT, (event) =>
      onChange(event.payload),
    );
  } catch {
    return () => {};
  }
}
