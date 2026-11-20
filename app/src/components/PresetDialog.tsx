import { useCallback, useState, type FormEvent } from 'react';
import { ApiError, listAccounts } from '../api/client';
import type {
  Account,
  CreatePresetInput,
  EnvOverride,
  LaunchPreset,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';
import { AlertIcon, TrashIcon } from './icons';

interface PresetDialogProps {
  accounts: Account[];
  preset: LaunchPreset | null;
  onSubmit: (
    id: string | null,
    input: CreatePresetInput,
  ) => Promise<LaunchPreset>;
  onSaved: (preset: LaunchPreset) => void;
  onClose: () => void;
}

interface EnvRow {
  name: string;
  value: string;
}

interface ConfigRow {
  key: string;
  value: string;
}

interface FormErrors {
  name?: string;
  account?: string;
  env?: string;
  config?: string;
  form?: string;
}

export function PresetDialog({
  accounts,
  preset,
  onSubmit,
  onSaved,
  onClose,
}: PresetDialogProps) {
  const { t, language } = useLanguage();
  const [name, setName] = useState(preset?.name ?? '');
  const [accountId, setAccountId] = useState<string | null>(
    preset?.accountId ?? null,
  );
  const [model, setModel] = useState(preset?.model ?? '');
  const [envRows, setEnvRows] = useState<EnvRow[]>(
    () => preset?.env.map((item) => ({ ...item })) ?? [],
  );
  const [configRows, setConfigRows] = useState<ConfigRow[]>(
    () =>
      Object.entries(preset?.config ?? {}).map(([key, value]) => ({
        key,
        value,
      })),
  );
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);

  const selectedAccount = accountId
    ? accounts.find((account) => account.id === accountId) ?? null
    : null;

  const loadAccountOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listAccounts(query, signal);
      return list.map((account) => ({
        id: account.id,
        label: account.name,
        meta: `${account.client} · ${accountUsage(account, language).summary}`,
        tag: account.sample ? t('common.sample') : undefined,
      }));
    },
    [language, t],
  );

  function chooseAccount(id: string | null) {
    setAccountId(id);
    const account = id
      ? accounts.find((candidate) => candidate.id === id) ?? null
      : null;
    if (account && name.trim() === '') {
      setName(account.name);
    }
    setErrors((current) => ({ ...current, account: undefined }));
  }

  function updateEnv(index: number, patch: Partial<EnvRow>) {
    setEnvRows((rows) =>
      rows.map((row, rowIndex) =>
        rowIndex === index ? { ...row, ...patch } : row,
      ),
    );
    setErrors((current) => ({ ...current, env: undefined }));
  }

  function updateConfig(index: number, patch: Partial<ConfigRow>) {
    setConfigRows((rows) =>
      rows.map((row, rowIndex) =>
        rowIndex === index ? { ...row, ...patch } : row,
      ),
    );
    setErrors((current) => ({ ...current, config: undefined }));
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextErrors: FormErrors = {};

    if (name.trim() === '') {
      nextErrors.name = t('presets.errorName');
    }
    if (!accountId) {
      nextErrors.account = t('presets.errorAccount');
    }

    const env: EnvOverride[] = [];
    const envNames = new Set<string>();
    for (const row of envRows) {
      const variable = row.name.trim();
      if (variable === '') {
        continue;
      }
      if (envNames.has(variable)) {
        nextErrors.env = t('presets.errorDuplicateEnv');
        break;
      }
      envNames.add(variable);
      env.push({ name: variable, value: row.value });
    }

    const config: Record<string, string> = {};
    const configKeys = new Set<string>();
    for (const row of configRows) {
      const key = row.key.trim();
      if (key === '') {
        continue;
      }
      if (configKeys.has(key)) {
        nextErrors.config = t('presets.errorDuplicateConfig');
        break;
      }
      configKeys.add(key);
      config[key] = row.value;
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      return;
    }

    setErrors({});
    setSubmitting(true);
    try {
      const saved = await onSubmit(preset?.stored ? preset.id : null, {
        name: name.trim(),
        accountId: accountId as string,
        model: model.trim() === '' ? null : model.trim(),
        env,
        config,
      });
      onSaved(saved);
    } catch (error) {
      setSubmitting(false);
      setErrors({
        form:
          error instanceof ApiError ? error.message : t('presets.errorSave'),
      });
    }
  }

  return (
    <Dialog
      titleId="preset-dialog-title"
      descriptionId="preset-dialog-description"
      busyLabel={t('presets.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="preset-dialog-title">
          {preset?.stored ? t('presets.titleEdit') : t('presets.titleNew')}
        </h2>
        <p className="dialog-sub" id="preset-dialog-description">
          {t('presets.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="preset-name">{t('presets.name')}</label>
            <input
              id="preset-name"
              className="text-input"
              value={name}
              autoFocus
              required
              aria-invalid={errors.name ? true : undefined}
              aria-describedby={errors.name ? 'preset-name-error' : undefined}
              onChange={(event) => {
                setName(event.target.value);
                setErrors((current) => ({ ...current, name: undefined }));
              }}
            />
            {errors.name ? (
              <p className="field-error" id="preset-name-error" role="alert">
                {errors.name}
              </p>
            ) : null}
          </div>

          <Combobox
            id="preset-account"
            label={t('newSession.account')}
            value={accountId}
            loadOptions={loadAccountOptions}
            selectedOption={
              selectedAccount
                ? {
                    id: selectedAccount.id,
                    label: selectedAccount.name,
                    meta: `${selectedAccount.client} · ${
                      accountUsage(selectedAccount, language).summary
                    }`,
                  }
                : null
            }
            onChange={chooseAccount}
            placeholder={t('newSession.accountPlaceholder')}
            error={errors.account}
            required
          />

          <div className="field">
            <label htmlFor="preset-model">{t('presets.modelOptional')}</label>
            <input
              id="preset-model"
              className="text-input"
              value={model}
              placeholder={t('presets.modelPlaceholder')}
              spellCheck={false}
              onChange={(event) => setModel(event.target.value)}
            />
            <p className="field-hint">{t('presets.modelHint')}</p>
          </div>

          <fieldset className="field override-group">
            <legend>{t('presets.env')}</legend>
            {envRows.length === 0 ? (
              <p className="field-hint">{t('presets.envNone')}</p>
            ) : (
              <div className="override-list">
                {envRows.map((row, index) => (
                  <div className="override-row" key={index}>
                    <input
                      className="text-input"
                      aria-label={`${t('presets.variableName')} ${index + 1}`}
                      placeholder="NAME"
                      spellCheck={false}
                      value={row.name}
                      onChange={(event) =>
                        updateEnv(index, { name: event.target.value })
                      }
                    />
                    <input
                      className="text-input"
                      aria-label={`${t('presets.variableValue')} ${index + 1}`}
                      placeholder={t('presets.variableValue')}
                      value={row.value}
                      onChange={(event) =>
                        updateEnv(index, { value: event.target.value })
                      }
                    />
                    <button
                      type="button"
                      className="btn btn-ghost override-remove"
                      aria-label={`${t('presets.removeRow')} ${index + 1}`}
                      onClick={() =>
                        setEnvRows((rows) =>
                          rows.filter((_, rowIndex) => rowIndex !== index),
                        )
                      }
                    >
                      <TrashIcon />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {errors.env ? (
              <p className="field-error" role="alert">
                {errors.env}
              </p>
            ) : (
              <p className="field-hint">{t('presets.envHint')}</p>
            )}
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() =>
                setEnvRows((rows) => [...rows, { name: '', value: '' }])
              }
            >
              {t('presets.addEnv')}
            </button>
          </fieldset>

          <fieldset className="field override-group">
            <legend>{t('presets.config')}</legend>
            {configRows.length === 0 ? (
              <p className="field-hint">{t('presets.configNone')}</p>
            ) : (
              <div className="override-list">
                {configRows.map((row, index) => (
                  <div className="override-row" key={index}>
                    <input
                      className="text-input"
                      aria-label={`${t('presets.configKey')} ${index + 1}`}
                      placeholder={t('presets.configKey')}
                      spellCheck={false}
                      value={row.key}
                      onChange={(event) =>
                        updateConfig(index, { key: event.target.value })
                      }
                    />
                    <input
                      className="text-input"
                      aria-label={`${t('presets.configValue')} ${index + 1}`}
                      placeholder={t('presets.configValue')}
                      spellCheck={false}
                      value={row.value}
                      onChange={(event) =>
                        updateConfig(index, { value: event.target.value })
                      }
                    />
                    <button
                      type="button"
                      className="btn btn-ghost override-remove"
                      aria-label={`${t('presets.removeRow')} ${index + 1}`}
                      onClick={() =>
                        setConfigRows((rows) =>
                          rows.filter((_, rowIndex) => rowIndex !== index),
                        )
                      }
                    >
                      <TrashIcon />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {errors.config ? (
              <p className="field-error" role="alert">
                {errors.config}
              </p>
            ) : (
              <p className="field-hint">{t('presets.configHint')}</p>
            )}
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() =>
                setConfigRows((rows) => [...rows, { key: '', value: '' }])
              }
            >
              {t('presets.addConfig')}
            </button>
          </fieldset>

          {errors.form ? (
            <div className="form-error" role="alert">
              <AlertIcon />
              <span>{errors.form}</span>
            </div>
          ) : null}
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
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? t('presets.submitting') : t('presets.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
