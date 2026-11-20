import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import {
  ApiError,
  getGitStatus,
  listBranches,
  listConfigs,
  listProjects,
} from '../api/client';
import { isTauri, pickFolder } from '../api/shell';
import type {
  AiConfig,
  CreateSessionInput,
  Project,
  Provider,
  Session,
  SessionKind,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { providerUsage } from '../state/usage';
import { ConfigMultiSelect } from './ConfigMultiSelect';
import { configOption } from './configOption';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';
import {
  Combobox,
  type ComboboxLoader,
  type ComboboxOption,
} from './Combobox';

interface NewSessionDialogProps {
  projects: Project[];
  providers: Provider[];
  configs: AiConfig[];
  initialProjectId?: string | null;
  onCreate: (input: CreateSessionInput) => Promise<Session>;
  onCreated: (session: Session) => void;
  onClose: () => void;
}

interface FormErrors {
  project?: string;
  config?: string;
  allowed?: string;
  folder?: string;
  env?: string;
  form?: string;
}

function projectOption(project: Project): ComboboxOption {
  return {
    id: project.id,
    label: project.name,
    meta: project.folder,
  };
}

function defaultAllowedConfigs(
  config: AiConfig | null,
  configs: AiConfig[],
  providers: Provider[],
): string[] {
  if (!config) {
    return [];
  }
  const ids = configs
    .filter((candidate) => {
      if (candidate.client !== config.client) {
        return false;
      }
      const provider = providers.find(
        (provider) => provider.id === candidate.providerId,
      );
      return provider ? !provider.sample : false;
    })
    .map((candidate) => candidate.id);
  if (!ids.includes(config.id)) {
    ids.push(config.id);
  }
  return ids;
}

function overrideSummary(
  config: AiConfig,
  t: ReturnType<typeof useLanguage>['t'],
): string {
  const parts: string[] = [];
  if (config.env.length > 0) {
    parts.push(
      t(
        config.env.length === 1
          ? 'newSession.envOverrideOne'
          : 'newSession.envOverrideMany',
        { count: config.env.length },
      ),
    );
  }
  if (config.args.length > 0) {
    parts.push(t('configs.argsCount', { count: config.args.length }));
  }
  const configCount = Object.keys(config.config).length;
  if (configCount > 0) {
    parts.push(
      t(
        configCount === 1
          ? 'newSession.configOverrideOne'
          : 'newSession.configOverrideMany',
        { count: configCount },
      ),
    );
  }
  return parts.length > 0 ? parts.join(' · ') : t('newSession.overridesNone');
}

export function NewSessionDialog({
  projects,
  providers,
  configs,
  initialProjectId,
  onCreate,
  onCreated,
  onClose,
}: NewSessionDialogProps) {
  const { t, language } = useLanguage();
  const initialProject =
    projects.find((project) => project.id === initialProjectId) ?? null;
  const initialConfig =
    configs.find((config) => config.id === initialProject?.defaultConfigId) ??
    configs[0] ??
    null;
  const [projectId, setProjectId] = useState<string | null>(initialProjectId ?? null);
  const [kind, setKind] = useState<SessionKind>('agent');
  const [configId, setConfigId] = useState<string | null>(
    initialConfig?.id ?? null,
  );
  const [allowedIds, setAllowedIds] = useState<string[]>(() =>
    defaultAllowedConfigs(initialConfig, configs, providers),
  );
  const [allowedTouched, setAllowedTouched] = useState(false);
  const configCache = useRef(new Map<string, AiConfig>());
  const [sourceBranch, setSourceBranch] = useState<string | null>(
    initialProject?.defaultSourceBranch ?? null,
  );
  const [dirty, setDirty] = useState(false);
  const [copyUncommitted, setCopyUncommitted] = useState(false);
  const [firstPrompt, setFirstPrompt] = useState('');
  const [envText, setEnvText] = useState('');
  const [name, setName] = useState('');
  const [folder, setFolder] = useState(
    () =>
      projects.find((project) => project.id === initialProjectId)?.folder ?? '',
  );
  const [folderEdited, setFolderEdited] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [focusTarget, setFocusTarget] = useState<string | null>(null);

  useEffect(() => {
    if (!focusTarget) {
      return;
    }
    document.getElementById(focusTarget)?.focus();
    setFocusTarget(null);
  }, [focusTarget]);

  useEffect(() => {
    setCopyUncommitted(false);
    if (kind !== 'agent' || folder.trim() === '') {
      setDirty(false);
      return;
    }

    let cancelled = false;
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      getGitStatus(folder.trim(), controller.signal)
        .then((status) => {
          if (!cancelled) {
            setDirty(status.git && status.dirty);
          }
        })
        .catch(() => {
          if (!cancelled) {
            setDirty(false);
          }
        });
    }, 300);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [folder, kind]);

  const loadProjectOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listProjects(query, signal);
      return list.map(projectOption);
    },
    [],
  );

  const loadConfigOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listConfigs(query, signal);
      const options: ComboboxOption[] = [];
      for (const item of list) {
        configCache.current.set(item.id, item);
        const provider =
          providers.find((candidate) => candidate.id === item.providerId) ??
          null;
        options.push({
          ...configOption(item, providers, language),
          tag: provider?.sample ? t('common.sample') : undefined,
        });
      }
      return options;
    },
    [providers, language, t],
  );

  const loadBranchOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const path = folder.trim();
      if (!path) {
        return [];
      }
      const list = await listBranches(path, query, signal);
      return list.map((branch) => ({
        id: branch.name,
        label: branch.name,
        tag: branch.current ? t('newSession.currentTag') : undefined,
      }));
    },
    [folder, t],
  );

  const selectedProject = projectId
    ? projects.find((project) => project.id === projectId) ?? null
    : null;
  const selectedConfig = configId
    ? configs.find((config) => config.id === configId) ??
      configCache.current.get(configId) ??
      null
    : null;
  const selectedProvider = selectedConfig
    ? providers.find((provider) => provider.id === selectedConfig.providerId) ??
      null
    : null;
  const selectedUsage = selectedProvider
    ? providerUsage(selectedProvider, language)
    : null;
  const configsForClient = selectedConfig
    ? configs.filter((config) => config.client === selectedConfig.client)
    : [];

  function applyAllowedForConfig(config: AiConfig | null) {
    setAllowedIds((current) => {
      if (!allowedTouched) {
        return defaultAllowedConfigs(config, configs, providers);
      }
      if (!config) {
        return current;
      }
      return current.includes(config.id) ? current : [...current, config.id];
    });
  }

  function chooseProject(id: string | null) {
    const nextProject = projects.find((project) => project.id === id);
    const config =
      configs.find((item) => item.id === nextProject?.defaultConfigId) ??
      configs[0] ??
      null;

    setProjectId(id);
    setConfigId(config?.id ?? null);
    applyAllowedForConfig(config);
    setSourceBranch(nextProject?.defaultSourceBranch ?? null);
    setErrors((current) => ({
      ...current,
      project: undefined,
      config: undefined,
      allowed: undefined,
    }));

    if (nextProject && (!folderEdited || folder === '')) {
      setFolder(nextProject.folder);
    }
  }

  function chooseConfig(id: string | null) {
    if (!id) {
      setConfigId(null);
      applyAllowedForConfig(null);
      return;
    }
    const next =
      configs.find((config) => config.id === id) ??
      configCache.current.get(id) ??
      null;
    if (!next) {
      return;
    }
    setConfigId(next.id);
    applyAllowedForConfig(next);
    setErrors((current) => ({
      ...current,
      config: undefined,
      allowed: undefined,
    }));
  }

  async function browse() {
    try {
      const chosen = await pickFolder();
      if (chosen) {
        setFolder(chosen);
        setFolderEdited(true);
        setSourceBranch(null);
        setErrors((current) => ({ ...current, folder: undefined }));
      }
    } catch {
      setErrors((current) => ({
        ...current,
        form: t('newSession.errorPicker'),
      }));
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    const projectIsKnown = projectId
      ? projects.some((project) => project.id === projectId)
      : false;

    if (!projectIsKnown) {
      nextErrors.project = t('newSession.errorProject');
    }

    if (kind === 'agent') {
      if (!configId) {
        nextErrors.config = t('newSession.errorConfig');
      } else if (allowedIds.length === 0) {
        nextErrors.allowed = t('allowedAccounts.errorEmpty');
      }
    }

    if (folder.trim() === '') {
      nextErrors.folder = t('newSession.errorFolder');
    }

    const env: Array<{ name: string; value: string }> = [];
    for (const raw of envText.split('\n')) {
      const line = raw.trim();
      if (!line) {
        continue;
      }
      const separator = line.indexOf('=');
      if (separator <= 0) {
        nextErrors.env = t('newSession.errorEnv');
        break;
      }
      env.push({
        name: line.slice(0, separator).trim(),
        value: line.slice(separator + 1),
      });
    }

    if (Object.keys(nextErrors).length > 0) {
      const firstInvalid = nextErrors.project
        ? 'new-session-project'
        : nextErrors.config
          ? 'new-session-config'
          : nextErrors.allowed
            ? 'new-session-allowed'
            : nextErrors.folder
              ? 'new-session-folder'
              : nextErrors.env
                ? 'new-session-env'
                : null;

      setErrors(nextErrors);
      setFocusTarget(firstInvalid);
      return;
    }

    const input: CreateSessionInput = {
      projectId: projectId as string,
      kind,
      name: name.trim() || undefined,
      folder: folder.trim(),
    };

    if (kind === 'agent') {
      input.configId = configId as string;
      input.allowedConfigIds = allowedIds;
      if (sourceBranch) {
        input.sourceBranch = sourceBranch;
      }
      if (dirty && copyUncommitted) {
        input.copyUncommittedChanges = true;
      }
      if (firstPrompt.trim() !== '') {
        input.firstPrompt = firstPrompt.trim();
      }
    }
    if (env.length > 0) {
      input.env = env;
    }

    setErrors({});
    setSubmitting(true);
    try {
      const session = await onCreate(input);
      onCreated(session);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : t('newSession.errorCreate'),
      });
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="new-session-title"
      descriptionId="new-session-description"
      busyLabel={t('newSession.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="new-session-title">
          {t('newSession.title')}
        </h2>
        <p className="dialog-sub" id="new-session-description">
          {t('newSession.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="new-session-project"
            label={t('newSession.project')}
            value={projectId}
            loadOptions={loadProjectOptions}
            selectedOption={
              selectedProject ? projectOption(selectedProject) : null
            }
            onChange={chooseProject}
            placeholder={t('newSession.projectPlaceholder')}
            error={errors.project}
            required
            autoFocus
          />

          <fieldset className="field">
            <legend>{t('newSession.type')}</legend>
            <div className="segmented">
              <label className="segment">
                <input
                  type="radio"
                  name="session-kind"
                  value="agent"
                  checked={kind === 'agent'}
                  onChange={() => setKind('agent')}
                />
                <span>{t('status.agent')}</span>
              </label>
              <label className="segment">
                <input
                  type="radio"
                  name="session-kind"
                  value="terminal"
                  checked={kind === 'terminal'}
                  onChange={() => setKind('terminal')}
                />
                <span>{t('status.terminal')}</span>
              </label>
            </div>
          </fieldset>

          {kind === 'agent' ? (
            <>
              <Combobox
                id="new-session-config"
                label={t('newSession.config')}
                value={configId}
                loadOptions={loadConfigOptions}
                selectedOption={
                  selectedConfig
                    ? {
                        ...configOption(selectedConfig, providers, language),
                        tag: selectedProvider?.sample
                          ? t('common.sample')
                          : undefined,
                      }
                    : null
                }
                onChange={chooseConfig}
                placeholder={t('newSession.configPlaceholder')}
                hint={
                  configsForClient.length === 0
                    ? t('newSession.noConfigForClient')
                    : undefined
                }
                error={errors.config}
                required
              />

              {selectedConfig && selectedProvider && selectedUsage ? (
                <div className="selected-account">
                  <span className="selected-account-name">
                    {selectedConfig.name}
                  </span>
                  {selectedProvider.sample ? (
                    <span className="sample-tag">{t('common.sample')}</span>
                  ) : null}
                  <span className="selected-account-plan">
                    {selectedProvider.plan}
                  </span>
                  <span
                    className={
                      selectedUsage.warn
                        ? 'selected-account-usage is-warn'
                        : 'selected-account-usage'
                    }
                  >
                    {selectedUsage.summary}
                  </span>
                  {selectedConfig.model ? (
                    <span className="selected-account-model">
                      {t('newSession.modelWith', {
                        model: selectedConfig.model,
                      })}
                    </span>
                  ) : null}
                  <span className="selected-account-overrides">
                    {overrideSummary(selectedConfig, t)}
                  </span>
                </div>
              ) : null}

              <ConfigMultiSelect
                id="new-session-allowed"
                label={t('allowedAccounts.label')}
                configs={configs}
                providers={providers}
                value={allowedIds}
                requiredId={configId}
                onChange={(ids) => {
                  setAllowedIds(ids);
                  setAllowedTouched(true);
                  setErrors((current) => ({
                    ...current,
                    allowed: undefined,
                  }));
                }}
                hint={t('allowedAccounts.hint')}
                error={errors.allowed}
                disabled={!configId}
              />
            </>
          ) : null}

          <div className="field">
            <label htmlFor="new-session-folder">
              {kind === 'agent'
                ? t('newSession.repositoryFolder')
                : t('newSession.folder')}
            </label>
            <div className="field-row">
              <input
                id="new-session-folder"
                className="text-input"
                value={folder}
                placeholder={t('newSession.folderPlaceholder')}
                spellCheck={false}
                required
                aria-invalid={errors.folder ? true : undefined}
                aria-describedby={
                  [
                    kind === 'agent' ? 'new-session-folder-hint' : null,
                    errors.folder ? 'new-session-folder-error' : null,
                  ]
                    .filter(Boolean)
                    .join(' ') || undefined
                }
                onChange={(event) => {
                  setFolder(event.target.value);
                  setFolderEdited(true);
                  setSourceBranch(null);
                  setErrors((current) => ({ ...current, folder: undefined }));
                }}
              />
              {isTauri() ? (
                <button type="button" className="btn btn-ghost" onClick={() => void browse()}>
                  {t('newSession.browse')}
                </button>
              ) : null}
            </div>
            {kind === 'agent' ? (
              <p className="field-hint" id="new-session-folder-hint">
                {t('newSession.gitHint')}
              </p>
            ) : null}
            {errors.folder ? (
              <p className="field-error" id="new-session-folder-error" role="alert">
                {errors.folder}
              </p>
            ) : null}
          </div>

          {kind === 'agent' ? (
            <>
              <Combobox
                key={folder}
                id="new-session-branch"
                label={t('newSession.sourceBranch')}
                value={sourceBranch}
                loadOptions={loadBranchOptions}
                selectedOption={
                  sourceBranch
                    ? { id: sourceBranch, label: sourceBranch }
                    : null
                }
                onChange={setSourceBranch}
                placeholder={t('newSession.sourceBranchPlaceholder')}
                hint={t('newSession.sourceBranchHint')}
              />

              <div className="field">
                <label htmlFor="new-session-prompt">
                  {t('newSession.firstPrompt')}
                </label>
                <textarea
                  id="new-session-prompt"
                  className="text-input prompt-input"
                  value={firstPrompt}
                  placeholder={t('newSession.firstPromptPlaceholder')}
                  rows={3}
                  onChange={(event) => setFirstPrompt(event.target.value)}
                />
                <p className="field-hint">
                  {t('newSession.firstPromptHint')}
                </p>
              </div>
            </>
          ) : null}

          <div className="field">
            <label htmlFor="new-session-name">{t('newSession.name')}</label>
            <input
              id="new-session-name"
              className="text-input"
              value={name}
              placeholder={t('newSession.namePlaceholder')}
              onChange={(event) => setName(event.target.value)}
            />
          </div>

          <details className="field additional-settings">
            <summary>{t('newSession.additional')}</summary>
            {dirty ? (
              <fieldset className="field" id="new-session-uncommitted">
                <legend>{t('newSession.uncommitted')}</legend>
                <p className="field-hint">{t('newSession.uncommittedHint')}</p>
                <div className="segmented">
                  <label className="segment">
                    <input
                      type="radio"
                      name="uncommitted-changes"
                      value="copy"
                      checked={copyUncommitted}
                      onChange={() => setCopyUncommitted(true)}
                    />
                    <span>{t('newSession.uncommittedCopy')}</span>
                  </label>
                  <label className="segment">
                    <input
                      type="radio"
                      name="uncommitted-changes"
                      value="leave"
                      checked={!copyUncommitted}
                      onChange={() => setCopyUncommitted(false)}
                    />
                    <span>{t('newSession.uncommittedLeave')}</span>
                  </label>
                </div>
              </fieldset>
            ) : null}
            <div className="field">
              <label htmlFor="new-session-env">{t('newSession.env')}</label>
              <textarea
                id="new-session-env"
                className="text-input prompt-input"
                value={envText}
                placeholder={'NAME=value\nANOTHER_NAME=value'}
                rows={3}
                spellCheck={false}
                aria-invalid={errors.env ? true : undefined}
                aria-describedby={
                  errors.env ? 'new-session-env-error' : 'new-session-env-hint'
                }
                onChange={(event) => {
                  setEnvText(event.target.value);
                  setErrors((current) => ({ ...current, env: undefined }));
                }}
              />
              {errors.env ? (
                <p
                  className="field-error"
                  id="new-session-env-error"
                  role="alert"
                >
                  {errors.env}
                </p>
              ) : (
                <p className="field-hint" id="new-session-env-hint">
                  {t('newSession.envHint')}
                </p>
              )}
            </div>
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
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting
              ? t('newSession.submitting')
              : t('newSession.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
