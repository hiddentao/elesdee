import { invoke } from '@tauri-apps/api/core';
import type {
  CommandError,
  ComputerRecord,
  ComputerView,
  PairConfirm,
  PhoneInfo,
} from './types';

function normalizeError(error: unknown): CommandError {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  ) {
    const value = error as Partial<CommandError>;
    return {
      code: value.code ?? 'unknown',
      status: value.status,
      message: value.message ?? '',
    };
  }
  return {
    code: 'unknown',
    message: error instanceof Error ? error.message : String(error),
  };
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

export const phone = {
  info: () => call<PhoneInfo>('phone_info'),
  computers: () => call<ComputerView[]>('phone_computers'),
  pairStart: (ticket: string, name: string, platform: string) =>
    call<PairConfirm>('phone_pair_start', { ticket, name, platform }),
  pairFinish: () => call<ComputerRecord>('phone_pair_finish'),
  pairCancel: () => call<void>('phone_pair_cancel'),
  forget: (computerId: string) =>
    call<void>('phone_forget', { computerId }),
  connect: (computerId: string) =>
    call<ComputerRecord>('phone_connect', { computerId }),
  disconnect: (computerId: string) =>
    call<void>('phone_disconnect', { computerId }),
  request: (computerId: string, method: string, params?: unknown) =>
    call<unknown>('phone_request', {
      computerId,
      method,
      params: params ?? null,
    }),
};

/**
 * Opens the camera scanner and returns the ticket text, or null when the
 * scanner is not available on this platform.
 */
export async function scanTicket(): Promise<string | null> {
  const scanner = await import('@tauri-apps/plugin-barcode-scanner');
  const result = await scanner.scan({
    windowed: true,
    formats: [scanner.Format.QRCode],
  });
  return result?.content ?? null;
}
