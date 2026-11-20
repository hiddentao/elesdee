import { useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';

interface RenameSessionDialogProps {
  session: Session;
  onRename: (name: string) => Promise<void>;
  onClose: () => void;
}

export function RenameSessionDialog({
  session,
  onRename,
  onClose,
}: RenameSessionDialogProps) {
  const { t } = useLanguage();
  const [name, setName] = useState(session.name);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const trimmed = name.trim();
    if (trimmed.length === 0) {
      setError(t('rename.errorEmpty'));
      return;
    }
    if (trimmed.length > 64) {
      setError(t('rename.errorLong'));
      return;
    }

    setError(null);
    setSubmitting(true);
    try {
      await onRename(trimmed);
      onClose();
    } catch (reason) {
      setError(
        reason instanceof ApiError
          ? reason.message
          : t('rename.error'),
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="rename-session-title"
      descriptionId="rename-session-description"
      busyLabel={t('rename.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="rename-session-title">
          {t('rename.title')}
        </h2>
        <p className="dialog-sub" id="rename-session-description">
          {t('rename.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="rename-session-name">{t('rename.name')}</label>
            <input
              id="rename-session-name"
              className="text-input"
              value={name}
              maxLength={64}
              autoFocus
              required
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? 'rename-session-name-error' : undefined}
              onChange={(event) => {
                setName(event.target.value);
                setError(null);
              }}
            />
            {error ? (
              <p className="field-error" id="rename-session-name-error" role="alert">
                {error}
              </p>
            ) : (
              <p className="field-hint">{t('rename.hint')}</p>
            )}
          </div>
        </div>

        <div className="dialog-foot">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={onClose}
            disabled={submitting}
          >
            {t('common.cancel')}
          </button>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? t('rename.submitting') : t('rename.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
