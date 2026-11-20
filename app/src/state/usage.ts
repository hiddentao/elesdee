import type { Account, UsageRow } from '../api/types';
import {
  formatNumber,
  translate,
  type Language,
} from '../i18n';

function parseNumber(value: string | undefined): number | null {
  if (!value) {
    return null;
  }

  const match = value.replace(/,/g, '').match(/-?\d+(?:\.\d+)?/);
  return match ? Number(match[0]) : null;
}

export function rowRemaining(row: UsageRow): number | null {
  if (typeof row.used !== 'number' || !Number.isFinite(row.used)) {
    return null;
  }

  return Math.max(0, Math.min(100, 100 - row.used));
}

export function balanceBelowMin(row: UsageRow): boolean {
  if (typeof row.used === 'number') {
    return false;
  }

  const value = parseNumber(row.value);
  const min = parseNumber(row.min);
  return value !== null && min !== null && value < min;
}

export function rowWarn(row: UsageRow): boolean {
  if (row.warn) {
    return true;
  }

  const remaining = rowRemaining(row);
  if (remaining !== null) {
    return remaining <= 20;
  }

  return balanceBelowMin(row);
}

export interface AccountUsage {
  kind: 'percent' | 'balance' | 'none';
  summary: string;
  remaining: number | null;
  warn: boolean;
  critical: boolean;
  tightestRow: UsageRow | null;
}

export function accountUsage(
  account: Account,
  language: Language = 'en',
): AccountUsage {
  const limits = account.rows.filter((row) => rowRemaining(row) !== null);

  if (limits.length > 0) {
    let tightest = limits[0];
    let remaining = rowRemaining(tightest) as number;

    for (const row of limits) {
      const next = rowRemaining(row) as number;
      if (next < remaining) {
        tightest = row;
        remaining = next;
      }
    }

    return {
      kind: 'percent',
      summary: translate(language, 'usage.percentLeft', {
        percent: formatNumber(language, Math.round(remaining)),
      }),
      remaining,
      warn: rowWarn(tightest),
      critical: remaining <= 10,
      tightestRow: tightest,
    };
  }

  const balances = account.rows.filter(
    (row) => typeof row.used !== 'number' && Boolean(row.value),
  );

  if (balances.length > 0) {
    const first = balances[0];
    return {
      kind: 'balance',
      summary: first.value ?? translate(language, 'usage.none'),
      remaining: null,
      warn: rowWarn(first),
      critical: false,
      tightestRow: first,
    };
  }

  return {
    kind: 'none',
    summary: translate(language, 'usage.none'),
    remaining: null,
    warn: false,
    critical: false,
    tightestRow: null,
  };
}

export function tightestAccount(accounts: Account[]): Account | null {
  let tightest: Account | null = null;
  let lowest = Infinity;

  for (const account of accounts) {
    const usage = accountUsage(account);
    if (usage.remaining !== null && usage.remaining < lowest) {
      lowest = usage.remaining;
      tightest = account;
    }
  }

  if (tightest) {
    return tightest;
  }

  return accounts.find((account) => account.rows.length > 0) ?? accounts[0] ?? null;
}
