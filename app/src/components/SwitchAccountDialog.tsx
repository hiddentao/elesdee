import {
  useCallback,
  useEffect,
  useState,
  type FormEvent,
} from 'react';
import {
  ApiError,
  getSwitchOptions,
  listAccounts,
} from '../api/client';
import type { Account, Session, SwitchOptions } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';

interface SwitchAccountDialogProps {
  session: Session;
  onSwitch: (accountId: string) => Promise<void>;
  onClose: () => void;
}

export function SwitchAccountDialog({
  session,
  onSwitch,
  onClose,
}: SwitchAccountDialogProps) {
  const { t, language } = useLanguage();
  const [options, setOptions] = useState<SwitchOptions | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setOptions(null);
    setLoadFailed(false);
    getSwitchOptions(session.id)
      .then((next) => {
        if (!cancelled) {
          setOptions(next);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setLoadFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [session.id, attempt]);

  const loadAccounts = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!options) {
        return [];
      }
      const allowed = new Set(options.accounts.map((account) => account.id));
      const list = await listAccounts(query, signal);
      return list
        .filter(
          (account) =>
            allowed.has(account.id) && account.id !== session.accountId,
        )
        .map((account) => ({
          id: account.id,
          label: account.name,
          meta: `${account.client} · ${accountUsage(account, language).summary}`,
          tag:
            account.id === options.nextAccountId
              ? t('switchAccount.nextAuto')
              : account.sample
                ? t('common.sample')
                : undefined,
        }));
    },
    [options, session.accountId, language, t],
  );

  const selectedAccount: Account | null = accountId
    ? (options?.accounts.find((account) => account.id === accountId) ?? null)
    : null;
  const hint = selectedAccount
    ? selectedAccount.client === session.client
      ? t('switchAccount.handoffSame', { client: selectedAccount.client })
      : t('switchAccount.handoffOther', { client: selectedAccount.client })
    : t('switchAccount.hint');

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

      {options === null && !loadFailed ? (
        <div className="dialog-body">
          <p className="dialog-sub" role="status">
            {t('switchAccount.loading')}
          </p>
        </div>
      ) : null}

      {loadFailed ? (
        <div className="dialog-body">
          <p className="form-error" role="alert">
            {t('switchAccount.loadFailed')}
          </p>
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => setAttempt((current) => current + 1)}
          >
            {t('common.retry')}
          </button>
        </div>
      ) : null}

      {options ? (
        <form className="dialog-form" onSubmit={handleSubmit} noValidate>
          <div className="dialog-body">
            <Combobox
              id="switch-account"
              label={t('switchAccount.account')}
              value={accountId}
              loadOptions={loadAccounts}
              onChange={(id) => {
                setAccountId(id);
                setError(null);
              }}
              placeholder={
                options.accounts.length <= 1
                  ? t('switchAccount.noOther')
                  : t('switchAccount.placeholder')
              }
              hint={hint}
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
              disabled={
                submitting || !accountId || options.accounts.length <= 1
              }
            >
              {submitting
                ? t('switchAccount.submitting')
                : t('switchAccount.submit')}
            </button>
          </div>
        </form>
      ) : null}
    </Dialog>
  );
}
