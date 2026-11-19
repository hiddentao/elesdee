import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type { Account, CreateAccountInput } from '../api/types';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

const DEFAULT_MODEL = 'gemini-3.8-flash-medium';

interface AddAccountDialogProps {
  onCreate: (input: CreateAccountInput) => Promise<Account>;
  onCreated: (account: Account) => void;
  onClose: () => void;
}

interface FormErrors {
  name?: string;
  key?: string;
  form?: string;
}

export function AddAccountDialog({
  onCreate,
  onCreated,
  onClose,
}: AddAccountDialogProps) {
  const [name, setName] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [model, setModel] = useState(DEFAULT_MODEL);
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

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const nextErrors: FormErrors = {};
    if (name.trim() === '') {
      nextErrors.name = 'Enter an account name.';
    }
    if (apiKey.trim() === '') {
      nextErrors.key = 'Enter an API key.';
    }

    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      setFocusTarget(nextErrors.name ? 'add-account-name' : 'add-account-key');
      return;
    }

    const input: CreateAccountInput = {
      name: name.trim(),
      providerId: 'google',
      client: 'Antigravity',
      auth: 'key',
      key: apiKey.trim(),
    };

    const trimmedModel = model.trim();
    if (trimmedModel) {
      input.model = trimmedModel;
    }

    setErrors({});
    setSubmitting(true);

    try {
      const account = await onCreate(input);
      onCreated(account);
    } catch (error) {
      setErrors({
        form:
          error instanceof ApiError
            ? error.message
            : 'The account was not added. Try again.',
      });
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="add-account-title"
      descriptionId="add-account-description"
      busyLabel="Adding account…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="add-account-title">
          Add account
        </h2>
        <p className="dialog-sub" id="add-account-description">
          Connect a Google account for Antigravity.
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <dl className="account-fixed">
            <div>
              <dt>Provider</dt>
              <dd>Google</dd>
            </div>
            <div>
              <dt>Client</dt>
              <dd>Antigravity</dd>
            </div>
          </dl>

          <div className="field">
            <label htmlFor="add-account-name">Account name</label>
            <input
              id="add-account-name"
              className="text-input"
              value={name}
              autoFocus
              required
              aria-invalid={errors.name ? true : undefined}
              aria-describedby={
                errors.name ? 'add-account-name-error' : undefined
              }
              onChange={(event) => {
                setName(event.target.value);
                setErrors((current) => ({ ...current, name: undefined }));
              }}
            />
            {errors.name ? (
              <p className="field-error" id="add-account-name-error" role="alert">
                {errors.name}
              </p>
            ) : null}
          </div>

          <div className="field">
            <label htmlFor="add-account-key">API key</label>
            <input
              id="add-account-key"
              className="text-input"
              type="password"
              value={apiKey}
              autoComplete="off"
              required
              aria-invalid={errors.key ? true : undefined}
              aria-describedby={
                errors.key ? 'add-account-key-error' : undefined
              }
              onChange={(event) => {
                setApiKey(event.target.value);
                setErrors((current) => ({ ...current, key: undefined }));
              }}
            />
            {errors.key ? (
              <p className="field-error" id="add-account-key-error" role="alert">
                {errors.key}
              </p>
            ) : null}
          </div>

          <div className="field">
            <label htmlFor="add-account-model">Model</label>
            <input
              id="add-account-model"
              className="text-input"
              value={model}
              spellCheck={false}
              onChange={(event) => setModel(event.target.value)}
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
            {submitting ? 'Adding…' : 'Add account'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
