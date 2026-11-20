import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from 'react';
import {
  ApiError,
  cancelGitHubSignIn,
  cloneGitHubRepo,
  getGitHubSignIn,
  getGitHubStatus,
  listGitHubRepos,
  signOutGitHub,
  startGitHubSignIn,
} from '../api/client';
import { isTauri, openUrl, pickFolder } from '../api/shell';
import type {
  CloneGitHubRepoInput,
  CreateProjectInput,
  GitHubDevice,
  GitHubDeviceReason,
  GitHubRepo,
  GitHubStatus,
  Project,
} from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import type { MessageKey } from '../i18n/en';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';
import {
  Combobox,
  type ComboboxLoader,
  type ComboboxOption,
} from './Combobox';

export interface GitHubActions {
  status: () => Promise<GitHubStatus>;
  startSignIn: () => Promise<GitHubDevice>;
  getSignIn: (id: string) => Promise<GitHubDevice>;
  cancelSignIn: (id: string) => Promise<void>;
  signOut: () => Promise<void>;
  listRepos: (query: string, signal?: AbortSignal) => Promise<GitHubRepo[]>;
  clone: (input: CloneGitHubRepoInput) => Promise<Project>;
}

const DEFAULT_GITHUB: GitHubActions = {
  status: getGitHubStatus,
  startSignIn: startGitHubSignIn,
  getSignIn: getGitHubSignIn,
  cancelSignIn: cancelGitHubSignIn,
  signOut: signOutGitHub,
  listRepos: listGitHubRepos,
  clone: cloneGitHubRepo,
};

interface AddProjectDialogProps {
  onCreate: (input: CreateProjectInput) => Promise<Project>;
  onCreated: (project: Project) => void;
  onClose: () => void;
  github?: Partial<GitHubActions>;
}

interface FormErrors {
  name?: string;
  folder?: string;
  repo?: string;
  form?: string;
}

type SourceMode = 'folder' | 'github';

function joinPath(parent: string, name: string): string {
  if (parent === '') {
    return name;
  }
  return `${parent.replace(/[\\/]+$/, '')}/${name}`;
}

function reasonKey(reason: GitHubDeviceReason | null): MessageKey {
  switch (reason) {
    case 'notConfigured':
      return 'addProject.githubNotConfigured';
    case 'network':
      return 'addProject.githubNetwork';
    case 'denied':
      return 'addProject.githubDenied';
    case 'expired':
      return 'addProject.githubExpired';
    case 'keychain':
      return 'addProject.githubKeychain';
    default:
      return 'addProject.githubSignInFailed';
  }
}

export function AddProjectDialog({
  onCreate,
  onCreated,
  onClose,
  github,
}: AddProjectDialogProps) {
  const { t } = useLanguage();
  const actions = useMemo<GitHubActions>(
    () => ({ ...DEFAULT_GITHUB, ...github }),
    [github],
  );
  const [mode, setMode] = useState<SourceMode>('folder');
  const [name, setName] = useState('');
  const [nameTouched, setNameTouched] = useState(false);
  const [localFolder, setLocalFolder] = useState('');
  const [cloneFolder, setCloneFolder] = useState('');
  const [repo, setRepo] = useState<GitHubRepo | null>(null);
  const [errors, setErrors] = useState<FormErrors>({});
  const [submitting, setSubmitting] = useState(false);
  const [focusTarget, setFocusTarget] = useState<string | null>(null);

  const [status, setStatus] = useState<GitHubStatus | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [statusFailed, setStatusFailed] = useState(false);
  const [statusRequested, setStatusRequested] = useState(false);
  const [device, setDevice] = useState<GitHubDevice | null>(null);
  const [startingSignIn, setStartingSignIn] = useState(false);
  const [signInError, setSignInError] = useState<MessageKey | null>(null);
  const [signingOut, setSigningOut] = useState(false);

  const reposRef = useRef<Map<string, GitHubRepo>>(new Map());
  const flowRef = useRef<string | null>(null);

  const loadStatus = useCallback(async () => {
    setStatusLoading(true);
    setStatusFailed(false);
    try {
      const next = await actions.status();
      setStatus(next);
      setCloneFolder((current) =>
        current === '' ? next.defaultCloneFolder : current,
      );
    } catch {
      setStatusFailed(true);
    } finally {
      setStatusLoading(false);
    }
  }, [actions]);

  useEffect(() => {
    if (mode !== 'github' || statusRequested) {
      return;
    }
    setStatusRequested(true);
    void loadStatus();
  }, [mode, statusRequested, loadStatus]);

  useEffect(() => {
    if (!device) {
      return;
    }
    if (device.state === 'success') {
      flowRef.current = null;
      setStartingSignIn(false);
      setStatus((current) => ({
        connected: true,
        login: device.login,
        name: current?.name ?? null,
        avatarUrl: current?.avatarUrl ?? null,
        defaultCloneFolder: current?.defaultCloneFolder ?? '',
      }));
    } else if (device.state === 'failed' || device.state === 'cancelled') {
      flowRef.current = null;
      setStartingSignIn(false);
    }
  }, [device]);

  useEffect(() => {
    if (!device || device.state !== 'waiting') {
      return;
    }
    let active = true;
    const timer = window.setTimeout(
      () => {
        void (async () => {
          try {
            const next = await actions.getSignIn(device.id);
            if (active) {
              setDevice(next);
            }
          } catch {
            if (active) {
              setDevice(null);
              setSignInError('addProject.githubSignInFailed');
            }
          }
        })();
      },
      Math.max(1, device.interval) * 1000,
    );
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [device, actions]);

  useEffect(
    () => () => {
      const id = flowRef.current;
      if (id) {
        void actions.cancelSignIn(id).catch(() => undefined);
      }
    },
    [actions],
  );

  useEffect(() => {
    if (!focusTarget) {
      return;
    }
    document.getElementById(focusTarget)?.focus();
    setFocusTarget(null);
  }, [focusTarget]);

  const loadRepoOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const repos = await actions.listRepos(query, signal);
      reposRef.current = new Map(
        repos.map((item) => [item.fullName, item]),
      );
      return repos.map<ComboboxOption>((item) => ({
        id: item.fullName,
        label: item.fullName,
        meta: `${item.private ? t('addProject.githubPrivate') : t('addProject.githubPublic')} · ${item.cloneUrl}`,
        tag: item.added ? t('addProject.githubAdded') : undefined,
      }));
    },
    [actions, t],
  );

  function chooseMode(next: SourceMode) {
    setMode(next);
    setErrors({});
    setSignInError(null);
    if (
      next === 'github' &&
      cloneFolder === '' &&
      status?.defaultCloneFolder
    ) {
      setCloneFolder(status.defaultCloneFolder);
    }
  }

  async function connect() {
    setSignInError(null);
    setStartingSignIn(true);
    setDevice(null);
    try {
      const next = await actions.startSignIn();
      setDevice(next);
      if (next.state === 'waiting' || next.state === 'starting') {
        flowRef.current = next.id;
      } else {
        setStartingSignIn(false);
      }
    } catch (error) {
      setStartingSignIn(false);
      setSignInError(
        error instanceof ApiError && error.status === 422
          ? 'addProject.githubNotConfigured'
          : error instanceof ApiError &&
              (error.status === 502 || error.status === 408)
            ? 'addProject.githubNetwork'
            : 'addProject.githubSignInFailed',
      );
    }
  }

  async function disconnect() {
    setSigningOut(true);
    setSignInError(null);
    try {
      await actions.signOut();
      setStatus((current) => ({
        connected: false,
        login: null,
        name: null,
        avatarUrl: null,
        defaultCloneFolder: current?.defaultCloneFolder ?? '',
      }));
      setRepo(null);
    } catch {
      setSignInError('addProject.githubSignOutFailed');
    } finally {
      setSigningOut(false);
    }
  }

  function chooseRepo(id: string | null) {
    const chosen = id ? reposRef.current.get(id) ?? null : null;
    setRepo(chosen);
    setErrors((current) => ({ ...current, repo: undefined, form: undefined }));
    if (chosen && !nameTouched) {
      setName(chosen.name);
    }
  }

  async function browse(target: 'local' | 'clone') {
    try {
      const chosen = await pickFolder();
      if (chosen) {
        if (target === 'local') {
          setLocalFolder(chosen);
          setErrors((current) => ({ ...current, folder: undefined }));
        } else {
          setCloneFolder(chosen);
          setErrors((current) => ({ ...current, folder: undefined }));
        }
      }
    } catch {
      setErrors((current) => ({
        ...current,
        form: t('addProject.errorPicker'),
      }));
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    if (name.trim() === '') {
      nextErrors.name = t('addProject.errorName');
    }
    if (mode === 'folder') {
      if (localFolder.trim() === '') {
        nextErrors.folder = t('addProject.errorFolder');
      }
    } else {
      if (!repo) {
        nextErrors.repo = t('addProject.githubErrorRepo');
      }
      if (cloneFolder.trim() === '') {
        nextErrors.folder = t('addProject.githubErrorFolder');
      }
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      const focus = nextErrors.name
        ? 'add-project-name'
        : nextErrors.repo
          ? 'add-project-repo'
          : nextErrors.folder
            ? mode === 'github'
              ? 'add-project-clone-folder'
              : 'add-project-folder'
            : null;
      setFocusTarget(focus);
      return;
    }

    setErrors({});
    setSubmitting(true);
    try {
      const project =
        mode === 'folder'
          ? await onCreate({
              name: name.trim(),
              folder: localFolder.trim(),
            })
          : await actions.clone({
              fullName: (repo as GitHubRepo).fullName,
              name: name.trim(),
              parentFolder: cloneFolder.trim(),
            });
      onCreated(project);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : mode === 'github'
              ? t('addProject.githubErrorClone')
              : t('addProject.errorAdd'),
      });
      setSubmitting(false);
    }
  }

  const signInFailure =
    signInError ??
    (device?.state === 'failed' ? reasonKey(device.reason) : null);
  const connected = status?.connected === true;
  const cloneTarget =
    repo && cloneFolder.trim() !== ''
      ? joinPath(cloneFolder.trim(), repo.name)
      : '';
  const submitLabel =
    mode === 'github' && connected
      ? submitting
        ? t('addProject.githubCloning')
        : t('addProject.githubClone')
      : submitting
        ? t('addProject.submitting')
        : t('addProject.submit');
  const submitDisabled =
    submitting || (mode === 'github' && !connected);

  return (
    <Dialog
      titleId="add-project-title"
      descriptionId="add-project-description"
      busyLabel={t('addProject.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="add-project-title">
          {t('addProject.title')}
        </h2>
        <p className="dialog-sub" id="add-project-description">
          {mode === 'github'
            ? t('addProject.githubSubtitle')
            : t('addProject.subtitle')}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <fieldset className="field">
            <legend>{t('addProject.source')}</legend>
            <div className="segmented">
              <label className="segment">
                <input
                  type="radio"
                  name="project-source"
                  value="folder"
                  checked={mode === 'folder'}
                  onChange={() => chooseMode('folder')}
                />
                <span>{t('addProject.sourceFolder')}</span>
              </label>
              <label className="segment">
                <input
                  type="radio"
                  name="project-source"
                  value="github"
                  checked={mode === 'github'}
                  onChange={() => chooseMode('github')}
                />
                <span>{t('addProject.sourceGithub')}</span>
              </label>
            </div>
          </fieldset>

          {mode === 'folder' ? (
            <>
              <div className="field">
                <label htmlFor="add-project-name">
                  {t('addProject.name')}
                </label>
                <input
                  id="add-project-name"
                  className="text-input"
                  value={name}
                  placeholder={t('addProject.namePlaceholder')}
                  autoFocus
                  required
                  aria-invalid={errors.name ? true : undefined}
                  aria-describedby={
                    errors.name ? 'add-project-name-error' : undefined
                  }
                  onChange={(event) => {
                    setName(event.target.value);
                    setNameTouched(true);
                    setErrors((current) => ({
                      ...current,
                      name: undefined,
                    }));
                  }}
                />
                {errors.name ? (
                  <p
                    className="field-error"
                    id="add-project-name-error"
                    role="alert"
                  >
                    {errors.name}
                  </p>
                ) : null}
              </div>

              <div className="field">
                <label htmlFor="add-project-folder">
                  {t('addProject.folder')}
                </label>
                <div className="field-row">
                  <input
                    id="add-project-folder"
                    className="text-input"
                    value={localFolder}
                    placeholder={t('addProject.folderPlaceholder')}
                    spellCheck={false}
                    required
                    aria-invalid={errors.folder ? true : undefined}
                    aria-describedby={
                      errors.folder ? 'add-project-folder-error' : undefined
                    }
                    onChange={(event) => {
                      setLocalFolder(event.target.value);
                      setErrors((current) => ({
                        ...current,
                        folder: undefined,
                      }));
                    }}
                  />
                  {isTauri() ? (
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => void browse('local')}
                    >
                      {t('addProject.browse')}
                    </button>
                  ) : null}
                </div>
                {errors.folder ? (
                  <p
                    className="field-error"
                    id="add-project-folder-error"
                    role="alert"
                  >
                    {errors.folder}
                  </p>
                ) : null}
              </div>
            </>
          ) : (
            <div className="sign-in-panel">
              {statusLoading ? (
                <p className="sign-in-status" role="status">
                  {t('addProject.githubLoading')}
                </p>
              ) : statusFailed ? (
                <div className="sign-in-status" role="alert">
                  <p>{t('addProject.githubStatusFailed')}</p>
                  <button
                    type="button"
                    className="btn btn-ghost"
                    onClick={() => {
                      setStatusRequested(false);
                    }}
                  >
                    {t('common.retry')}
                  </button>
                </div>
              ) : connected ? (
                <>
                  <div className="sign-in-head">
                    <p className="sign-in-status" role="status">
                      {status?.login
                        ? t('addProject.githubSignedInAs', {
                            login: status.login,
                          })
                        : t('addProject.githubSignedIn')}
                    </p>
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => void disconnect()}
                      disabled={signingOut}
                    >
                      {t('addProject.githubDisconnect')}
                    </button>
                  </div>

                  <Combobox
                    id="add-project-repo"
                    label={t('addProject.githubRepo')}
                    value={repo?.fullName ?? null}
                    loadOptions={loadRepoOptions}
                    selectedOption={
                      repo
                        ? {
                            id: repo.fullName,
                            label: repo.fullName,
                            meta: `${repo.private ? t('addProject.githubPrivate') : t('addProject.githubPublic')} · ${repo.cloneUrl}`,
                            tag: repo.added
                              ? t('addProject.githubAdded')
                              : undefined,
                          }
                        : null
                    }
                    onChange={chooseRepo}
                    placeholder={t('addProject.githubRepoPlaceholder')}
                    hint={t('addProject.githubRepoHint')}
                    error={errors.repo}
                    required
                  />

                  <div className="field">
                    <label htmlFor="add-project-name">
                      {t('addProject.name')}
                    </label>
                    <input
                      id="add-project-name"
                      className="text-input"
                      value={name}
                      placeholder={t('addProject.namePlaceholder')}
                      required
                      aria-invalid={errors.name ? true : undefined}
                      aria-describedby={
                        errors.name ? 'add-project-name-error' : undefined
                      }
                      onChange={(event) => {
                        setName(event.target.value);
                        setNameTouched(true);
                        setErrors((current) => ({
                          ...current,
                          name: undefined,
                        }));
                      }}
                    />
                    {errors.name ? (
                      <p
                        className="field-error"
                        id="add-project-name-error"
                        role="alert"
                      >
                        {errors.name}
                      </p>
                    ) : null}
                  </div>

                  <div className="field">
                    <label htmlFor="add-project-clone-folder">
                      {t('addProject.githubFolder')}
                    </label>
                    <div className="field-row">
                      <input
                        id="add-project-clone-folder"
                        className="text-input"
                        value={cloneFolder}
                        placeholder={t('addProject.githubFolderPlaceholder')}
                        spellCheck={false}
                        required
                        aria-invalid={errors.folder ? true : undefined}
                        aria-describedby={
                          cloneTarget
                            ? 'add-project-clone-hint'
                            : undefined
                        }
                        onChange={(event) => {
                          setCloneFolder(event.target.value);
                                          setErrors((current) => ({
                            ...current,
                            folder: undefined,
                          }));
                        }}
                      />
                      {isTauri() ? (
                        <button
                          type="button"
                          className="btn btn-ghost"
                          onClick={() => void browse('clone')}
                        >
                          {t('addProject.browse')}
                        </button>
                      ) : null}
                    </div>
                    {cloneTarget ? (
                      <p className="field-hint" id="add-project-clone-hint">
                        {t('addProject.githubFolderHint', {
                          path: cloneTarget,
                        })}
                      </p>
                    ) : null}
                    {errors.folder ? (
                      <p className="field-error" role="alert">
                        {errors.folder}
                      </p>
                    ) : null}
                  </div>
                </>
              ) : (
                <>
                  <div className="sign-in-status">
                    <p>{t('addProject.githubIntro')}</p>
                  </div>

                  {startingSignIn || device?.state === 'waiting' ? (
                    <p className="sign-in-status" role="status">
                      {device?.state === 'waiting'
                        ? t('addProject.githubWaiting')
                        : t('addProject.githubConnecting')}
                    </p>
                  ) : (
                    <button
                      type="button"
                      className="btn btn-primary sign-in-url"
                      onClick={() => void connect()}
                    >
                      {t('addProject.githubConnect')}
                    </button>
                  )}

                  {device?.state === 'waiting' ? (
                    <div className="sign-in-status">
                      <p>{t('addProject.githubCodeHint')}</p>
                      <p className="sign-in-code-value">{device.userCode}</p>
                      {device.verificationUri ? (
                        <button
                          type="button"
                          className="btn btn-ghost sign-in-url"
                          onClick={() => {
                            if (device.verificationUri) {
                              void openUrl(device.verificationUri);
                            }
                          }}
                        >
                          {t('addProject.githubOpen')}
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                </>
              )}

              {signInFailure ? (
                <div className="form-error" role="alert">
                  <AlertIcon />
                  <span>{t(signInFailure)}</span>
                </div>
              ) : null}
            </div>
          )}

          {mode === 'github' && connected ? (
            <div className="field">
              <span className="field-hint" role="note">
                {t('addProject.githubGitHint')}
              </span>
            </div>
          ) : null}

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
            disabled={submitDisabled}
          >
            {submitLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
