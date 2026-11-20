import { useCallback } from 'react';
import { listAccounts } from '../api/client';
import type { Account } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountOption } from './accountOption';
import { Combobox, type ComboboxLoader } from './Combobox';
import { CloseIcon } from './icons';

interface AccountMultiSelectProps {
  id: string;
  label: string;
  accounts: Account[];
  value: string[];
  requiredId?: string | null;
  onChange: (ids: string[]) => void;
  hint?: string;
  error?: string;
  disabled?: boolean;
}

export function AccountMultiSelect({
  id,
  label,
  accounts,
  value,
  requiredId,
  onChange,
  hint,
  error,
  disabled = false,
}: AccountMultiSelectProps) {
  const { t, language } = useLanguage();

  const loadOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listAccounts(query, signal);
      return list
        .filter((account) => !value.includes(account.id))
        .map((account) => accountOption(account, language));
    },
    [value, language],
  );

  function remove(accountId: string) {
    onChange(value.filter((existing) => existing !== accountId));
  }

  return (
    <fieldset className="field" id={id} disabled={disabled}>
      <legend>{label}</legend>
      {value.length > 0 ? (
        <ul className="allowed-account-list" aria-label={label}>
          {value.map((accountId) => {
            const account =
              accounts.find((candidate) => candidate.id === accountId) ?? null;
            const name = account?.name ?? accountId;
            const required = accountId === requiredId;
            return (
              <li
                key={accountId}
                className={required ? 'chip is-on' : 'chip'}
              >
                <span>{name}</span>
                {required ? (
                  <span className="chip-count">
                    {t('allowedAccounts.current')}
                  </span>
                ) : (
                  <button
                    type="button"
                    className="allowed-chip-close"
                    aria-label={t('allowedAccounts.remove', { name })}
                    disabled={disabled}
                    onClick={() => remove(accountId)}
                  >
                    <CloseIcon />
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      ) : null}
      <Combobox
        key={`${id}-${value.length}`}
        id={`${id}-add`}
        label={t('allowedAccounts.add')}
        value={null}
        loadOptions={loadOptions}
        onChange={(accountId) => {
          if (accountId && !value.includes(accountId)) {
            onChange([...value, accountId]);
          }
        }}
        placeholder={t('allowedAccounts.placeholder')}
        hint={hint}
        error={error}
        disabled={disabled}
      />
    </fieldset>
  );
}
