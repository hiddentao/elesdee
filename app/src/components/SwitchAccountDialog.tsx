import { useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type { Account, Session } from '../api/types';
import { accountUsage } from '../state/usage';
import { Combobox, type ComboboxOption } from './Combobox';
import { Dialog } from './Dialog';

interface SwitchAccountDialogProps {
  session: Session;
  accounts: Account[];
  onSwitch: (accountId: string) => Promise<void>;
  onClose: () => void;
}

function accountOption(account: Account): ComboboxOption {
  return {
    id: account.id,
    label: account.name,
    meta: `${account.client} · ${accountUsage(account).summary}`,
  };
}

export function SwitchAccountDialog({
  session,
  accounts,
  onSwitch,
  onClose,
}: SwitchAccountDialogProps) {
  const options = accounts
    .filter((account) => account.id !== session.accountId)
    .map(accountOption);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!accountId) {
      setError('Choose an account.');
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      await onSwitch(accountId);
      onClose();
    } catch (failure) {
      setSubmitting(false);
      setError(
        failure instanceof ApiError
          ? failure.message
          : 'The account was not changed. Try again.',
      );
    }
  }

  return (
    <Dialog
      titleId="switch-account-title"
      descriptionId="switch-account-description"
      busyLabel="Switching account…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="switch-account-title">
          Switch account
        </h2>
        <p className="dialog-sub" id="switch-account-description">
          {session.name}
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <Combobox
            id="switch-account"
            label="Account"
            value={accountId}
            options={options}
            onChange={(id) => {
              setAccountId(id);
              setError(null);
            }}
            placeholder={
              options.length === 0
                ? 'No other account is available'
                : 'Type to find an account'
            }
            hint="A different client starts a new conversation in the same worktree."
            error={error ?? undefined}
            required
            autoFocus
          />
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
          <button
            type="submit"
            className="btn btn-primary"
            disabled={submitting || options.length === 0}
          >
            {submitting ? 'Switching…' : 'Switch account'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
