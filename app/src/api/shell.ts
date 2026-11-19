import { invoke, isTauri as tauriIsTauri } from '@tauri-apps/api/core';

export function isTauri(): boolean {
  try {
    return tauriIsTauri();
  } catch {
    return false;
  }
}

export function openFolder(path: string): Promise<void> {
  return invoke<void>('open_folder', { path });
}

export function openTerminal(path: string): Promise<void> {
  return invoke<void>('open_terminal', { path });
}
