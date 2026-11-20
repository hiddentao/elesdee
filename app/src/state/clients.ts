import type { AgentCheck, Client } from '../api/types';

/** Drops the parenthetical build details from a version string. */
export function shortVersion(version: string): string {
  return version.replace(/\s*\([^)]*\)\s*$/, '').trim();
}

/** Returns the health check for a client, or null when it is unknown. */
export function agentFor(
  agents: AgentCheck[],
  client: Client,
): AgentCheck | null {
  return agents.find((agent) => agent.name === client) ?? null;
}
