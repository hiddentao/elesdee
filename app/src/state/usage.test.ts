import { describe, expect, it } from 'vitest';
import type { Account } from '../api/types';
import {
  accountUsage,
  balanceBelowMin,
  rowWarn,
  tightestAccount,
} from './usage';

function makeAccount(overrides: Partial<Account> = {}): Account {
  return {
    id: 'a1',
    name: 'Work',
    plan: 'Max',
    client: 'Claude Code',
    provider: 'Anthropic',
    auth: 'subscription',
    rows: [],
    foot: '',
    server: null,
    model: null,
    ...overrides,
  };
}

describe('usage helpers', () => {
  it('reports the remaining percent from the highest used limit', () => {
    const usage = accountUsage(
      makeAccount({
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
    const usage = accountUsage(
      makeAccount({ rows: [{ label: '5-hour', used: 92, warn: true }] }),
    );

    expect(usage.summary).toBe('8% left');
    expect(usage.warn).toBe(true);
    expect(usage.critical).toBe(true);
  });

  it('uses a balance value and warns below the minimum', () => {
    const normal = accountUsage(
      makeAccount({
        rows: [{ label: 'Credit', value: '$12.40', min: '$5.00' }],
      }),
    );
    const low = accountUsage(
      makeAccount({
        rows: [{ label: 'Credit', value: '$3.20', min: '$5.00' }],
      }),
    );

    expect(normal.kind).toBe('balance');
    expect(normal.summary).toBe('$12.40');
    expect(normal.warn).toBe(false);
    expect(low.summary).toBe('$3.20');
    expect(low.warn).toBe(true);
    expect(balanceBelowMin({ label: 'Credit', value: '$3.20', min: '$5.00' })).toBe(
      true,
    );
  });

  it('reports no usage data when there are no rows', () => {
    const usage = accountUsage(makeAccount());

    expect(usage.kind).toBe('none');
    expect(usage.summary).toBe('No usage data');
    expect(usage.remaining).toBeNull();
    expect(usage.warn).toBe(false);
  });

  it('warns on a low remaining percent', () => {
    expect(rowWarn({ label: 'Weekly', used: 85 })).toBe(true);
    expect(rowWarn({ label: 'Weekly', used: 40 })).toBe(false);
  });

  it('finds the account with the least remaining usage', () => {
    const roomy = makeAccount({
      id: 'a1',
      rows: [{ label: 'Weekly', used: 20 }],
    });
    const tight = makeAccount({
      id: 'a2',
      name: 'Personal',
      rows: [{ label: 'Weekly', used: 90 }],
    });

    expect(tightestAccount([roomy, tight])?.id).toBe('a2');
    expect(tightestAccount([])).toBeNull();
  });
});
