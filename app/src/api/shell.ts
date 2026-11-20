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

export function openUrl(url: string): Promise<void> {
  if (isTauri()) {
    return invoke<void>('open_url', { url });
  }
  window.open(url, '_blank', 'noopener,noreferrer');
  return Promise.resolve();
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) {
    return null;
  }
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({ directory: true, multiple: false });
  return typeof result === 'string' ? result : null;
}
