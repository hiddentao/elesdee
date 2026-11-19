import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke, tauriIsTauri } = vi.hoisted(() => ({
  invoke: vi.fn(),
  tauriIsTauri: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  isTauri: tauriIsTauri,
}));

import { isTauri, openFolder, openTerminal } from './shell';

beforeEach(() => {
  invoke.mockReset();
  tauriIsTauri.mockReset();
});

describe('shell', () => {
  it('uses the Tauri check and falls back safely', () => {
    tauriIsTauri.mockReturnValue(true);
    expect(isTauri()).toBe(true);

    tauriIsTauri.mockReturnValue(false);
    expect(isTauri()).toBe(false);

    tauriIsTauri.mockImplementation(() => {
      throw new Error('No Tauri runtime.');
    });
    expect(isTauri()).toBe(false);
  });

  it('opens a folder with the path argument', async () => {
    invoke.mockResolvedValue(undefined);

    await expect(openFolder('/dev/web')).resolves.toBeUndefined();

    expect(invoke).toHaveBeenCalledWith('open_folder', { path: '/dev/web' });
  });

  it('opens a terminal with the path argument', async () => {
    invoke.mockResolvedValue(undefined);

    await expect(openTerminal('/dev/web')).resolves.toBeUndefined();

    expect(invoke).toHaveBeenCalledWith('open_terminal', { path: '/dev/web' });
  });
});
