import { describe, expect, it } from 'vitest';
import type { Provider } from '../api/types';
import { translate, type Translate } from '../i18n';
import {
  providerUsage,
  balanceBelowMin,
  balanceCurrency,
  formatUsageTime,
  rowLabel,
  rowWarn,
  tightestProvider,
  usageStatusText,
} from './usage';

const t: Translate = (key, values) => translate('en', key, values);

function makeProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    vendor: 'anthropic',
    auth: 'subscription',
    sample: true,
    connected: false,
    rows: [],
    foot: '',
    server: null,
    supportedClients: ['Claude Code', 'OpenCode 2'],
    ...overrides,
  };
}

describe('usage helpers', () => {
  it('reports the remaining percent from the highest used limit', () => {
    const usage = providerUsage(
      makeProvider({
        rows: [
          { label: 'Weekly', used: 20 },
          { label: '5-hour', used: 58 },
        ],
      }),
    );

    expect(usage.kind).toBe('percent');
    expect(usage.summary).toBe('42% left');
    expect(usage.remaining).toBe(42);
    expect(usage.tightestRow?.label).toBe('5-hour');
    expect(usage.warn).toBe(false);
    expect(usage.critical).toBe(false);
  });

  it('marks a tight percent limit', () => {
    const usage = providerUsage(
      makeProvider({ rows: [{ label: '5-hour', used: 92, warn: true }] }),
    );

    expect(usage.summary).toBe('8% left');
    expect(usage.warn).toBe(true);
    expect(usage.critical).toBe(true);
  });

  it('uses a balance value and warns at or below the minimum', () => {
    const normal = providerUsage(
      makeProvider({
        minimumBalance: 5,
        rows: [{ label: 'Credit', value: 'USD 12.40' }],
      }),
    );
    const low = providerUsage(
      makeProvider({
        minimumBalance: 5,
        rows: [{ label: 'Credit', value: 'USD 3.20' }],
      }),
    );
    const equal = providerUsage(
      makeProvider({
        minimumBalance: 5,
        rows: [{ label: 'Credit', value: 'USD 5.00' }],
      }),
    );

    expect(normal.kind).toBe('balance');
    expect(normal.summary).toBe('USD 12.40');
    expect(normal.warn).toBe(false);
    expect(low.summary).toBe('USD 3.20');
    expect(low.warn).toBe(true);
    expect(equal.warn).toBe(true);
    expect(balanceBelowMin({ label: 'Credit', value: 'USD 3.20' }, 5)).toBe(
      true,
    );
    expect(balanceBelowMin({ label: 'Credit', value: 'USD 5.00' }, 5)).toBe(
      true,
    );
    expect(balanceBelowMin({ label: 'Credit', value: 'USD 5.01' }, 5)).toBe(
      false,
    );
    expect(balanceBelowMin({ label: 'Credit', value: 'USD 1.00' })).toBe(false);
  });

  it('reads the currency that a balance row reports', () => {
    expect(
      balanceCurrency(
        makeProvider({
          rows: [{ label: 'Balance', window: 'balance', value: 'USD 12.00' }],
        }),
      ),
    ).toBe('USD');
    expect(balanceCurrency(makeProvider())).toBeNull();
    expect(
      balanceCurrency(
        makeProvider({ rows: [{ label: 'Credit', value: '$12.00' }] }),
      ),
    ).toBeNull();
  });

  it('reports no usage data when there are no rows', () => {
    const usage = providerUsage(makeProvider());

    expect(usage.kind).toBe('none');
    expect(usage.summary).toBe('No usage data');
    expect(usage.remaining).toBeNull();
    expect(usage.warn).toBe(false);
  });

  it('warns on a low remaining percent', () => {
    expect(rowWarn({ label: 'Weekly', used: 85 })).toBe(true);
    expect(rowWarn({ label: 'Weekly', used: 40 })).toBe(false);
  });

  it('names known usage windows from the catalog', () => {
    expect(rowLabel({ label: 'ignored', window: 'five_hour' }, t)).toBe(
      '5-hour limit',
    );
    expect(rowLabel({ label: 'ignored', window: 'seven_day' }, t)).toBe(
      'Weekly limit',
    );
    expect(rowLabel({ label: 'ignored', window: 'balance' }, t)).toBe(
      'Balance',
    );
    expect(rowLabel({ label: 'Custom', window: 'other' }, t)).toBe('Custom');
  });

  it('reports when the usage was read and whether the read is stale', () => {
    const readAt = '2026-11-20T10:00:00Z';
    const time = formatUsageTime(readAt, 'en');
    expect(time).not.toBeNull();

    const fresh = usageStatusText(
      makeProvider({ usageStatus: 'fresh', usageReadAt: readAt }),
      'en',
      t,
    );
    expect(fresh).toBe(`Read ${time}.`);

    const stale = usageStatusText(
      makeProvider({
        usageStatus: 'stale',
        usageReadAt: readAt,
        usageCheckedAt: '2026-11-20T11:00:00Z',
      }),
      'en',
      t,
    );
    expect(stale).toBe(`Last read ${time}. The latest read failed.`);

    const unknownTime = formatUsageTime('2026-11-20T11:00:00Z', 'en');
    const unknown = usageStatusText(
      makeProvider({
        usageStatus: 'unknown',
        usageCheckedAt: '2026-11-20T11:00:00Z',
      }),
      'en',
      t,
    );
    expect(unknown).toBe(`No usage data. Checked ${unknownTime}.`);
    expect(usageStatusText(makeProvider(), 'en', t)).toBeNull();
  });

  it('finds the account with the least remaining usage', () => {
    const roomy = makeProvider({
      id: 'a1',
      rows: [{ label: 'Weekly', used: 20 }],
    });
    const tight = makeProvider({
      id: 'a2',
      name: 'Personal',
      rows: [{ label: 'Weekly', used: 90 }],
    });

    expect(tightestProvider([roomy, tight])?.id).toBe('a2');
    expect(tightestProvider([])).toBeNull();
  });
});
