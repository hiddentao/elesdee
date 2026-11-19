import { useState, type FormEvent } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { Dialog } from './Dialog';

interface RenameSessionDialogProps {
  session: Session;
  onRename: (name: string) => Promise<void>;
  onClose: () => void;
}

export function RenameSessionDialog({
  session,
  onRename,
  onClose,
}: RenameSessionDialogProps) {
  const [name, setName] = useState(session.name);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const trimmed = name.trim();
    if (trimmed.length === 0) {
      setError('Enter a name.');
      return;
    }
    if (trimmed.length > 64) {
      setError('Use 64 characters or fewer.');
      return;
    }

    setError(null);
    setSubmitting(true);
    try {
      await onRename(trimmed);
      onClose();
    } catch (reason) {
      setError(
        reason instanceof ApiError
          ? reason.message
          : 'The session was not renamed. Try again.',
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="rename-session-title"
      descriptionId="rename-session-description"
      busyLabel="Renaming session…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="rename-session-title">
          Rename session
        </h2>
        <p className="dialog-sub" id="rename-session-description">
          The session and its tab take the new name. The branch keeps its name.
        </p>
      </div>

      <form className="dialog-form" onSubmit={handleSubmit} noValidate>
        <div className="dialog-body">
          <div className="field">
            <label htmlFor="rename-session-name">Name</label>
            <input
              id="rename-session-name"
              className="text-input"
              value={name}
              maxLength={64}
              autoFocus
              required
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? 'rename-session-name-error' : undefined}
              onChange={(event) => {
                setName(event.target.value);
                setError(null);
              }}
            />
            {error ? (
              <p className="field-error" id="rename-session-name-error" role="alert">
                {error}
              </p>
            ) : (
              <p className="field-hint">1–64 characters.</p>
            )}
          </div>
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
            {submitting ? 'Renaming…' : 'Rename'}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
