const TAURI_HOSTS = new Set(['tauri.localhost']);

export function apiBase(protocol: string, hostname: string): string {
  if (protocol === 'tauri:' || TAURI_HOSTS.has(hostname)) {
    return 'http://127.0.0.1:4319';
  }
  return '';
}

export const API_BASE = apiBase(window.location.protocol, window.location.hostname);
