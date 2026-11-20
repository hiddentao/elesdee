import { useCallback, useState, type FormEvent } from 'react';
import { ApiError, listAccounts, listBranches } from '../api/client';
import type {
  Account,
  Project,
  UpdateProjectDefaultsInput,
} from '../api/types';
import { type Language } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
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

function accountOption(
  account: Account,
  language: Language,
  sampleLabel: string,
): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${account.client} · ${accountUsage(account, language).summary}`,
    tag: account.sample ? sampleLabel : undefined,
  };
}

function branchOption(
  branch: { name: string; current: boolean },
  currentLabel: string,
): ComboboxOption {
  return {
    id: branch.name,
    label: branch.name,
    tag: branch.current ? currentLabel : undefined,
  };
}

export function ProjectSettingsDialog({
  project,
  accounts,
  onSave,
  onClose,
}: ProjectSettingsDialogProps) {
  const { t, language } = useLanguage();
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
      return list.map((account) =>
        accountOption(account, language, t('common.sample')),
      );
    },
    [language, t],
  );

  const loadBranchOptions = useCallback<ComboboxLoader>(
    async (query, signal) => {
      const list = await listBranches(project.folder, query, signal);
      return list.map((branch) =>
        branchOption(branch, t('newSession.currentTag')),
      );
    },
    [project.folder, t],
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
          : t('projectSettings.errorSave'),
      );
    }
  }

  return (
    <Dialog
      titleId="project-settings-title"
      descriptionId="project-settings-description"
      busyLabel={t('projectSettings.busyLabel')}
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="project-settings-title">
          {t('projectSettings.title')}
        </h2>
        <p className="dialog-sub" id="project-settings-description">
          {project.name} · {project.folder}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="project-default-account"
            label={t('projectSettings.defaultAccount')}
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
            onChange={setAccountId}
            placeholder={t('projectSettings.noDefaultAccount')}
            hint={t('projectSettings.accountHint')}
            autoFocus
          />
          {accountId ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setAccountId(null)}
            >
              {t('projectSettings.clearAccount')}
            </button>
          ) : null}

          <Combobox
            key={project.folder}
            id="project-default-branch"
            label={t('projectSettings.defaultBranch')}
            value={branch}
            loadOptions={loadBranchOptions}
            selectedOption={
              branch ? { id: branch, label: branch } : null
            }
            onChange={setBranch}
            placeholder={t('projectSettings.noDefaultBranch')}
            hint={t('projectSettings.branchHint')}
          />
          {branch ? (
            <button
              type="button"
              className="link-button project-default-clear"
              onClick={() => setBranch(null)}
            >
              {t('projectSettings.clearBranch')}
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
            {t('common.cancel')}
          </button>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting
              ? t('projectSettings.submitting')
              : t('projectSettings.submit')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
