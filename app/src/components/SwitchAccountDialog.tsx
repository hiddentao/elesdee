import { useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type { Account, Session } from '../api/types';
import { type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { Combobox, type ComboboxOption } from './Combobox';
import { Dialog } from './Dialog';

interface SwitchAccountDialogProps {
  session: Session;
  accounts: Account[];
  onSwitch: (accountId: string) => Promise<void>;
  onClose: () => void;
}

function accountOption(
  account: Account,
  language: Language,
): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${account.client} · ${accountUsage(account, language).summary}`,
  };
}

export function SwitchAccountDialog({
  session,
  accounts,
  onSwitch,
  onClose,
}: SwitchAccountDialogProps) {
  const { t, language } = useLanguage();
  const options = accounts
    .filter((account) => account.id !== session.accountId)
    .map((account) => accountOption(account, language));
  const [accountId, setAccountId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!accountId) {
      setError(t('switchAccount.errorChoose'));
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      await onSwitch(accountId);
      onClose();
    } catch (failure) {
      setSubmitting(false);
      setError(
        failure instanceof ApiError
          ? failure.message
          : t('switchAccount.errorSwitch'),
      );
    }
  }

  return (
    <Dialog
      titleId="switch-account-title"
      descriptionId="switch-account-description"
      busyLabel={t('switchAccount.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="switch-account-title">
          {t('switchAccount.title')}
        </h2>
        <p className="dialog-sub" id="switch-account-description">
          {session.name}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="switch-account"
            label={t('switchAccount.account')}
            value={accountId}
            options={options}
            onChange={(id) => {
              setAccountId(id);
              setError(null);
            }}
            placeholder={
              options.length === 0
                ? t('switchAccount.noOther')
                : t('switchAccount.placeholder')
            }
            hint={t('switchAccount.hint')}
            error={error ?? undefined}
            required
            autoFocus
          />
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
          <button
            type="submit"
            className="btn btn-primary"
            disabled={submitting || options.length === 0}
          >
            {submitting
              ? t('switchAccount.submitting')
              : t('switchAccount.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
