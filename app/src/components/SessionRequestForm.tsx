import { useState } from 'react';
import type { SessionRequest } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';

interface SessionRequestFormProps {
  request: SessionRequest;
  onApprove: (allowFurther: boolean) => Promise<void>;
  onDeny: () => Promise<void>;
}

export function SessionRequestForm({
  request,
  onApprove,
  onDeny,
}: SessionRequestFormProps) {
  const { t } = useLanguage();
  const [allowFurther, setAllowFurther] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch {
      setBusy(false);
      setError(t('requests.error'));
    }
  }

  const summaryParts = t('requests.summary').split(
    /(\{parent\}|\{project\})/,
  );

  return (
    <div className="request-card">
      <p className="request-text">
        {summaryParts.map((part, index) => {
          if (part === '{parent}') {
            return <strong key={index}>{request.parentSessionName}</strong>;
          }
          if (part === '{project}') {
            return <strong key={index}>{request.projectName}</strong>;
          }
          return <span key={index}>{part}</span>;
        })}
      </p>
      <dl className="request-details">
        <div>
          <dt>{t('requests.client')}</dt>
          <dd>{request.client}</dd>
        </div>
        {request.name ? (
          <div>
            <dt>{t('requests.name')}</dt>
            <dd>{request.name}</dd>
          </div>
        ) : null}
        {request.firstPrompt ? (
          <div>
            <dt>{t('requests.firstPrompt')}</dt>
            <dd>{request.firstPrompt}</dd>
          </div>
        ) : null}
        {request.sourceBranch ? (
          <div>
            <dt>{t('requests.sourceBranch')}</dt>
            <dd>{request.sourceBranch}</dd>
          </div>
        ) : null}
      </dl>
      <label className="request-allow">
        <input
          type="checkbox"
          checked={allowFurther}
          disabled={busy}
          onChange={(event) => setAllowFurther(event.target.checked)}
        />
        {t('requests.allowFurther')}
      </label>
      <div className="request-actions">
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy}
          onClick={() => void run(onDeny)}
        >
          {t('permission.deny')}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => void run(() => onApprove(allowFurther))}
        >
          {busy ? t('requests.starting') : t('requests.approve')}
        </button>
      </div>
      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
