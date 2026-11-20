import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import {
  ApiError,
  cloneGitHubRepo,
  getGitHubStatus,
  getGitStatus,
  listBranches,
  listConfigs,
  listGitHubRepos,
  listProjects,
} from '../api/client';
import { isTauri, pickFolder } from '../api/shell';
import type {
  AiConfig,
  CloneGitHubRepoInput,
  CreateSessionInput,
  GitHubRepo,
  GitHubStatus,
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
  sessions: Session[];
  initialProjectId?: string | null;
  onCreate: (input: CreateSessionInput) => Promise<Session>;
  onCreated: (session: Session) => void;
  onClose: () => void;
  github?: RepositoryGitHubActions;
}

/** The GitHub calls the repository search uses. Tests replace them. */
export interface RepositoryGitHubActions {
  status: () => Promise<GitHubStatus>;
  listRepos: (query: string, signal?: AbortSignal) => Promise<GitHubRepo[]>;
  clone: (input: CloneGitHubRepoInput) => Promise<Project>;
}

const DEFAULT_GITHUB_ACTIONS: RepositoryGitHubActions = {
  status: getGitHubStatus,
  listRepos: listGitHubRepos,
  clone: cloneGitHubRepo,
};

interface FormErrors {
  project?: string;
  config?: string;
  allowed?: string;
  folder?: string;
  cloneFolder?: string;
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

function folderLabel(folder: string): string {
  const trimmed = folder.replace(/[\\/]+$/, '');
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] || folder;
}

const RECENT_REPOSITORY_LIMIT = 20;
const GITHUB_REPO_PREFIX = 'github:';

function sessionRepository(
  session: Session,
  projectsById: Map<string, Project>,
): string | undefined {
  const source = session.sourceRepository?.trim();
  if (source) {
    return source;
  }
  return projectsById.get(session.projectId)?.folder?.trim() || undefined;
}

function recentRepositoryOptions(
  sessions: Session[],
  projects: Project[],
  query: string,
): ComboboxOption[] {
  const projectsById = new Map(
    projects.map((project) => [project.id, project]),
  );
  const normalized = query.trim().toLowerCase();
  const latest = new Map<string, number>();

  for (const session of sessions) {
    const path = sessionRepository(session, projectsById);
    if (!path) {
      continue;
    }
    const started = Date.parse(session.startedAt) || 0;
    if (!latest.has(path) || started > latest.get(path)!) {
      latest.set(path, started);
    }
  }

  const entries = [...latest.entries()].sort(
    (left, right) => right[1] - left[1],
  );
  const options: ComboboxOption[] = [];
  for (const [path] of entries) {
    const label = folderLabel(path);
    if (
      normalized &&
      !path.toLowerCase().includes(normalized) &&
      !label.toLowerCase().includes(normalized)
    ) {
      continue;
    }
    options.push({ id: path, label, closedLabel: path, meta: path });
    if (options.length === RECENT_REPOSITORY_LIMIT) {
      break;
    }
  }
  return options;
}

/** The local folder of a repository that a project already holds. */
function githubRepoLocalFolder(repo: GitHubRepo): string | null {
  return repo.localFolder?.trim() || null;
}

function mergeRepositoryOptions(
  recent: ComboboxOption[],
  repos: GitHubRepo[],
  t: ReturnType<typeof useLanguage>['t'],
): ComboboxOption[] {
  const options = [...recent];
  const seen = new Set(recent.map((option) => option.id));
  for (const repo of repos) {
    const local = githubRepoLocalFolder(repo);
    const id = local ?? `${GITHUB_REPO_PREFIX}${repo.fullName}`;
    if (seen.has(id)) {
      continue;
    }
    seen.add(id);
    const visibility = repo.private
      ? t('newSession.githubPrivate')
      : t('newSession.githubPublic');
    options.push({
      id,
      label: local ? folderLabel(local) : repo.fullName,
      closedLabel: local ?? repo.fullName,
      meta: local ?? `${visibility} · ${repo.cloneUrl}`,
      tag: local ? t('newSession.githubAdded') : t('newSession.githubClone'),
    });
  }
  return options;
}

function joinFolder(parent: string, name: string): string {
  const trimmed = parent.replace(/[\\/]+$/, '');
  return trimmed ? `${trimmed}/${name}` : name;
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
  sessions,
  initialProjectId,
  onCreate,
  onCreated,
  onClose,
  github = DEFAULT_GITHUB_ACTIONS,
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
  const [githubStatus, setGithubStatus] = useState<GitHubStatus | null>(null);
  const [remoteRepo, setRemoteRepo] = useState<GitHubRepo | null>(null);
  const [cloneFolder, setCloneFolder] = useState('');
  const githubRepos = useRef(new Map<string, GitHubRepo>());
  const githubStatusRequest = useRef<Promise<GitHubStatus | null> | null>(null);

  useEffect(() => {
    if (!githubStatusRequest.current) {
      githubStatusRequest.current = github.status().catch(() => null);
    }
    let cancelled = false;
    void githubStatusRequest.current.then((next) => {
      if (cancelled || !next) {
        return;
      }
      setGithubStatus(next);
      if (next.connected && next.defaultCloneFolder) {
        setCloneFolder((current) => current || next.defaultCloneFolder);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [github]);

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

  const loadRepositoryOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (kind !== 'agent') {
        return [];
      }
      await new Promise<void>((resolve) => {
        const timer = window.setTimeout(resolve, 0);
        signal.addEventListener(
          'abort',
          () => {
            window.clearTimeout(timer);
            resolve();
          },
          { once: true },
        );
      });
      if (signal.aborted) {
        return [];
      }
      const recent = recentRepositoryOptions(sessions, projects, query);
      if (!githubStatusRequest.current) {
        githubStatusRequest.current = github.status().catch(() => null);
      }
      const status = githubStatus ?? (await githubStatusRequest.current);
      if (signal.aborted) {
        return recent;
      }
      if (!status?.connected) {
        return recent;
      }
      let repos: GitHubRepo[] = [];
      try {
        repos = await github.listRepos(query, signal);
      } catch {
        repos = [];
      }
      if (signal.aborted) {
        return recent;
      }
      for (const repo of repos) {
        githubRepos.current.set(
          `${GITHUB_REPO_PREFIX}${repo.fullName}`,
          repo,
        );
      }
      return mergeRepositoryOptions(recent, repos, t);
    },
    [kind, projects, sessions, githubStatus, github, t],
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

  function chooseKind(next: SessionKind) {
    if (next !== 'agent') {
      setRemoteRepo(null);
    }
    setKind(next);
  }

  function applyRepository(next: string) {
    if (next !== folder.trim()) {
      setSourceBranch(null);
    }
    setRemoteRepo(null);
    setFolder(next);
    setFolderEdited(true);
    setErrors((current) => ({ ...current, folder: undefined }));
  }

  async function browse() {
    try {
      const chosen = await pickFolder();
      if (chosen) {
        applyRepository(chosen);
      }
    } catch {
      setErrors((current) => ({
        ...current,
        form: t('newSession.errorPicker'),
      }));
    }
  }

  function chooseGitHubRepository(repo: GitHubRepo) {
    const local = githubRepoLocalFolder(repo);
    if (local) {
      applyRepository(local);
      return;
    }
    const parent =
      cloneFolder.trim() || githubStatus?.defaultCloneFolder || '';
    if (parent && !cloneFolder.trim()) {
      setCloneFolder(parent);
    }
    const target = parent ? joinFolder(parent, repo.name) : '';
    if (target !== folder.trim()) {
      setSourceBranch(null);
    }
    setRemoteRepo(repo);
    setFolder(target);
    setFolderEdited(true);
    setErrors((current) => ({
      ...current,
      folder: undefined,
      cloneFolder: undefined,
    }));
  }

  function chooseRepository(id: string | null) {
    if (id?.startsWith(GITHUB_REPO_PREFIX)) {
      const repo = githubRepos.current.get(id);
      if (repo) {
        chooseGitHubRepository(repo);
        return;
      }
    }
    applyRepository(id?.trim() ?? '');
  }

  function changeCloneFolder(next: string) {
    setCloneFolder(next);
    setErrors((current) => ({ ...current, cloneFolder: undefined }));
    if (remoteRepo) {
      setFolder(joinFolder(next.trim(), remoteRepo.name));
      setFolderEdited(true);
    }
  }

  async function browseCloneFolder() {
    try {
      const chosen = await pickFolder();
      if (chosen) {
        changeCloneFolder(chosen);
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

    if (kind === 'agent' && remoteRepo) {
      if (cloneFolder.trim() === '') {
        nextErrors.cloneFolder = t('newSession.errorCloneFolder');
      }
    } else if (folder.trim() === '') {
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
              : nextErrors.cloneFolder
                ? 'new-session-clone-folder'
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
      if (kind === 'agent' && remoteRepo) {
        const cloned = await github.clone({
          fullName: remoteRepo.fullName,
          parentFolder: cloneFolder.trim(),
        });
        input.folder = cloned.folder;
      }
      const session = await onCreate(input);
      onCreated(session);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : kind === 'agent' && remoteRepo
              ? t('newSession.errorClone')
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
      className="new-session-dialog"
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
                  onChange={() => chooseKind('agent')}
                />
                <span>{t('status.agent')}</span>
              </label>
              <label className="segment">
                <input
                  type="radio"
                  name="session-kind"
                  value="terminal"
                  checked={kind === 'terminal'}
                  onChange={() => chooseKind('terminal')}
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

          {kind === 'agent' ? (
            <Combobox
              id="new-session-folder"
              label={t('newSession.repositoryFolder')}
              value={
                remoteRepo
                  ? `${GITHUB_REPO_PREFIX}${remoteRepo.fullName}`
                  : folder.trim() || null
              }
              loadOptions={loadRepositoryOptions}
              selectedOption={
                remoteRepo
                  ? {
                      id: `${GITHUB_REPO_PREFIX}${remoteRepo.fullName}`,
                      label: remoteRepo.fullName,
                      closedLabel: remoteRepo.fullName,
                      meta: remoteRepo.cloneUrl,
                      tag: t('newSession.githubClone'),
                    }
                  : folder.trim()
                    ? {
                        id: folder.trim(),
                        label: folderLabel(folder.trim()),
                        closedLabel: folder.trim(),
                        meta: folder.trim(),
                      }
                    : null
              }
              onChange={chooseRepository}
              placeholder={t('newSession.repositoryPlaceholder')}
              hint={t('newSession.gitHint')}
              error={errors.folder}
              required
              allowCustom
              action={
                isTauri() ? (
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => void browse()}
                  >
                    {t('newSession.browse')}
                  </button>
                ) : undefined
              }
            />
          ) : (
            <div className="field">
              <label htmlFor="new-session-folder">{t('newSession.folder')}</label>
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
                    errors.folder ? 'new-session-folder-error' : undefined
                  }
                  onChange={(event) => applyRepository(event.target.value)}
                />
                {isTauri() ? (
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => void browse()}
                  >
                    {t('newSession.browse')}
                  </button>
                ) : null}
              </div>
              {errors.folder ? (
                <p
                  className="field-error"
                  id="new-session-folder-error"
                  role="alert"
                >
                  {errors.folder}
                </p>
              ) : null}
            </div>
          )}

          {kind === 'agent' && remoteRepo ? (
            <div className="field">
              <label htmlFor="new-session-clone-folder">
                {t('newSession.cloneFolder')}
              </label>
              <div className="field-row">
                <input
                  id="new-session-clone-folder"
                  className="text-input"
                  value={cloneFolder}
                  placeholder={t('newSession.cloneFolderPlaceholder')}
                  spellCheck={false}
                  aria-invalid={errors.cloneFolder ? true : undefined}
                  aria-describedby={
                    errors.cloneFolder
                      ? 'new-session-clone-folder-error'
                      : 'new-session-clone-folder-hint'
                  }
                  onChange={(event) => changeCloneFolder(event.target.value)}
                />
                {isTauri() ? (
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => void browseCloneFolder()}
                  >
                    {t('newSession.browse')}
                  </button>
                ) : null}
              </div>
              {errors.cloneFolder ? (
                <p
                  className="field-error"
                  id="new-session-clone-folder-error"
                  role="alert"
                >
                  {errors.cloneFolder}
                </p>
              ) : (
                <p className="field-hint" id="new-session-clone-folder-hint">
                  {t('newSession.cloneFolderHint', {
                    path: joinFolder(cloneFolder.trim(), remoteRepo.name),
                  })}
                </p>
              )}
            </div>
          ) : null}

          {kind === 'agent' ? (
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
            {kind === 'agent' ? (
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
            ) : null}
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
