import { useState } from 'react';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';

interface UpdateInstallDialogProps {
  sessionCount: number;
  onConfirm: () => Promise<void>;
  onLater: () => Promise<void>;
  onClose: () => void;
}

export function UpdateInstallDialog({
  sessionCount,
  onConfirm,
  onLater,
  onClose,
}: UpdateInstallDialogProps) {
  const { t } = useLanguage();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setSubmitting(true);
    setError(null);
    try {
      await action();
    } catch {
      setSubmitting(false);
      setError(t('updateInstall.error'));
    }
  }

  return (
    <Dialog
      titleId="update-install-title"
      descriptionId="update-install-description"
      busyLabel={t('updateInstall.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="update-install-title">
          {t('updateInstall.title')}
        </h2>
        <p className="dialog-sub" id="update-install-description">
          {sessionCount === 1
            ? t('updateInstall.oneRunning')
            : t('updateInstall.manyRunning', { count: sessionCount })}
        </p>
      </div>
      <div className="dialog-body">
        <p className="close-copy">{t('updateInstall.copy')}</p>
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
          onClick={() => void run(onLater)}
          disabled={submitting}
          autoFocus
        >
          {t('updateInstall.later')}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => void run(onConfirm)}
          disabled={submitting}
        >
          {t('updateInstall.submit')}
        </button>
      </div>
    </Dialog>
  );
}
