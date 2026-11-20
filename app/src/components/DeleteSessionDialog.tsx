import { useState } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface DeleteSessionDialogProps {
  session: Session;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function DeleteSessionDialog({
  session,
  onConfirm,
  onClose,
}: DeleteSessionDialogProps) {
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
          : t('deleteSession.error'),
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="delete-session-title"
      descriptionId="delete-session-description"
      busyLabel={t('deleteSession.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="delete-session-title">
          {t('deleteSession.title')}
        </h2>
        <p className="dialog-sub" id="delete-session-description">
          {session.name}
        </p>
      </div>

      <div className="dialog-body close-body">
        <p className="close-copy">{t('deleteSession.copy')}</p>

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
          className="btn btn-danger"
          onClick={() => void confirm()}
          disabled={submitting}
        >
          {submitting
            ? t('deleteSession.submitting')
            : t('deleteSession.submit')}
        </button>
      </div>
    </Dialog>
  );
}
