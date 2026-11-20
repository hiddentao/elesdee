import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { statusText, statusTone } from '../state/sessionStatus';

interface StatusBadgeProps {
  session: Session;
}

export function StatusBadge({ session }: StatusBadgeProps) {
  const { t } = useLanguage();
  const closed = session.status === 'closed';

  return (
    <span
      className="status-badge"
      data-status={statusTone(session)}
      title={statusText(session, t)}
    >
      <span className="status-dot" aria-hidden="true" />
      <span>{closed ? t('status.closed') : t('status.active')}</span>
    </span>
  );
}
