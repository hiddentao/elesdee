import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type {
  Account,
  Client,
  CreateSessionInput,
  Project,
  Session,
  SessionKind,
} from '../api/types';
import { CLIENTS } from '../api/types';
import { accountUsage } from '../state/usage';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';
import { Combobox, type ComboboxOption } from './Combobox';

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
  form?: string;
}

export function NewSessionDialog({
  projects,
  accounts,
  initialProjectId,
  onCreate,
  onCreated,
  onClose,
}: NewSessionDialogProps) {
  const [projectId, setProjectId] = useState<string | null>(initialProjectId ?? null);
  const [kind, setKind] = useState<SessionKind>('agent');
  const [client, setClient] = useState<Client | null>(null);
  const [accountId, setAccountId] = useState<string | null>(null);
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

  const projectOptions: ComboboxOption[] = projects.map((project) => ({
    id: project.id,
    label: project.name,
    meta: project.folder,
  }));

  const accountOptions: ComboboxOption[] = accounts
    .filter((account) => account.client === client)
    .map((account) => ({
      id: account.id,
      label: account.name,
      meta: `${accountUsage(account).summary} · ${account.plan}`,
      tag: account.sample ? 'Sample' : undefined,
    }));

  const selectedAccount = accountId
    ? accounts.find((account) => account.id === accountId) ?? null
    : null;
  const selectedUsage = selectedAccount
    ? accountUsage(selectedAccount)
    : null;

  function chooseProject(id: string | null) {
    const nextProject = projects.find((project) => project.id === id);

    setProjectId(id);
    setErrors((current) => ({ ...current, project: undefined }));

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

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    const projectIsKnown = projectId
      ? projects.some((project) => project.id === projectId)
      : false;

    if (!projectIsKnown) {
      nextErrors.project = 'Choose a project from the list.';
    }

    if (kind === 'agent') {
      if (!client) {
        nextErrors.client = 'Choose a client.';
        nextErrors.account = 'Choose a client and an account.';
      } else if (!accountId) {
        nextErrors.account = 'Choose an account.';
      }
    }

    if (folder.trim() === '') {
      nextErrors.folder = 'Enter a folder.';
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
            : 'The session was not created. Try again.',
      });
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="new-session-title"
      descriptionId="new-session-description"
      busyLabel="Starting session…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="new-session-title">
          New session
        </h2>
        <p className="dialog-sub" id="new-session-description">
          Run an agent or a terminal in one of your projects.
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="new-session-project"
            label="Project"
            value={projectId}
            options={projectOptions}
            onChange={chooseProject}
            placeholder="Type to find a project"
            error={errors.project}
            required
            autoFocus
          />

          <fieldset className="field">
            <legend>Session type</legend>
            <div className="segmented">
              <label className="segment">
                <input
                  type="radio"
                  name="session-kind"
                  value="agent"
                  checked={kind === 'agent'}
                  onChange={() => setKind('agent')}
                />
                <span>Agent</span>
              </label>
              <label className="segment">
                <input
                  type="radio"
                  name="session-kind"
                  value="terminal"
                  checked={kind === 'terminal'}
                  onChange={() => setKind('terminal')}
                />
                <span>Terminal</span>
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
                <legend>Client</legend>
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
                label="Account"
                value={accountId}
                options={accountOptions}
                onChange={chooseAccount}
                placeholder={client ? 'Type to find an account' : 'Choose a client first'}
                hint={
                  client && accountOptions.length === 0
                    ? `No account is set up for ${client} yet.`
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
                    <span className="sample-tag">Sample</span>
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
              {kind === 'agent' ? 'Repository folder' : 'Folder'}
            </label>
            <input
              id="new-session-folder"
              className="text-input"
              value={folder}
              placeholder="Type a folder path"
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
                setErrors((current) => ({ ...current, folder: undefined }));
              }}
            />
            {kind === 'agent' ? (
              <p className="field-hint" id="new-session-folder-hint">
                The folder must be inside a Git repository.
              </p>
            ) : null}
            {errors.folder ? (
              <p className="field-error" id="new-session-folder-error" role="alert">
                {errors.folder}
              </p>
            ) : null}
          </div>

          <div className="field">
            <label htmlFor="new-session-name">Session name (optional)</label>
            <input
              id="new-session-name"
              className="text-input"
              value={name}
              placeholder="Uses the first prompt or folder when empty"
              onChange={(event) => setName(event.target.value)}
            />
          </div>

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
            Cancel
          </button>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? 'Starting…' : 'Start session'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
