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
import { AccountMultiSelect } from './AccountMultiSelect';
import { accountOption } from './accountOption';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';

interface SwitchAccountDialogProps {
  session: Session;
  onSwitch: (accountId: string) => Promise<void>;
  onAllowedChange: (accountIds: string[]) => Promise<void>;
  onClose: () => void;
}

export function SwitchAccountDialog({
  session,
  onSwitch,
  onAllowedChange,
  onClose,
}: SwitchAccountDialogProps) {
  const { t, language } = useLanguage();
  const [options, setOptions] = useState<SwitchOptions | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [savingAllowed, setSavingAllowed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [allowedError, setAllowedError] = useState<string | null>(null);

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

  const allowed = options?.allowedAccountIds ?? [];
  const allowedSet = new Set(allowed);
  const switchable = (options?.accounts ?? []).filter(
    (account) =>
      allowedSet.has(account.id) && account.id !== session.accountId,
  );

  useEffect(() => {
    if (
      accountId &&
      options &&
      !options.allowedAccountIds.includes(accountId)
    ) {
      setAccountId(null);
    }
  }, [accountId, options]);

  const loadAccounts = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!options) {
        return [];
      }
      const allowedIds = new Set(options.allowedAccountIds);
      const list = await listAccounts(query, signal);
      return list
        .filter(
          (account) =>
            allowedIds.has(account.id) && account.id !== session.accountId,
        )
        .map((account) => ({
          ...accountOption(account, language),
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

  async function changeAllowed(ids: string[]) {
    if (!options || savingAllowed) {
      return;
    }
    const previous = options.allowedAccountIds;
    const nextAllowed = ids.slice();
    setOptions({
      ...options,
      allowedAccountIds: nextAllowed,
      nextAccountId:
        options.nextAccountId && nextAllowed.includes(options.nextAccountId)
          ? options.nextAccountId
          : null,
    });
    setAllowedError(null);
    setSavingAllowed(true);
    try {
      await onAllowedChange(nextAllowed);
    } catch (failure) {
      setOptions((current) =>
        current ? { ...current, allowedAccountIds: previous } : current,
      );
      setAllowedError(
        failure instanceof ApiError
          ? failure.message
          : t('allowedAccounts.saveFailed'),
      );
    } finally {
      setSavingAllowed(false);
    }
  }

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
                switchable.length === 0
                  ? t('switchAccount.noOther')
                  : t('switchAccount.placeholder')
              }
              hint={hint}
              error={error ?? undefined}
              required
              autoFocus
              disabled={savingAllowed}
            />

            <AccountMultiSelect
              id="switch-allowed"
              label={t('allowedAccounts.label')}
              accounts={options.accounts}
              value={options.allowedAccountIds}
              requiredId={session.accountId}
              onChange={(ids) => void changeAllowed(ids)}
              hint={t('allowedAccounts.hint')}
              error={allowedError ?? undefined}
              disabled={submitting || savingAllowed}
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
                submitting ||
                savingAllowed ||
                !accountId ||
                switchable.length === 0
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
