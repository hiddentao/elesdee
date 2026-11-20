import type { UsageRow } from '../api/types';
import { formatNumber } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import {
  balanceBelowMin,
  rowLabel,
  rowRemaining,
  rowWarn,
} from '../state/usage';

export function UsageLimitRow({ row }: { row: UsageRow }) {
  const { t, language } = useLanguage();
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
      <span className="usage-limit-label">{rowLabel(row, t)}</span>
      {remaining !== null ? (
        <>
          <span className="usage-limit-bar" aria-hidden="true">
            <span
              className="usage-limit-fill"
              style={{ width: `${remaining}%` }}
            />
          </span>
          <span className="usage-limit-value">
            {t('usage.percentLeft', {
              percent: formatNumber(language, Math.round(remaining)),
            })}
          </span>
        </>
      ) : (
        <span className="usage-limit-value">
          {row.value ?? '—'}
          {lowBalance ? (
            <span className="sr-only"> {t('usage.belowMinimum')}</span>
          ) : null}
        </span>
      )}
    </span>
  );
}
