import { useCallback, useEffect, useState } from 'react';
import {
  ApiError,
  createPreset,
  deletePreset,
  listPresets,
  updatePreset,
} from '../api/client';
import type {
  Account,
  CreateAccountInput,
  CreatePresetInput,
  LaunchPreset,
} from '../api/types';
import { LANGUAGES, type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { AddAccountDialog } from './AddAccountDialog';
import { Dialog } from './Dialog';
import { PresetDialog } from './PresetDialog';
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

type SettingsSection = 'providers' | 'presets' | 'languages' | 'about';

interface SettingsDialogProps {
  accounts: Account[];
  version: string | null;
  onCreateAccount: (input: CreateAccountInput) => Promise<Account>;
  onAccountCreated: (account: Account) => void;
  onClose: () => void;
}

export function SettingsDialog({
  accounts,
  version,
  onCreateAccount,
  onAccountCreated,
  onClose,
}: SettingsDialogProps) {
  const { t, language, setLanguage } = useLanguage();
  const [section, setSection] = useState<SettingsSection>('providers');
  const [addOpen, setAddOpen] = useState(false);
  const [presets, setPresets] = useState<LaunchPreset[]>([]);
  const [presetsLoading, setPresetsLoading] = useState(true);
  const [presetsError, setPresetsError] = useState<string | null>(null);
  const [presetEditor, setPresetEditor] = useState<
    LaunchPreset | null | undefined
  >(undefined);
  const [deleteTarget, setDeleteTarget] = useState<LaunchPreset | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  const refreshPresets = useCallback(async () => {
    setPresetsLoading(true);
    try {
      setPresets(await listPresets(''));
      setPresetsError(null);
    } catch {
      setPresetsError(t('presets.errorLoad'));
    } finally {
      setPresetsLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void refreshPresets();
  }, [refreshPresets]);

  async function savePreset(id: string | null, input: CreatePresetInput) {
    return id ? updatePreset(id, input) : createPreset(input);
  }

  async function confirmDelete() {
    if (!deleteTarget) {
      return;
    }
    setDeleting(true);
    setDeleteError(null);
    try {
      await deletePreset(deleteTarget.id);
      setDeleteTarget(null);
      await refreshPresets();
    } catch (error) {
      setDeleteError(
        error instanceof ApiError
          ? error.message
          : t('presets.errorDelete'),
      );
    } finally {
      setDeleting(false);
    }
  }

  return (
    <>
      <Dialog
        titleId="settings-title"
        descriptionId="settings-description"
        className="settings-dialog"
        onClose={onClose}
        inert={
          addOpen || presetEditor !== undefined || deleteTarget !== null
        }
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
          <nav
            className="settings-nav"
            aria-label={t('settings.sections')}
          >
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
                section === 'presets'
                  ? 'settings-nav-item is-on'
                  : 'settings-nav-item'
              }
              aria-current={section === 'presets' ? 'page' : undefined}
              onClick={() => setSection('presets')}
            >
              <PresetIcon />
              <span>{t('settings.presets')}</span>
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
                    onClick={() => setAddOpen(true)}
                  >
                    <PlusIcon />
                    {t('settings.addAccount')}
                  </button>
                </div>

                {accounts.length === 0 ? (
                  <p className="settings-empty">{t('settings.noAccounts')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {accounts.map((account) => {
                      const usage = accountUsage(account, language);
                      return (
                        <li key={account.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {account.name}
                              </span>
                              {account.sample ? (
                                <span className="sample-tag">
                                  {t('common.sample')}
                                </span>
                              ) : null}
                            </div>
                            <span className="settings-account-sub">
                              {account.client} · {account.plan}
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
                        </li>
                      );
                    })}
                  </ul>
                )}
              </section>
            ) : section === 'presets' ? (
              <section
                className="settings-section"
                aria-labelledby="settings-presets-title"
              >
                <div className="settings-section-head">
                  <div>
                    <h3
                      className="settings-section-title"
                      id="settings-presets-title"
                    >
                      {t('settings.presets')}
                    </h3>
                    <p className="settings-section-note">
                      {t('settings.presetsNote')}
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => setPresetEditor(null)}
                  >
                    <PlusIcon />
                    {t('settings.addPreset')}
                  </button>
                </div>

                {presetsError ? (
                  <p className="form-error" role="alert">
                    {presetsError}
                  </p>
                ) : presetsLoading && presets.length === 0 ? (
                  <p className="settings-empty">{t('presets.loading')}</p>
                ) : presets.length === 0 ? (
                  <p className="settings-empty">{t('settings.noPresets')}</p>
                ) : (
                  <ul className="settings-accounts">
                    {presets.map((preset) => {
                      const account =
                        accounts.find(
                          (candidate) => candidate.id === preset.accountId,
                        ) ?? null;
                      const configCount = Object.keys(preset.config).length;
                      return (
                        <li key={preset.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {preset.name}
                              </span>
                              {!preset.stored ? (
                                <span className="sample-tag">
                                  {t('presets.fromAccount')}
                                </span>
                              ) : null}
                            </div>
                            <span className="settings-account-sub">
                              {account
                                ? `${account.name} · ${account.client}`
                                : preset.accountId}
                              {preset.model
                                ? ` · ${t('newSession.modelWith', {
                                    model: preset.model,
                                  })}`
                                : ''}
                            </span>
                            <span className="settings-account-sub">
                              {preset.env.length > 0
                                ? t(
                                    preset.env.length === 1
                                      ? 'newSession.envOverrideOne'
                                      : 'newSession.envOverrideMany',
                                    { count: preset.env.length },
                                  )
                                : ''}
                              {preset.env.length > 0 && configCount > 0
                                ? ' · '
                                : ''}
                              {configCount > 0
                                ? t(
                                    configCount === 1
                                      ? 'newSession.configOverrideOne'
                                      : 'newSession.configOverrideMany',
                                    { count: configCount },
                                  )
                                : ''}
                              {preset.env.length === 0 && configCount === 0
                                ? t('newSession.overridesNone')
                                : ''}
                            </span>
                          </div>
                          <div className="settings-account-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              aria-label={t('presets.edit', {
                                name: preset.name,
                              })}
                              onClick={() => setPresetEditor(preset)}
                            >
                              <PencilIcon />
                            </button>
                            {preset.stored ? (
                              <button
                                type="button"
                                className="btn btn-ghost"
                                aria-label={t('presets.delete', {
                                  name: preset.name,
                                })}
                                onClick={() => {
                                  setDeleteError(null);
                                  setDeleteTarget(preset);
                                }}
                              >
                                <TrashIcon />
                              </button>
                            ) : null}
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

      {addOpen ? (
        <AddAccountDialog
          onCreate={onCreateAccount}
          onCreated={(account) => {
            onAccountCreated(account);
            setAddOpen(false);
            void refreshPresets();
          }}
          onClose={() => setAddOpen(false)}
        />
      ) : null}

      {presetEditor !== undefined ? (
        <PresetDialog
          accounts={accounts}
          preset={presetEditor}
          onSubmit={savePreset}
          onSaved={() => {
            setPresetEditor(undefined);
            void refreshPresets();
          }}
          onClose={() => setPresetEditor(undefined)}
        />
      ) : null}

      {deleteTarget ? (
        <Dialog
          titleId="delete-preset-title"
          descriptionId="delete-preset-description"
          busyLabel={t('presets.deleting')}
          onClose={() => setDeleteTarget(null)}
          busy={deleting}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="delete-preset-title">
              {t('presets.deleteTitle')}
            </h2>
            <p className="dialog-sub" id="delete-preset-description">
              {t('presets.deleteText', { name: deleteTarget.name })}
            </p>
          </div>
          <div className="dialog-body">
            {deleteError ? (
              <p className="form-error" role="alert">
                {deleteError}
              </p>
            ) : null}
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setDeleteTarget(null)}
              disabled={deleting}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmDelete()}
              disabled={deleting}
            >
              {deleting
                ? t('presets.deleting')
                : t('presets.deleteConfirm')}
            </button>
          </div>
        </Dialog>
      ) : null}
    </>
  );
}
