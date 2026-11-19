import { useState } from 'react';
import type { Account, CreateAccountInput } from '../api/types';
import { accountUsage } from '../state/usage';
import { AddAccountDialog } from './AddAccountDialog';
import { Dialog } from './Dialog';
import { CloseIcon, LayersIcon, PlusIcon, SparkIcon } from './icons';

type SettingsSection = 'providers' | 'about';

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
              Settings
            </h2>
            <p className="settings-page-sub" id="settings-description">
              This computer
            </p>
          </div>
          <button
            type="button"
            className="btn btn-ghost settings-close"
            aria-label="Close settings"
            onClick={onClose}
            autoFocus
          >
            <CloseIcon />
          </button>
        </div>

        <div className="settings-page-body">
          <nav className="settings-nav" aria-label="Settings sections">
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
              <span>Providers</span>
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
              <span>About</span>
            </button>
            {version ? (
              <span className="settings-nav-version">elesdee {version}</span>
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
                      Providers
                    </h3>
                    <p className="settings-section-note">
                      Accounts on this computer and their usage.
                    </p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => setAddOpen(true)}
                  >
                    <PlusIcon />
                    Add account
                  </button>
                </div>

                {accounts.length === 0 ? (
                  <p className="settings-empty">No accounts yet.</p>
                ) : (
                  <ul className="settings-accounts">
                    {accounts.map((account) => {
                      const usage = accountUsage(account);
                      return (
                        <li key={account.id} className="settings-account">
                          <div className="settings-account-main">
                            <div className="settings-account-title">
                              <span className="settings-account-name">
                                {account.name}
                              </span>
                              {account.sample ? (
                                <span className="sample-tag">Sample</span>
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
            ) : (
              <section
                className="settings-section"
                aria-labelledby="settings-about-title"
              >
                <h3
                  className="settings-section-title"
                  id="settings-about-title"
                >
                  About
                </h3>
                <div className="settings-about-card">
                  <span className="settings-about-name">elesdee</span>
                  {version ? (
                    <span className="settings-about-version">
                      Version {version}
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
