import { useState } from 'react';
import { ApiError } from '../api/client';
import type { DemoServer } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface DemoFunnelDialogProps {
  server: DemoServer;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

/**
 * Asks before a demo server becomes reachable from the internet.
 *
 * The confirm button sends the request that carries the user's answer. A
 * failure keeps the server local and shows why.
 */
export function DemoFunnelDialog({
  server,
  onConfirm,
  onClose,
}: DemoFunnelDialogProps) {
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
        reason instanceof ApiError ? reason.message : t('funnel.error'),
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="demo-funnel-title"
      descriptionId="demo-funnel-description"
      busyLabel={t('funnel.enabling')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="demo-funnel-title">
          {t('funnel.title')}
        </h2>
        <p className="dialog-sub" id="demo-funnel-description">
          {server.name}
        </p>
      </div>

      <div className="dialog-body close-body">
        <p className="close-copy">{t('funnel.copy', { name: server.name })}</p>

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
          {submitting ? t('funnel.enabling') : t('funnel.confirm')}
        </button>
      </div>
    </Dialog>
  );
}
