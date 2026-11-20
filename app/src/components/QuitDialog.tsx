import { useState } from 'react';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';

interface QuitDialogProps {
  sessionCount: number;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function QuitDialog({
  sessionCount,
  onConfirm,
  onClose,
}: QuitDialogProps) {
  const { t } = useLanguage();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
    } catch {
      setSubmitting(false);
      setError(t('quit.error'));
    }
  }

  return (
    <Dialog
      titleId="quit-title"
      descriptionId="quit-description"
      busyLabel={t('quit.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="quit-title">
          {t('quit.title')}
        </h2>
        <p className="dialog-sub" id="quit-description">
          {sessionCount === 1
            ? t('quit.oneRunning')
            : t('quit.manyRunning', { count: sessionCount })}
        </p>
      </div>
      <div className="dialog-body">
        <p className="close-copy">{t('quit.copy')}</p>
        {error ? (
          <div className="form-error" role="alert">
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
          {submitting ? t('quit.submitting') : t('quit.submit')}
        </button>
      </div>
    </Dialog>
  );
}
