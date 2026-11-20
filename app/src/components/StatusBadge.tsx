import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { statusShape, statusText, statusTone } from '../state/sessionStatus';

interface StatusBadgeProps {
  session: Session;
}

export function StatusBadge({ session }: StatusBadgeProps) {
  const { t } = useLanguage();
  const tone = statusTone(session);

  return (
    <span className="status-badge" data-status={tone}>
      <span
        className="status-marker status-badge-marker"
        data-status={tone}
        data-shape={statusShape(tone)}
        aria-hidden="true"
      />
      <span>{statusText(session, t)}</span>
    </span>
  );
}
