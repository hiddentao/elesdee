import type { UpdateSnapshot } from '../api/updater';
import type { Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';

interface UpdatePanelProps {
  update: UpdateSnapshot | null;
  onCheck: () => void;
  onInstall: () => void;
}

function errorText(update: UpdateSnapshot, t: Translate): string {
  switch (update.error) {
    case 'network':
      return t('settings.updateErrorNetwork');
    case 'signature':
      return t('settings.updateErrorSignature');
    case 'downloadFailed':
      return t('settings.updateErrorDownload');
    case 'installFailed':
      return t('settings.updateErrorInstall');
    default:
      return t('settings.updateErrorCheck');
  }
}

function stateText(update: UpdateSnapshot, t: Translate): string {
  const version = update.version ?? '';
  switch (update.state) {
    case 'unavailable':
      return t('settings.updateUnavailable');
    case 'idle':
      return t('settings.updateIdle');
    case 'checking':
      return t('settings.updateChecking');
    case 'downloading':
      return update.version
        ? t('settings.updateDownloading', { version })
        : t('settings.updateDownloadingUnknown');
    case 'ready':
      return t('settings.updateReady', { version });
    case 'upToDate':
      return t('settings.updateUpToDate');
    case 'deferred':
      return t('settings.updateDeferred', { version });
    case 'installing':
      return t('settings.updateInstalling', { version });
    default:
      return errorText(update, t);
  }
}

export function UpdatePanel({ update, onCheck, onInstall }: UpdatePanelProps) {
  const { t } = useLanguage();
  if (!update) {
    return null;
  }

  const action =
    update.state === 'ready' || update.state === 'deferred'
      ? { label: t('settings.updateInstall'), run: onInstall }
      : update.state === 'error'
        ? { label: t('settings.updateRetry'), run: onCheck }
        : update.state === 'idle' || update.state === 'upToDate'
          ? { label: t('settings.updateCheck'), run: onCheck }
          : null;

  return (
    <div className="settings-update">
      <p
        className={
          update.state === 'error'
            ? 'settings-status is-warn'
            : 'settings-status'
        }
        role="status"
      >
        {stateText(update, t)}
      </p>
      {update.state === 'downloading' && update.progress !== null ? (
        <div
          className="settings-update-progress"
          role="progressbar"
          aria-label={t('settings.updateProgressLabel')}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={update.progress}
        >
          <span style={{ width: `${update.progress}%` }} />
        </div>
      ) : null}
      {action ? (
        <div className="settings-actions">
          <button type="button" className="btn btn-primary" onClick={action.run}>
            {action.label}
          </button>
        </div>
      ) : null}
    </div>
  );
}
