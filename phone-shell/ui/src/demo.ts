import type { DemoServer } from './types';

/** The most demo servers that one session may register. */
export const MAX_DEMO_SERVERS = 8;

/** The progress of one phone listener. */
export type ForwardPhase = 'starting' | 'ready' | 'unavailable' | 'failed';

export interface ForwardView {
  phase: ForwardPhase;
  address?: string;
  /** The session that owns the tunnel. */
  sessionId?: string;
}

export type DemoStateKey =
  | 'demoServer.starting'
  | 'demoServer.ready'
  | 'demoServer.unavailable'
  | 'demoServer.failed';

export interface DemoServerRow {
  server: DemoServer;
  /** The address on this phone, when its listener is ready. */
  address: string | null;
  state: DemoStateKey;
}

/**
 * Selects the active demo servers that belong to one session.
 *
 * The list holds ports for every open session, so a session shows only its own.
 * A duplicate port keeps its first entry, and the session limit caps the rows.
 */
export function sessionDemoServers(
  value: unknown,
  sessionId: string,
): DemoServer[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const seen = new Set<number>();
  const servers: DemoServer[] = [];
  for (const entry of value) {
    if (!isDemoServer(entry) || entry.sessionId !== sessionId) {
      continue;
    }
    if (entry.state !== 'active' || seen.has(entry.port)) {
      continue;
    }
    seen.add(entry.port);
    servers.push(entry);
    if (servers.length === MAX_DEMO_SERVERS) {
      break;
    }
  }
  return servers;
}

/**
 * Joins the active servers with the state of their phone listeners.
 *
 * A tunnel belongs to the session that opened it, so a leftover entry for
 * another session does not mark this server as ready.
 */
export function demoServerRows(
  servers: readonly DemoServer[],
  forwards: ReadonlyMap<number, ForwardView>,
  sessionId: string,
): DemoServerRow[] {
  return servers.map((server) => {
    const forward = forwards.get(server.port);
    if (!forward || forward.sessionId !== sessionId) {
      return { server, address: null, state: 'demoServer.starting' };
    }
    if (forward.phase === 'ready' && forward.address) {
      return {
        server,
        address: forward.address,
        state: 'demoServer.ready',
      };
    }
    return {
      server,
      address: null,
      state:
        forward.phase === 'starting'
          ? 'demoServer.starting'
          : forward.phase === 'unavailable'
            ? 'demoServer.unavailable'
            : 'demoServer.failed',
    };
  });
}

/** Returns the class that colours one listener state. */
export function demoStateClass(state: DemoStateKey): string {
  switch (state) {
    case 'demoServer.ready':
      return 'demo-state is-ready';
    case 'demoServer.starting':
      return 'demo-state is-starting';
    case 'demoServer.unavailable':
      return 'demo-state is-unavailable';
    default:
      return 'demo-state is-failed';
  }
}

/** Maps a shell error code to the listener state that the row shows. */
export function forwardPhaseFor(code: string): ForwardPhase {
  if (code === 'unavailable' || code === 'not_registered' || code === 'refused') {
    return 'unavailable';
  }
  return 'failed';
}

function isDemoServer(value: unknown): value is DemoServer {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const server = value as Partial<DemoServer>;
  return (
    typeof server.id === 'string' &&
    typeof server.sessionId === 'string' &&
    typeof server.name === 'string' &&
    typeof server.port === 'number' &&
    Number.isInteger(server.port) &&
    server.port > 0 &&
    server.port <= 65535 &&
    typeof server.state === 'string'
  );
}
