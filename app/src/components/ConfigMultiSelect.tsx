import { useCallback, useEffect, useRef } from 'react';
import { listConfigs } from '../api/client';
import type { AiConfig, Provider } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { Combobox, type ComboboxLoader } from './Combobox';
import { CloseIcon } from './icons';

interface ConfigMultiSelectProps {
  id: string;
  label: string;
  configs: AiConfig[];
  providers: Provider[];
  value: string[];
  requiredId?: string | null;
  onChange: (ids: string[]) => void;
  hint?: string;
  error?: string;
  disabled?: boolean;
}

export function ConfigMultiSelect({
  id,
  label,
  configs,
  providers,
  value,
  requiredId,
  onChange,
  hint,
  error,
  disabled = false,
}: ConfigMultiSelectProps) {
  const { t } = useLanguage();
  const listRef = useRef<HTMLUListElement>(null);

  // The capped list scrolls. Keep the current config visible when the list
  // grows or the current config changes.
  useEffect(() => {
    const list = listRef.current;
    const current = list?.querySelector<HTMLElement>('[data-current="true"]');
    if (!list || !current) {
      return;
    }
    const listRect = list.getBoundingClientRect();
    const chipRect = current.getBoundingClientRect();
    if (chipRect.top < listRect.top) {
      list.scrollTop -= listRect.top - chipRect.top;
    } else if (chipRect.bottom > listRect.bottom) {
      list.scrollTop += chipRect.bottom - listRect.bottom;
    }
  }, [value, requiredId]);

  const loadOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listConfigs(query, signal);
      return list
        .filter((config) => !value.includes(config.id))
        .map((config) => {
          const provider = providers.find(
            (item) => item.id === config.providerId,
          );
          return {
            id: config.id,
            label: config.name,
            meta: [config.client, provider?.name].filter(Boolean).join(' · '),
          };
        });
    },
    [providers, value],
  );

  function remove(configId: string) {
    onChange(value.filter((existing) => existing !== configId));
  }

  return (
    <fieldset className="field" id={id} disabled={disabled}>
      <legend>{label}</legend>
      {value.length > 0 ? (
        <ul ref={listRef} className="allowed-account-list" aria-label={label}>
          {value.map((configId) => {
            const config =
              configs.find((candidate) => candidate.id === configId) ?? null;
            const name = config?.name ?? configId;
            const required = configId === requiredId;
            return (
              <li
                key={configId}
                className={required ? 'chip is-on' : 'chip'}
                data-current={required ? 'true' : undefined}
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
                    onClick={() => remove(configId)}
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
        onChange={(configId) => {
          if (configId && !value.includes(configId)) {
            onChange([...value, configId]);
          }
        }}
        placeholder={t('allowedConfigs.placeholder')}
        hint={hint}
        error={error}
        disabled={disabled}
      />
    </fieldset>
  );
}
