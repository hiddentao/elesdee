import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError, listProviderModels, listProviders } from '../api/client';
import type {
  AgentCheck,
  AiConfig,
  Client,
  ClientInstall,
  CreateConfigInput,
  CreateProviderInput,
  InstallStage,
  Provider,
  ProviderOption,
  ProviderSignIn,
  StartProviderSignInInput,
  UpdateConfigInput,
} from '../api/types';
import type { MessageKey } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { agentFor, shortVersion } from '../state/clients';
import { providerUsage } from '../state/usage';
import { Combobox, type ComboboxLoader } from './Combobox';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';
import { ProviderDialog } from './ProviderDialog';

interface AiConfigDialogProps {
  config?: AiConfig | null;
  providers: Provider[];
  options: ProviderOption[];
  agents: AgentCheck[];
  agentsChecked: boolean;
  onInstallClient: (client: Client) => Promise<ClientInstall>;
  onGetClientInstall: (id: string) => Promise<ClientInstall>;
  onClientInstalled: (agent: AgentCheck) => void;
  initialProviderId?: string | null;
  onSaveConfig: (
    id: string | null,
    input: CreateConfigInput | UpdateConfigInput,
  ) => Promise<AiConfig>;
  onCreateProvider: (input: CreateProviderInput) => Promise<Provider>;
  onStartProviderSignIn: (
    input: StartProviderSignInInput,
  ) => Promise<ProviderSignIn>;
  onGetProviderSignIn: (id: string) => Promise<ProviderSignIn>;
  onSubmitProviderSignInCode: (
    id: string,
    code: string,
  ) => Promise<ProviderSignIn>;
  onCancelProviderSignIn: (id: string) => Promise<void>;
  onProviderSaved: (provider: Provider) => void;
  onSaved: (config: AiConfig) => void;
  onClose: () => void;
}

interface FormErrors {
  provider?: string;
  client?: string;
  model?: string;
  env?: string;
  config?: string;
  form?: string;
}

const INSTALL_STAGE_KEYS: Record<InstallStage, MessageKey> = {
  preparing: 'configs.installStagePreparing',
  downloading: 'configs.installStageDownloading',
  installing: 'configs.installStageInstalling',
  verifying: 'configs.installStageVerifying',
  finishing: 'configs.installStageFinishing',
};

function parseEnv(text: string): Array<{ name: string; value: string }> | null {
  const env = [];
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (!line) {
      continue;
    }
    const separator = line.indexOf('=');
    if (separator <= 0) {
      return null;
    }
    env.push({
      name: line.slice(0, separator).trim(),
      value: line.slice(separator + 1),
    });
  }
  return env;
}

function parseConfig(text: string): Record<string, string> | null {
  const config: Record<string, string> = {};
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (!line) {
      continue;
    }
    const separator = line.indexOf('=');
    if (separator <= 0) {
      return null;
    }
    config[line.slice(0, separator).trim()] = line.slice(separator + 1);
  }
  return config;
}

function envText(env: Array<{ name: string; value: string }>): string {
  return env.map((item) => `${item.name}=${item.value}`).join('\n');
}

function configText(config: Record<string, string>): string {
  return Object.entries(config)
    .map(([key, value]) => `${key}=${value}`)
    .join('\n');
}

export function AiConfigDialog({
  config = null,
  providers,
  options,
  agents,
  agentsChecked,
  onInstallClient,
  onGetClientInstall,
  onClientInstalled,
  initialProviderId = null,
  onSaveConfig,
  onCreateProvider,
  onStartProviderSignIn,
  onGetProviderSignIn,
  onSubmitProviderSignInCode,
  onCancelProviderSignIn,
  onProviderSaved,
  onSaved,
  onClose,
}: AiConfigDialogProps) {
  const { t, language } = useLanguage();
  const editing = config !== null;
  const initialProvider =
    providers.find(
      (provider) =>
        provider.id === (config?.providerId ?? initialProviderId ?? ''),
    ) ??
    providers[0] ??
    null;
  const [providerId, setProviderId] = useState<string | null>(
    initialProvider?.id ?? null,
  );
  const [client, setClient] = useState<Client | null>(
    config?.client ?? initialProvider?.supportedClients[0] ?? null,
  );
  const [name, setName] = useState(config?.name ?? '');
  const [model, setModel] = useState<string | null>(config?.model ?? null);
  const [effort, setEffort] = useState(config?.effort ?? '');
  const [argsText, setArgsText] = useState((config?.args ?? []).join('\n'));
  const [env, setEnv] = useState(envText(config?.env ?? []));
  const [agentConfig, setAgentConfig] = useState(
    configText(config?.config ?? {}),
  );
  const [fallback, setFallback] = useState(false);
  const [providerOpen, setProviderOpen] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [installs, setInstalls] = useState<Record<string, ClientInstall>>({});
  const [installTarget, setInstallTarget] = useState<Client | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  const provider = providers.find((item) => item.id === providerId) ?? null;
  const option = options.find((item) => item.id === provider?.vendor) ?? null;
  const clients = provider?.supportedClients ?? [];
  const efforts = client ? (option?.efforts[client] ?? []) : [];
  const defaultName =
    provider && client ? `${provider.name} (${client})` : '';

  useEffect(() => {
    if (!client || clients.includes(client)) {
      return;
    }
    setClient(clients[0] ?? null);
  }, [client, clients]);

  useEffect(() => {
    const running = Object.values(installs).filter(
      (job) => job.state === 'running',
    );
    if (running.length === 0) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      for (const job of running) {
        onGetClientInstall(job.id)
          .then((next) => {
            if (cancelled) {
              return;
            }
            setInstalls((current) => ({ ...current, [next.client]: next }));
            if (next.state === 'done') {
              onClientInstalled({
                name: next.client,
                installed: true,
                version: next.version,
                path: next.path,
              });
            }
          })
          .catch(() => {
            if (cancelled) {
              return;
            }
            setInstalls((current) => {
              const existing = current[job.client];
              if (
                !existing ||
                existing.id !== job.id ||
                existing.state !== 'running'
              ) {
                return current;
              }
              return {
                ...current,
                [job.client]: {
                  ...existing,
                  state: 'failed',
                  error: t('configs.installReasonInternal'),
                },
              };
            });
          });
      }
    }, 400);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [installs, onClientInstalled, onGetClientInstall, t]);

  const loadProviderOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listProviders(query, signal);
      return list.map((item) => ({
        id: item.id,
        label: item.name,
        meta: [
          item.supportedClients.join(', '),
          providerUsage(item, language).summary,
        ]
          .filter(Boolean)
          .join(' · '),
        tag: item.sample ? t('common.sample') : undefined,
      }));
    },
    [language, t],
  );

  const loadModelOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!provider || !client) {
        return [];
      }
      const list = await listProviderModels(provider.id, client, signal);
      setFallback(list.fallback);
      const filter = query.trim().toLowerCase();
      return list.models
        .filter(
          (item) =>
            filter === '' ||
            item.id.toLowerCase().includes(filter) ||
            item.label.toLowerCase().includes(filter),
        )
        .map((item) => ({ id: item.id, label: item.label, meta: item.id }));
    },
    [client, provider],
  );

  function chooseProvider(id: string | null) {
    const next = providers.find((item) => item.id === id) ?? null;
    setProviderId(id);
    setModel(null);
    setFallback(false);
    setClient(next?.supportedClients[0] ?? null);
    setErrors((current) => ({ ...current, provider: undefined }));
  }

  async function confirmInstall() {
    if (!installTarget) {
      return;
    }
    const target = installTarget;
    setInstallTarget(null);
    setInstallError(null);
    try {
      const job = await onInstallClient(target);
      setInstalls((current) => ({ ...current, [job.client]: job }));
    } catch (error) {
      setInstallError(
        error instanceof ApiError
          ? error.message
          : t('configs.installReasonInternal'),
      );
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextErrors: FormErrors = {};
    if (!provider) {
      nextErrors.provider = t('configs.errorProvider');
    }
    if (!client) {
      nextErrors.client = t('configs.errorClient');
    }
    const parsedEnv = parseEnv(env);
    if (parsedEnv === null) {
      nextErrors.env = t('newSession.errorEnv');
    }
    const parsedConfig = parseConfig(agentConfig);
    if (parsedConfig === null) {
      nextErrors.config = t('configs.errorConfig');
    }
    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      return;
    }

    const input: CreateConfigInput = {
      providerId: provider!.id,
      client: client!,
      model,
      effort: effort === '' ? null : effort,
      args: argsText
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => line !== ''),
      env: parsedEnv ?? [],
      config: parsedConfig ?? {},
    };
    if (name.trim() !== '') {
      input.name = name.trim();
    }

    setErrors({});
    setSubmitting(true);
    try {
      const saved = await onSaveConfig(config?.id ?? null, input);
      onSaved(saved);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t(editing ? 'configs.errorSave' : 'configs.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  return (
    <>
      <Dialog
        titleId="config-dialog-title"
        descriptionId="config-dialog-description"
        busyLabel={t(editing ? 'configs.saving' : 'configs.adding')}
        onClose={onClose}
        busy={submitting}
        inert={providerOpen || installTarget !== null}
      >
        <div className="dialog-head">
          <h2 className="dialog-title" id="config-dialog-title">
            {t(editing ? 'configs.editTitle' : 'configs.addTitle')}
          </h2>
          <p className="dialog-sub" id="config-dialog-description">
            {t(editing ? 'configs.editSubtitle' : 'configs.addSubtitle')}
          </p>
        </div>

        <form className="dialog-form" onSubmit={handleSubmit} noValidate>
          <div className="dialog-body">
            <Combobox
              id="config-provider"
              label={t('configs.provider')}
              value={providerId}
              loadOptions={loadProviderOptions}
              selectedOption={
                provider
                  ? {
                      id: provider.id,
                      label: provider.name,
                      meta: provider.supportedClients.join(', '),
                    }
                  : null
              }
              onChange={chooseProvider}
              placeholder={t('configs.providerPlaceholder')}
              hint={t('configs.providerHint')}
              error={errors.provider}
              required
            />
            <button
              type="button"
              className="btn btn-ghost config-provider-new"
              onClick={() => setProviderOpen(true)}
            >
              {t('configs.newProvider')}
            </button>

            {provider ? (
              <fieldset className="field">
                <legend>{t('configs.client')}</legend>
                <div className="agent-rows">
                  {clients.map((item) => {
                    const agent = agentFor(agents, item);
                    const job = installs[item];
                    const running = job?.state === 'running';
                    const failed = job?.state === 'failed';
                    const done = job?.state === 'done';
                    const installed = agent?.installed === true || done;
                    const version = done ? job?.version : agent?.version;
                    return (
                      <div
                        className={`agent-row${client === item ? ' is-selected' : ''}`}
                        key={item}
                      >
                        <label className="agent-choice">
                          <input
                            type="radio"
                            name="config-client"
                            value={item}
                            checked={client === item}
                            disabled={running}
                            onChange={() => {
                              setClient(item);
                              setModel(null);
                              setFallback(false);
                            }}
                          />
                          <span>{item}</span>
                        </label>
                        <div className="agent-state-line">
                          {running && job ? (
                            <>
                              <span className="agent-stage" role="status">
                                {t(INSTALL_STAGE_KEYS[job.stage])}
                              </span>
                              <progress
                                className="agent-progress"
                                max={job.total}
                                value={job.step}
                                aria-label={t('configs.installProgress', {
                                  client: item,
                                })}
                              />
                            </>
                          ) : installed ? (
                            <span className="agent-state">
                              {version
                                ? t('firstRun.installedWithVersion', {
                                    version: shortVersion(version),
                                  })
                                : t('firstRun.installed')}
                            </span>
                          ) : agentsChecked && agent ? (
                            <>
                              <span className="agent-state is-missing">
                                {t('firstRun.notInstalled')}
                              </span>
                              <button
                                type="button"
                                className="btn btn-ghost agent-install-button"
                                aria-label={t('configs.installClient', {
                                  client: item,
                                })}
                                onClick={() => {
                                  setInstallError(null);
                                  setInstallTarget(item);
                                }}
                              >
                                {t('configs.install')}
                              </button>
                            </>
                          ) : null}
                        </div>
                        {failed && job ? (
                          <p className="agent-install-error" role="alert">
                            {t('configs.installFailed', { client: item })}
                            {job.error ? ` ${job.error}` : ''}
                          </p>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
                {errors.client ? (
                  <p className="field-error" role="alert">
                    {errors.client}
                  </p>
                ) : null}
                {installError ? (
                  <p className="field-error" role="alert">
                    {installError}
                  </p>
                ) : null}
              </fieldset>
            ) : null}

            <div className="field">
              <label htmlFor="config-name">{t('configs.name')}</label>
              <input
                id="config-name"
                className="text-input"
                value={name}
                placeholder={defaultName || t('configs.namePlaceholder')}
                onChange={(event) => setName(event.target.value)}
              />
              <p className="field-hint">
                {t('configs.nameHint', { name: defaultName })}
              </p>
            </div>

            <Combobox
              key={`${providerId ?? 'none'}-${client ?? 'none'}`}
              id="config-model"
              label={t('configs.model')}
              value={model}
              loadOptions={loadModelOptions}
              onChange={setModel}
              placeholder={t('configs.modelPlaceholder')}
              hint={
                fallback
                  ? t('configs.modelFallback')
                  : t('configs.modelHint')
              }
              error={errors.model}
              disabled={!provider || !client}
              allowCustom
            />

            <div className="field">
              <label htmlFor="config-effort">{t('configs.effort')}</label>
              <select
                id="config-effort"
                className="text-input"
                value={effort}
                disabled={efforts.length === 0}
                onChange={(event) => setEffort(event.target.value)}
              >
                <option value="">{t('configs.providerDefault')}</option>
                {efforts.map((level) => (
                  <option key={level} value={level}>
                    {t(`configs.effort.${level}` as never)}
                  </option>
                ))}
              </select>
              {efforts.length === 0 ? (
                <p className="field-hint">{t('configs.noEffort')}</p>
              ) : null}
            </div>

            <details className="field additional-settings">
              <summary>{t('configs.additional')}</summary>

              <div className="field">
                <label htmlFor="config-args">{t('configs.args')}</label>
                <textarea
                  id="config-args"
                  className="text-input prompt-input"
                  value={argsText}
                  placeholder={'--verbose\n--permission-mode=plan'}
                  rows={3}
                  spellCheck={false}
                  onChange={(event) => setArgsText(event.target.value)}
                />
                <p className="field-hint">{t('configs.argsHint')}</p>
              </div>

              <div className="field">
                <label htmlFor="config-env">{t('configs.env')}</label>
                <textarea
                  id="config-env"
                  className="text-input prompt-input"
                  value={env}
                  placeholder={'NAME=value'}
                  rows={3}
                  spellCheck={false}
                  aria-invalid={errors.env ? true : undefined}
                  onChange={(event) => {
                    setEnv(event.target.value);
                    setErrors((current) => ({ ...current, env: undefined }));
                  }}
                />
                {errors.env ? (
                  <p className="field-error" role="alert">
                    {errors.env}
                  </p>
                ) : (
                  <p className="field-hint">{t('configs.envHint')}</p>
                )}
              </div>

              <div className="field">
                <label htmlFor="config-agent">
                  {t('configs.agentConfig')}
                </label>
                <textarea
                  id="config-agent"
                  className="text-input prompt-input"
                  value={agentConfig}
                  placeholder={'permissions.defaultMode=acceptEdits'}
                  rows={3}
                  spellCheck={false}
                  aria-invalid={errors.config ? true : undefined}
                  onChange={(event) => {
                    setAgentConfig(event.target.value);
                    setErrors((current) => ({
                      ...current,
                      config: undefined,
                    }));
                  }}
                />
                {errors.config ? (
                  <p className="field-error" role="alert">
                    {errors.config}
                  </p>
                ) : (
                  <p className="field-hint">{t('configs.agentConfigHint')}</p>
                )}
              </div>

              {provider?.server ? (
                <div className="field">
                  <label>{t('configs.serverOverride')}</label>
                  <p className="field-hint">
                    {t('configs.serverOverrideHint', {
                      server: provider.server,
                    })}
                  </p>
                </div>
              ) : null}
            </details>

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
            <button
              type="submit"
              className="btn btn-primary"
              disabled={submitting}
            >
              {submitting
                ? t(editing ? 'configs.saving' : 'configs.adding')
                : t(editing ? 'configs.save' : 'configs.add')}
            </button>
          </div>
        </form>
      </Dialog>

      {installTarget ? (
        <Dialog
          titleId="install-client-title"
          descriptionId="install-client-description"
          onClose={() => setInstallTarget(null)}
        >
          <div className="dialog-head">
            <h2 className="dialog-title" id="install-client-title">
              {t('configs.installTitle', { client: installTarget })}
            </h2>
            <p className="dialog-sub" id="install-client-description">
              {t('configs.installConfirm', { client: installTarget })}
            </p>
          </div>
          <div className="dialog-foot">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setInstallTarget(null)}
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void confirmInstall()}
            >
              {t('configs.installStart', { client: installTarget })}
            </button>
          </div>
        </Dialog>
      ) : null}

      {providerOpen ? (
        <ProviderDialog
          options={options}
          onSubmit={onCreateProvider}
          onStartSignIn={onStartProviderSignIn}
          onGetSignIn={onGetProviderSignIn}
          onSubmitSignInCode={onSubmitProviderSignInCode}
          onCancelSignIn={onCancelProviderSignIn}
          onSaved={(saved) => {
            onProviderSaved(saved);
            setProviderId(saved.id);
            setClient(saved.supportedClients[0] ?? null);
            setModel(null);
            setProviderOpen(false);
          }}
          onClose={() => setProviderOpen(false)}
        />
      ) : null}
    </>
  );
}
