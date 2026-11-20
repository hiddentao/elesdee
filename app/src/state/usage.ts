import type { Provider, UsageRow } from '../api/types';
import {
  formatNumber,
  translate,
  type Language,
  type Translate,
} from '../i18n';

export function rowLabel(row: UsageRow, t: Translate): string {
  if (row.window === 'five_hour') {
    return t('usage.fiveHour');
  }
  if (row.window === 'seven_day') {
    return t('usage.weekly');
  }
  if (row.window === 'balance') {
    return t('usage.balance');
  }
  return row.label;
}

export function formatUsageTime(
  value: string | null | undefined,
  language: Language,
): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return new Intl.DateTimeFormat(language, {
    dateStyle: 'short',
    timeStyle: 'short',
  }).format(date);
}

export function usageStatusText(
  provider: Provider,
  language: Language,
  t: Translate,
): string | null {
  if (provider.usageStatus === 'fresh') {
    const time = formatUsageTime(provider.usageReadAt, language);
    return time ? t('usage.readAt', { time }) : null;
  }
  if (provider.usageStatus === 'stale') {
    const time = formatUsageTime(
      provider.usageReadAt ?? provider.usageCheckedAt,
      language,
    );
    return time ? t('usage.stale', { time }) : null;
  }
  if (provider.usageStatus === 'unknown') {
    const time = formatUsageTime(provider.usageCheckedAt, language);
    return time ? t('usage.checked', { time }) : null;
  }
  return null;
}

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

export interface ProviderUsage {
  kind: 'percent' | 'balance' | 'none';
  summary: string;
  remaining: number | null;
  warn: boolean;
  critical: boolean;
  tightestRow: UsageRow | null;
}

export function providerUsage(
  provider: Provider,
  language: Language = 'en',
): ProviderUsage {
  const limits = provider.rows.filter((row) => rowRemaining(row) !== null);

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

  const balances = provider.rows.filter(
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

export function tightestProvider(providers: Provider[]): Provider | null {
  let tightest: Provider | null = null;
  let lowest = Infinity;

  for (const provider of providers) {
    const usage = providerUsage(provider);
    if (usage.remaining !== null && usage.remaining < lowest) {
      lowest = usage.remaining;
      tightest = provider;
    }
  }

  if (tightest) {
    return tightest;
  }

  return providers.find((provider) => provider.rows.length > 0) ?? providers[0] ?? null;
}
