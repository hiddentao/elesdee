import type { UsageRow } from '../api/types';
import { balanceBelowMin, rowRemaining, rowWarn } from '../state/usage';

export function UsageLimitRow({ row }: { row: UsageRow }) {
  const remaining = rowRemaining(row);
  const warn = rowWarn(row);
  const lowBalance = balanceBelowMin(row);
  const classes = [
    'usage-limit',
    warn ? 'is-warn' : '',
    remaining === null ? 'is-text' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <span className={classes}>
      <span className="usage-limit-label">{row.label}</span>
      {remaining !== null ? (
        <>
          <span className="usage-limit-bar" aria-hidden="true">
            <span
              className="usage-limit-fill"
              style={{ width: `${remaining}%` }}
            />
          </span>
          <span className="usage-limit-value">
            {Math.round(remaining)}% left
          </span>
        </>
      ) : (
        <span className="usage-limit-value">
          {row.value ?? '—'}
          {lowBalance ? (
            <span className="sr-only"> Below the minimum balance.</span>
          ) : null}
        </span>
      )}
    </span>
  );
}
