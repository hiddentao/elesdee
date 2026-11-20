import { useCallback, useState, type FormEvent } from 'react';
import { ApiError, listAccounts, listBranches } from '../api/client';
import type {
  Account,
  Project,
  UpdateProjectDefaultsInput,
} from '../api/types';
import { accountUsage } from '../state/usage';
import { Combobox, type ComboboxLoader, type ComboboxOption } from './Combobox';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface ProjectSettingsDialogProps {
  project: Project;
  accounts: Account[];
  onSave: (input: UpdateProjectDefaultsInput) => Promise<void>;
  onClose: () => void;
}

function accountOption(account: Account): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${account.client} · ${accountUsage(account).summary}`,
    tag: account.sample ? 'Sample' : undefined,
  };
}

function branchOption(branch: { name: string; current: boolean }): ComboboxOption {
  return {
    id: branch.name,
    label: branch.name,
    tag: branch.current ? 'Current' : undefined,
  };
}

export function ProjectSettingsDialog({
  project,
  accounts,
  onSave,
  onClose,
}: ProjectSettingsDialogProps) {
  const initialAccount = project.defaultAccountId
    ? accounts.find((account) => account.id === project.defaultAccountId) ?? null
    : null;
  const [accountId, setAccountId] = useState<string | null>(
    initialAccount?.id ?? null,
  );
  const [branch, setBranch] = useState<string | null>(
    project.defaultSourceBranch ?? null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedAccount = accountId
    ? accounts.find((account) => account.id === accountId) ?? null
    : null;

  const loadAccountOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listAccounts(query, signal);
      return list.map(accountOption);
    },
    [],
  );

  const loadBranchOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listBranches(project.folder, query, signal);
      return list.map(branchOption);
    },
    [project.folder],
  );

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    setSubmitting(true);
    setError(null);
    try {
      await onSave({
        defaultAccountId: accountId,
        defaultSourceBranch: branch,
      });
      onClose();
    } catch (failure) {
      setSubmitting(false);
      setError(
        failure instanceof ApiError
          ? failure.message
          : 'The project defaults were not saved. Try again.',
      );
    }
  }

  return (
    <Dialog
      titleId="project-settings-title"
      descriptionId="project-settings-description"
      busyLabel="Saving project defaults…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="project-settings-title">
          Project settings
        </h2>
        <p className="dialog-sub" id="project-settings-description">
          {project.name} · {project.folder}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="project-default-account"
            label="Default account"
            value={accountId}
            loadOptions={loadAccountOptions}
            selectedOption={
              selectedAccount ? accountOption(selectedAccount) : null
            }
            onChange={setAccountId}
            placeholder="No default account"
            hint="New sessions in this project start with this account."
            autoFocus
          />
          {accountId ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setAccountId(null)}
            >
              Clear default account
            </button>
          ) : null}

          <Combobox
            key={project.folder}
            id="project-default-branch"
            label="Default source branch"
            value={branch}
            loadOptions={loadBranchOptions}
            selectedOption={
              branch ? { id: branch, label: branch } : null
            }
            onChange={setBranch}
            placeholder="No default branch"
            hint="New sessions start from this commit. Choose a branch in the project folder."
          />
          {branch ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setBranch(null)}
            >
              Clear default branch
            </button>
          ) : null}

          {error ? (
            <div className="form-error" role="alert">
              <AlertIcon />
              <span>{error}</span>
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
            {submitting ? 'Saving…' : 'Save defaults'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
