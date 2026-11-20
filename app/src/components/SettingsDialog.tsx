import { useState } from 'react';
import type { Account, CreateAccountInput } from '../api/types';
import { LANGUAGES, type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { AddAccountDialog } from './AddAccountDialog';
import { Dialog } from './Dialog';
import {
  CloseIcon,
  GlobeIcon,
  LayersIcon,
  PlusIcon,
  SparkIcon,
} from './icons';

type SettingsSection = 'providers' | 'languages' | 'about';

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

  return (
    <>
      <Dialog
        titleId="settings-title"
        descriptionId="settings-description"
        className="settings-dialog"
        onClose={onClose}
        inert={addOpen}
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
          }}
          onClose={() => setAddOpen(false)}
        />
      ) : null}
    </>
  );
}
