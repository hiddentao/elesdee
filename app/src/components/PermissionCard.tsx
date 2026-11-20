import { useId, useState } from 'react';
import type {
  PendingPermission,
  PermissionDecision,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { AlertIcon } from './icons';

interface PermissionCardProps {
  permission: PendingPermission;
  client: string | null;
  sessionName: string;
  onRespond: (decision: PermissionDecision) => Promise<void>;
}

export function PermissionCard({
  permission,
  client,
  sessionName,
  onRespond,
}: PermissionCardProps) {
  const { t } = useLanguage();
  const titleId = useId();
  const [decision, setDecision] = useState<PermissionDecision | null>(null);
  const [error, setError] = useState('');

  async function answer(choice: PermissionDecision) {
    if (decision) {
      return;
    }

    setDecision(choice);
    setError('');

    try {
      await onRespond(choice);
    } catch {
      setDecision(null);
      setError(t('permission.error'));
    }
  }

  return (
    <section
      className="permission-card"
      aria-labelledby={titleId}
      aria-busy={decision !== null}
    >
      <div className="permission-head">
        <span className="permission-icon" aria-hidden="true">
          <AlertIcon />
        </span>
        <div className="permission-head-text">
          <h2 className="permission-title" id={titleId}>
            {t('permission.needed')}
          </h2>
          <span className="permission-context">
            {client ?? t('status.agent')} · {sessionName}
          </span>
        </div>
      </div>

      <div className="permission-copy" role="status">
        <p className="permission-tool">
          <span className="permission-label">{t('permission.tool')}</span>{' '}
          {permission.tool}
        </p>
        <pre className="permission-summary">{permission.summary}</pre>
        <p className="permission-rationale">{t('permission.paused')}</p>
      </div>

      <div className="permission-actions">
        <button
          type="button"
          className="btn btn-ghost"
          disabled={decision !== null}
          onClick={() => void answer('deny')}
        >
          {decision === 'deny'
            ? t('permission.denying')
            : t('permission.deny')}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={decision !== null}
          onClick={() => void answer('allow')}
        >
          {decision === 'allow'
            ? t('permission.allowing')
            : t('permission.allow')}
        </button>
      </div>

      {error ? (
        <p className="permission-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
