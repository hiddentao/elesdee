import type { Provider } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { usageStatusText } from '../state/usage';
import { Dialog } from './Dialog';
import { UsageLimitRow } from './UsageLimitRow';

interface UsageDialogProps {
  providers: Provider[];
  onClose: () => void;
}

export function UsageDialog({ providers, onClose }: UsageDialogProps) {
  const { t, language } = useLanguage();
  return (
    <Dialog
      titleId="usage-title"
      descriptionId="usage-description"
      onClose={onClose}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="usage-title">
          {t('usage.title')}
        </h2>
        <p className="dialog-sub" id="usage-description">
          {t('usage.subtitle')}
        </p>
      </div>

      <div className="dialog-body usage-panel">
        {providers.length === 0 ? (
          <p className="usage-empty">{t('usage.none')}</p>
        ) : (
          providers.map((provider) => {
            const status =
              usageStatusText(provider, language, t) ?? provider.foot;
            return (
              <section
                className="usage-card"
                key={provider.id}
                aria-label={provider.name}
              >
                <div className="usage-card-head">
                  <div className="usage-card-main">
                    <span className="usage-name">{provider.name}</span>
                    {provider.sample ? (
                      <span className="sample-tag">{t('common.sample')}</span>
                    ) : null}
                    <span className="usage-plan">
                      {provider.plan} · {provider.vendor}
                    </span>
                  </div>
                </div>

                {provider.rows.length === 0 ? (
                  <p className="usage-empty">{t('usage.none')}</p>
                ) : (
                  <div className="usage-card-rows">
                    {provider.rows.map((row, index) => (
                      <UsageLimitRow key={`${row.label}-${index}`} row={row} />
                    ))}
                  </div>
                )}

                {status ? <p className="usage-foot">{status}</p> : null}
              </section>
            );
          })
        )}
      </div>

      <div className="dialog-foot">
        <button
          type="button"
          className="btn btn-primary"
          onClick={onClose}
          autoFocus
        >
          {t('common.close')}
        </button>
      </div>
    </Dialog>
  );
}
