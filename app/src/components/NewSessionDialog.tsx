import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError, listAccounts, listBranches, listProjects } from '../api/client';
import { isTauri, pickFolder } from '../api/shell';
import type {
  Account,
  Client,
  CreateSessionInput,
  Project,
  Session,
  SessionKind,
} from '../api/types';
import { CLIENTS } from '../api/types';
import { type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { accountUsage } from '../state/usage';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';
import {
  Combobox,
  type ComboboxLoader,
  type ComboboxOption,
} from './Combobox';

interface NewSessionDialogProps {
  projects: Project[];
  accounts: Account[];
  initialProjectId?: string | null;
  onCreate: (input: CreateSessionInput) => Promise<Session>;
  onCreated: (session: Session) => void;
  onClose: () => void;
}

interface FormErrors {
  project?: string;
  client?: string;
  account?: string;
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

function accountOption(
  account: Account,
  language: Language,
  sampleLabel: string,
): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${accountUsage(account, language).summary} · ${account.plan}`,
    tag: account.sample ? sampleLabel : undefined,
  };
}

function defaultAccount(
  project: Project | null | undefined,
  accounts: Account[],
): Account | null {
  const id = project?.defaultAccountId;
  if (!id) {
    return null;
  }
  return accounts.find((account) => account.id === id) ?? null;
}

export function NewSessionDialog({
  projects,
  accounts,
  initialProjectId,
  onCreate,
  onCreated,
  onClose,
}: NewSessionDialogProps) {
  const { t, language } = useLanguage();
  const initialProject =
    projects.find((project) => project.id === initialProjectId) ?? null;
  const initialAccount = defaultAccount(initialProject, accounts);
  const [projectId, setProjectId] = useState<string | null>(initialProjectId ?? null);
  const [kind, setKind] = useState<SessionKind>('agent');
  const [client, setClient] = useState<Client | null>(
    initialAccount?.client ?? null,
  );
  const [accountId, setAccountId] = useState<string | null>(
    initialAccount?.id ?? null,
  );
  const [sourceBranch, setSourceBranch] = useState<string | null>(
    initialProject?.defaultSourceBranch ?? null,
  );
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

    const element = document.getElementById(focusTarget);
    if (element instanceof HTMLFieldSetElement) {
      element.querySelector<HTMLInputElement>('input')?.focus();
    } else {
      element?.focus();
    }

    setFocusTarget(null);
  }, [focusTarget]);

  const loadProjectOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listProjects(query, signal);
      return list.map(projectOption);
    },
    [],
  );

  const loadAccountOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!client) {
        return [];
      }
      const list = await listAccounts(query, signal);
      return list
        .filter((account) => account.client === client)
        .map((account) =>
          accountOption(account, language, t('common.sample')),
        );
    },
    [client, language, t],
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
  const selectedAccount = accountId
    ? accounts.find((account) => account.id === accountId) ?? null
    : null;
  const selectedUsage = selectedAccount
    ? accountUsage(selectedAccount, language)
    : null;
  const accountsForClient = client
    ? accounts.filter((account) => account.client === client)
    : [];

  function chooseProject(id: string | null) {
    const nextProject = projects.find((project) => project.id === id);
    const account = defaultAccount(nextProject, accounts);

    setProjectId(id);
    setClient(account?.client ?? null);
    setAccountId(account?.id ?? null);
    setSourceBranch(nextProject?.defaultSourceBranch ?? null);
    setErrors((current) => ({
      ...current,
      project: undefined,
      client: undefined,
      account: undefined,
    }));

    if (nextProject && (!folderEdited || folder === '')) {
      setFolder(nextProject.folder);
    }
  }

  function chooseClient(nextClient: Client) {
    setClient(nextClient);
    setAccountId(null);
    setErrors((current) => ({ ...current, client: undefined, account: undefined }));
  }

  function chooseAccount(id: string | null) {
    setAccountId(id);
    setErrors((current) => ({ ...current, account: undefined }));
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
      if (!client) {
        nextErrors.client = t('newSession.errorClient');
        nextErrors.account = t('newSession.errorClientAccount');
      } else if (!accountId) {
        nextErrors.account = t('newSession.errorAccount');
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
        : nextErrors.client
          ? 'new-session-client'
          : nextErrors.account
            ? 'new-session-account'
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
      input.accountId = accountId as string;
      if (sourceBranch) {
        input.sourceBranch = sourceBranch;
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
              <fieldset
                className="field"
                id="new-session-client"
                aria-describedby={
                  errors.client ? 'new-session-client-error' : undefined
                }
              >
                <legend>{t('newSession.client')}</legend>
                <div className="segmented">
                  {CLIENTS.map((item) => (
                    <label className="segment" key={item}>
                      <input
                        type="radio"
                        name="client"
                        value={item}
                        checked={client === item}
                        required
                        aria-invalid={errors.client ? true : undefined}
                        onChange={() => chooseClient(item)}
                      />
                      <span>{item}</span>
                    </label>
                  ))}
                </div>
                {errors.client ? (
                  <p className="field-error" id="new-session-client-error" role="alert">
                    {errors.client}
                  </p>
                ) : null}
              </fieldset>

              <Combobox
                key={client ?? 'no-client'}
                id="new-session-account"
                label={t('newSession.account')}
                value={accountId}
                loadOptions={loadAccountOptions}
                selectedOption={
                  selectedAccount
                    ? accountOption(
                        selectedAccount,
                        language,
                        t('common.sample'),
                      )
                    : null
                }
                onChange={chooseAccount}
                placeholder={
                  client
                    ? t('newSession.accountPlaceholder')
                    : t('newSession.accountPlaceholderNoClient')
                }
                hint={
                  client && accountsForClient.length === 0
                    ? t('newSession.noAccountForClient', { client })
                    : undefined
                }
                error={errors.account}
                required={Boolean(client)}
                disabled={!client}
              />

              {selectedAccount && selectedUsage ? (
                <div className="selected-account">
                  <span className="selected-account-name">
                    {selectedAccount.name}
                  </span>
                  {selectedAccount.sample ? (
                    <span className="sample-tag">{t('common.sample')}</span>
                  ) : null}
                  <span className="selected-account-plan">
                    {selectedAccount.plan}
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
                </div>
              ) : null}
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
