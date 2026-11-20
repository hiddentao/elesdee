import type { Account } from '../api/types';
import type { Language } from '../i18n';
import { accountUsage } from '../state/usage';
import type { ComboboxOption } from './Combobox';

export function accountOption(
  account: Account,
  language: Language,
): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${account.client} · ${accountUsage(account, language).summary}`,
  };
}
