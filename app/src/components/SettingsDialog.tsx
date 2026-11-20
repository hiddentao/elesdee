import { useState } from 'react';
import { ApiError } from '../api/client';
import type {
  AiConfig,
  CreateConfigInput,
  CreateProviderInput,
  Provider,
  ProviderOption,
  UpdateConfigInput,
  UpdateProviderInput,
} from '../api/types';
import { LANGUAGES, type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { providerUsage } from '../state/usage';
import { AiConfigDialog } from './AiConfigDialog';
import { Dialog } from './Dialog';
import { ProviderDialog } from './ProviderDialog';
import {
  CloseIcon,
  GlobeIcon,
  LayersIcon,
  PencilIcon,
  PlusIcon,
  PresetIcon,
  SparkIcon,
  TrashIcon,
} from './icons';

type SettingsSection = 'providers' | 'configs' | 'languages' | 'about';

interface SettingsDialogProps {
  providers: Provider[];
  configs: AiConfig[];
  options: ProviderOption[];
  version: string | null;
  onCreateProvider: (input: CreateProviderInput) => Promise<Provider>;
  onUpdateProvider: (
    id: string,
    input: UpdateProviderInput,
  ) => Promise<Provider>;
  onRemoveProvider: (id: string) => Promise<void>;
  onCreateConfig: (input: CreateConfigInput) => Promise<AiConfig>;
  onUpdateConfig: (
    id: string,
    input: UpdateConfigInput,
  ) => Promise<AiConfig>;
  onDeleteConfig: (id: string) => Promise<void>;
  onProviderSaved: (provider: Provider) => void;
  onConfigSaved: (config: AiConfig) => void;
  onClose: () => void;
}

export function SettingsDialog({
  providers,
  configs,
  options,
  version,
  onCreateProvider,
  onUpdateProvider,
  onRemoveProvider,
  onCreateConfig,
  onUpdateConfig,
  onDeleteConfig,
  onProviderSaved,
  onConfigSaved,
  onClose,
}: SettingsDialogProps) {
  const { t, language, setLanguage } = useLanguage();
  const [section, setSection] = useState<SettingsSection>('providers');
  const [addProviderOpen, setAddProviderOpen] = useState(false);
  const [editProvider, setEditProvider] = useState<Provider | null>(null);
  const [removeProviderTarget, setRemoveProviderTarget] =
    useState<Provider | null>(null);
  const [removeProviderError, setRemoveProviderError] = useState<string | null>(
    null,
  );
  const [removingProvider, setRemovingProvider] = useState(false);
  const [configEditor, setConfigEditor] = useState<AiConfig | null | undefined>(
    undefined,
  );
  const [deleteConfigTarget, setDeleteConfigTarget] =
    useState<AiConfig | null>(null);
  const [deleteConfigError, setDeleteConfigError] = useState<string | null>(
    null,
  );
  const [deletingConfig, setDeletingConfig] = useState(false);
  const [configProviderId, setConfigProviderId] = useState<string | null>(null);

  async function confirmRemoveProvider() {
    if (!removeProviderTarget) {
      return;
    }
    setRemovingProvider(true);
    setRemoveProviderError(null);
    try {
      await onRemoveProvider(removeProviderTarget.id);
      setRemoveProviderTarget(null);
    } catch (error) {
      setRemoveProviderError(
        error instanceof ApiError
          ? error.message
          : t('removeAccount.error'),
      );
    } finally {
      setRemovingProvider(false);
    }
  }

  async function confirmDeleteConfig() {
    if (!deleteConfigTarget) {
      return;
    }
    setDeletingConfig(true);
    setDeleteConfigError(null);
    try {
      await onDeleteConfig(deleteConfigTarget.id);
      setDeleteConfigTarget(null);
    } catch (error) {
      setDeleteConfigError(
        error instanceof ApiError ? error.message : t('presets.errorDelete'),
      );
    } finally {
      setDeletingConfig(false);
    }
  }

  const busy =
    addProviderOpen ||
    editProvider !== null ||
    removeProviderTarget !== null ||
    configEditor !== undefined ||
    deleteConfigTarget !== null;

  return (
    <>
      <Dialog
        titleId="settings-title"
        descriptionId="settings-description"
        className="settings-dialog"
        onClose={onClose}
        inert={busy}
      >
        <div className="settings-head">
          <div>
            <h2 className="settings-page-title" id="settings-title">
              {t('settings.title')}
            </h2>
            <p className="settings-page-sub" id="settings-description">
              {t('settings.subtitle')}
            </p>
          </div>
          <button
            type="button"
            className="btn btn-ghost settings-close"
            aria-label={t('settings.close')}
            onClick={onClose}
            autoFocus
          >
            <CloseIcon />
          </button>
        </div>

        <div className="settings-page-body">
          <nav className="settings-nav" aria-label={t('settings.sections')}>
            <button
              type="button"
              className={
                section === 'providers'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'providers' ? 'page' : undefined}
              onClick={() => setSection('providers')}
            >
              <LayersIcon />
              <span>{t('settings.providers')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'configs'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'configs' ? 'page' : undefined}
              onClick={() => setSection('configs')}
            >
              <PresetIcon />
              <span>{t('settings.configs')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'languages'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'languages' ? 'page' : undefined}
              onClick={() => setSection('languages')}
            >
              <GlobeIcon />
              <span>{t('settings.languages')}</span>
            </button>
            <button
              type="button"
              className={
                section === 'about'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'about' ? 'page' : undefined}
              onClick={() => setSection('about')}
            >
              <SparkIcon />
              <span>{t('settings.about')}</span>
            </button>
            {version ? (
              <span className="settings-nav-version">
                {t('settings.versionLabel', { version })}
              </span>
            ) : null}
          </nav>

          <div className="settings-content">
            {section === 'providers' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-providers-title"
              >
                <div className="settings-section-head">
                  <div>
                    <h3
                      className="settings-section-title"
                      id="settings-providers-title"
                    >
                      {t('settings.providers')}
                    </h3>
                    <p className="settings-section-note">
                      {t('settings.providersNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => setAddProviderOpen(true)}
                  >
                    <PlusIcon />
                    {t('providers.add')}
                  </button>
                </div>

                {providers.length === 0 ? (
                  <p className="settings-empty">{t('providers.none')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {providers.map((provider) => {
                      const usage = providerUsage(provider, language);
                      const used = configs.filter(
                        (config) => config.providerId === provider.id,
                      ).length;
                      return (
                        <li key={provider.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {provider.name}
                              </span>
                              {provider.sample ? (
                                <span className="sample-tag">
                                  {t('common.sample')}
                                </span>
                              ) : null}
                            </div>
                            <span className="settings-account-sub">
                              {provider.supportedClients.join(' · ')} ·{' '}
                              {provider.plan}
                            </span>
                            <span className="settings-account-sub">
                              {t('providers.usedBy', { count: used })}
                            </span>
                          </div>
                          <span
                            className={
                              usage.warn
                                ? 'settings-account-usage is-warn'
                                : 'settings-account-usage'
                            }
                          >
                            {usage.summary}
                          </span>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('settings.editAccount', {
                                name: provider.name,
                              })}
                              onClick={() => setEditProvider(provider)}
                            >
                              <PencilIcon />
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('settings.removeAccount', {
                                name: provider.name,
                              })}
                              onClick={() => {
                                setRemoveProviderError(null);
                                setRemoveProviderTarget(provider);
                              }}
                            >
                              <TrashIcon />
                            </button>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'configs' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-configs-title"
              >
                <div className="settings-section-head">
                  <div>
                    <h3
                      className="settings-section-title"
                      id="settings-configs-title"
                    >
                      {t('settings.configs')}
                    </h3>
                    <p className="settings-section-note">
                      {t('settings.configsNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => {
                      setConfigProviderId(null);
                      setConfigEditor(null);
                    }}
                  >
                    <PlusIcon />
                    {t('configs.add')}
                  </button>
                </div>

                {configs.length === 0 ? (
                  <p className="settings-empty">{t('configs.none')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {configs.map((config) => {
                      const provider =
                        providers.find(
                          (item) => item.id === config.providerId,
                        ) ?? null;
                      const envCount = config.env.length;
                      const configCount = Object.keys(config.config).length;
                      return (
                        <li key={config.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {config.name}
                              </span>
                            </div>
                            <span className="settings-account-sub">
                              {provider?.name ?? config.providerId} ·{' '}
                              {config.client}
                              {config.model
                                ? ` · ${t('newSession.modelWith', {
                                    model: config.model,
                                  })}`
                                : ''}
                              {config.effort
                                ? ` · ${t('configs.effortLabel', {
                                    effort: config.effort,
                                  })}`
                                : ''}
                            </span>
                            <span className="settings-account-sub">
                              {envCount > 0
                                ? t(
                                    envCount === 1
                                      ? 'newSession.envOverrideOne'
                                      : 'newSession.envOverrideMany',
                                    { count: envCount },
                                  )
                                : ''}
                              {envCount > 0 && configCount > 0 ? ' · ' : ''}
                              {configCount > 0
                                ? t(
                                    configCount === 1
                                      ? 'newSession.configOverrideOne'
                                      : 'newSession.configOverrideMany',
                                    { count: configCount },
                                  )
                                : ''}
                              {envCount === 0 && configCount === 0
                                ? t('newSession.overridesNone')
                                : ''}
                            </span>
                          </div>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('presets.edit', {
                                name: config.name,
                              })}
                              onClick={() => {
                                setConfigProviderId(null);
                                setConfigEditor(config);
                              }}
                            >
                              <PencilIcon />
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('presets.delete', {
                                name: config.name,
                              })}
                              onClick={() => {
                                setDeleteConfigError(null);
                                setDeleteConfigTarget(config);
                              }}
                            >
                              <TrashIcon />
                            </button>
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'languages' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-languages-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-languages-title"
                >
                  {t('settings.languages')}
                </h3>
                <div className="field settings-language">
                  <label htmlFor="settings-language">
                    {t('settings.languageLabel')}
                  </label>
                  <select
                    id="settings-language"
                    className="text-input"
                    value={language}
                    onChange={(event) =>
                      setLanguage(event.target.value as Language)
                    }
                  >
                    {LANGUAGES.map((item) => (
                      <option key={item.code} value={item.code}>
                        {item.label}
                      </option>
                    ))}
                  </select>
                  <p className="field-hint">{t('settings.languageNote')}</p>
                </div>
              </section>
            ) : (
              <section
                className="settings-section"
                aria-labelledby="settings-about-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-about-title"
                >
                  {t('settings.about')}
                </h3>
                <div className="settings-about-card">
                  <span className="settings-about-name">elesdee</span>
                  {version ? (
                    <span className="settings-about-version">
                      {t('settings.aboutVersion', { version })}
                    </span>
                  ) : null}
                </div>
              </section>
            )}
          </div>
        </div>
      </Dialog>

      {addProviderOpen ? (
        <ProviderDialog
          options={options}
          onSubmit={onCreateProvider}
          onSaved={(provider) => {
            onProviderSaved(provider);
            setAddProviderOpen(false);
          }}
          onClose={() => setAddProviderOpen(false)}
        />
      ) : null}

      {editProvider ? (
        <ProviderDialog
          provider={editProvider}
          options={options}
          onSubmit={(input) =>
            onUpdateProvider(editProvider.id, input as UpdateProviderInput)
          }
          onSaved={(provider) => {
            onProviderSaved(provider);
            setEditProvider(null);
          }}
          onClose={() => setEditProvider(null)}
        />
      ) : null}

      {removeProviderTarget ? (
        <Dialog
          titleId="remove-account-title"
          descriptionId="remove-account-description"
          busyLabel={t('removeAccount.removing')}
          onClose={() => setRemoveProviderTarget(null)}
          busy={removingProvider}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="remove-account-title">
              {t('removeAccount.title')}
            </h2>
            <p className="dialog-sub" id="remove-account-description">
              {t('removeAccount.text', { name: removeProviderTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {removeProviderError ? (
              <p className="form-error" role="alert">
                {removeProviderError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setRemoveProviderTarget(null)}
              disabled={removingProvider}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmRemoveProvider()}
              disabled={removingProvider}
            >
              {removingProvider
                ? t('removeAccount.removing')
                : t('removeAccount.confirm')}
            </button>
          </div>
        </Dialog>
      ) : null}

      {configEditor !== undefined ? (
        <AiConfigDialog
          config={configEditor}
          providers={providers}
          options={options}
          initialProviderId={configProviderId}
          onSaveConfig={(id, input) =>
            id
              ? onUpdateConfig(id, input as UpdateConfigInput)
              : onCreateConfig(input as CreateConfigInput)
          }
          onCreateProvider={onCreateProvider}
          onProviderSaved={onProviderSaved}
          onSaved={(config) => {
            onConfigSaved(config);
            setConfigEditor(undefined);
          }}
          onClose={() => setConfigEditor(undefined)}
        />
      ) : null}

      {deleteConfigTarget ? (
        <Dialog
          titleId="delete-preset-title"
          descriptionId="delete-preset-description"
          busyLabel={t('presets.deleting')}
          onClose={() => setDeleteConfigTarget(null)}
          busy={deletingConfig}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="delete-preset-title">
              {t('presets.deleteTitle')}
            </h2>
            <p className="dialog-sub" id="delete-preset-description">
              {t('presets.deleteText', { name: deleteConfigTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {deleteConfigError ? (
              <p className="form-error" role="alert">
                {deleteConfigError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setDeleteConfigTarget(null)}
              disabled={deletingConfig}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmDeleteConfig()}
              disabled={deletingConfig}
            >
              {deletingConfig
                ? t('presets.deleting')
                : t('presets.deleteConfirm')}
            </button>
          </div>
        </Dialog>
      ) : null}
    </>
  );
}
