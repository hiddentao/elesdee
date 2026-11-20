import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import type {
  CommandError,
  ComputerRecord,
  ComputerView,
  ConnectionState,
  ForwardAddress,
  PairConfirm,
  PhoneInfo,
  SubscriptionOpened,
  SubscriptionValue,
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

export type LiveChannel = 'events' | 'output';

export interface LiveHandlers {
  onEvent: (value: unknown) => void;
  onError?: (error: CommandError) => void;
  onEnd?: () => void;
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
  subscribe: (
    computerId: string,
    id: string,
    channel: LiveChannel,
    params?: unknown,
  ) =>
    call<SubscriptionOpened>('phone_subscribe', {
      computerId,
      id,
      channel,
      params: params ?? null,
    }),
  unsubscribe: (id: string) =>
    call<void>('phone_unsubscribe', { id }),
  forwardStart: (computerId: string, port: number) =>
    call<ForwardAddress>('phone_forward_start', { computerId, port }),
  forwardStop: (port: number) =>
    call<void>('phone_forward_stop', { port }),
};

/** Opens a demo-server address in the phone's browser. */
export async function openAddress(address: string): Promise<void> {
  try {
    await openUrl(address);
  } catch (error) {
    throw normalizeError(error);
  }
}

/**
 * Opens a live subscription and returns its close function.
 *
 * The listener is registered before the command sends, so a fast first event
 * is not lost.
 */
export async function openSubscription(
  computerId: string,
  id: string,
  channel: LiveChannel,
  params: unknown,
  handlers: LiveHandlers,
): Promise<() => Promise<void>> {
  const unlisten: UnlistenFn = await listen<SubscriptionValue>(
    'phone:subscription',
    (event) => {
      const value = event.payload;
      if (value.id !== id) {
        return;
      }
      if (value.event !== undefined) {
        handlers.onEvent(value.event);
      }
      if (value.error) {
        handlers.onError?.(value.error);
      }
      if (value.ended) {
        handlers.onEnd?.();
      }
    },
  );
  try {
    await phone.subscribe(computerId, id, channel, params);
  } catch (error) {
    await unlisten();
    throw error;
  }
  return async () => {
    await unlisten();
    try {
      await phone.unsubscribe(id);
    } catch {
      // The subscription may already have stopped; the local close is enough.
    }
  };
}

/** Listens for connection state changes. */
export function onConnection(
  listener: (state: ConnectionState) => void,
): Promise<UnlistenFn> {
  return listen<ConnectionState>('phone:connection', (event) =>
    listener(event.payload),
  );
}

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
