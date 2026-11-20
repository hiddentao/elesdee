import type { SessionRequest } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Dialog } from './Dialog';
import { SessionRequestForm } from './SessionRequestForm';

interface RequestsDialogProps {
  requests: SessionRequest[];
  onApprove: (id: string, allowFurther: boolean) => Promise<void>;
  onDeny: (id: string) => Promise<void>;
  onClose: () => void;
}

export function RequestsDialog({
  requests,
  onApprove,
  onDeny,
  onClose,
}: RequestsDialogProps) {
  const { t } = useLanguage();
  return (
    <Dialog
      titleId="requests-title"
      descriptionId="requests-description"
      className="questions-dialog"
      onClose={onClose}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="requests-title">
          {t('requests.title')}
        </h2>
        <p className="dialog-sub" id="requests-description">
          {t('requests.subtitle')}
        </p>
      </div>
      <div className="dialog-body questions-body">
        {requests.length === 0 ? (
          <p className="empty-text">{t('requests.none')}</p>
        ) : (
          requests.map((request) => (
            <SessionRequestForm
              key={request.id}
              request={request}
              onApprove={(allowFurther) => onApprove(request.id, allowFurther)}
              onDeny={() => onDeny(request.id)}
            />
          ))
        )}
      </div>
      <div className="dialog-foot">
        <button type="button" className="btn btn-ghost" onClick={onClose}>
          {t('common.close')}
        </button>
      </div>
    </Dialog>
  );
}
