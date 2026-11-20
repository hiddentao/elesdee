import { useState } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface CloseSessionDialogProps {
  session: Session;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function CloseSessionDialog({
  session,
  onConfirm,
  onClose,
}: CloseSessionDialogProps) {
  const { t } = useLanguage();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
    } catch (reason) {
      setError(
        reason instanceof ApiError
          ? reason.message
          : t('closeSession.error'),
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="close-session-title"
      descriptionId="close-session-description"
      busyLabel={t('closeSession.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="close-session-title">
          {t('closeSession.title')}
        </h2>
        <p className="dialog-sub" id="close-session-description">
          {session.name} ·{' '}
          {session.kind === 'terminal'
            ? t('closeSession.subtitleTerminal')
            : t('closeSession.subtitleAgent')}
          {session.branch ? ` · ${session.branch}` : ''}
        </p>
      </div>

      <div className="dialog-body close-body">
        {session.kind === 'terminal' ? (
          <p className="close-copy">{t('closeSession.terminalCopy')}</p>
        ) : (
          <p className="close-copy">{t('closeSession.agentCopy')}</p>
        )}

        {error ? (
          <div className="form-error" role="alert">
            <AlertIcon />
            <span>{error}</span>
          </div>
        ) : null}
      </div>

      <div className="dialog-foot">
        <button
          type="button"
          className="btn btn-ghost"
          onClick={onClose}
          disabled={submitting}
          autoFocus
        >
          {t('common.cancel')}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => void confirm()}
          disabled={submitting}
        >
          {submitting ? t('closeSession.submitting') : t('closeSession.submit')}
        </button>
      </div>
    </Dialog>
  );
}
