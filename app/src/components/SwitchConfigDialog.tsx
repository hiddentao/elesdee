import {
  useCallback,
  useEffect,
  useState,
  type FormEvent,
} from 'react';
import { ApiError, getSwitchOptions, listConfigs } from '../api/client';
import type { AiConfig, Provider, Session, SwitchOptions } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { configOption } from './configOption';
import { ConfigMultiSelect } from './ConfigMultiSelect';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';

interface SwitchConfigDialogProps {
  session: Session;
  providers: Provider[];
  onSwitch: (configId: string) => Promise<void>;
  onAllowedChange: (configIds: string[]) => Promise<void>;
  onClose: () => void;
}

export function SwitchConfigDialog({
  session,
  providers,
  onSwitch,
  onAllowedChange,
  onClose,
}: SwitchConfigDialogProps) {
  const { t, language } = useLanguage();
  const [options, setOptions] = useState<SwitchOptions | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [configId, setConfigId] = useState<string | null>(null);
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

  const allowed = options?.allowedConfigIds ?? [];
  const allowedSet = new Set(allowed);
  const switchable = (options?.configs ?? []).filter(
    (config) => allowedSet.has(config.id) && config.id !== session.configId,
  );

  useEffect(() => {
    if (
      configId &&
      options &&
      !options.allowedConfigIds.includes(configId)
    ) {
      setConfigId(null);
    }
  }, [configId, options]);

  const loadConfigs = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!options) {
        return [];
      }
      const allowedIds = new Set(options.allowedConfigIds);
      const list = await listConfigs(query, signal);
      return list
        .filter(
          (config) =>
            allowedIds.has(config.id) && config.id !== session.configId,
        )
        .map((config) => ({
          ...configOption(config, providers, language),
          tag:
            config.id === options.nextConfigId
              ? t('switchAccount.nextAuto')
              : undefined,
        }));
    },
    [options, providers, session.configId, language, t],
  );

  const selectedConfig: AiConfig | null = configId
    ? (options?.configs.find((config) => config.id === configId) ?? null)
    : null;
  const hint = selectedConfig
    ? selectedConfig.client === session.client
      ? t('switchAccount.handoffSame', { client: selectedConfig.client })
      : t('switchAccount.handoffOther', { client: selectedConfig.client })
    : t('switchAccount.hint');

  async function changeAllowed(ids: string[]) {
    if (!options || savingAllowed) {
      return;
    }
    const previous = options.allowedConfigIds;
    const nextAllowed = ids.slice();
    setOptions({
      ...options,
      allowedConfigIds: nextAllowed,
      nextConfigId:
        options.nextConfigId && nextAllowed.includes(options.nextConfigId)
          ? options.nextConfigId
          : null,
    });
    setAllowedError(null);
    setSavingAllowed(true);
    try {
      await onAllowedChange(nextAllowed);
    } catch (failure) {
      setOptions((current) =>
        current ? { ...current, allowedConfigIds: previous } : current,
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
    if (!configId) {
      setError(t('switchAccount.errorChoose'));
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      await onSwitch(configId);
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
              value={configId}
              loadOptions={loadConfigs}
              onChange={(id) => {
                setConfigId(id);
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

            <ConfigMultiSelect
              id="switch-allowed"
              label={t('allowedAccounts.label')}
              configs={options.configs}
              providers={providers}
              value={options.allowedConfigIds}
              requiredId={session.configId}
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
                !configId ||
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
