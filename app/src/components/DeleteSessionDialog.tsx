import { useState } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface DeleteSessionDialogProps {
  session: Session;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function DeleteSessionDialog({
  session,
  onConfirm,
  onClose,
}: DeleteSessionDialogProps) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
    } catch (reason) {
      setError(
        reason instanceof ApiError
          ? reason.message
          : 'The session was not deleted. Try again.',
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="delete-session-title"
      descriptionId="delete-session-description"
      busyLabel="Deleting session…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="delete-session-title">
          Delete session?
        </h2>
        <p className="dialog-sub" id="delete-session-description">
          {session.name}
        </p>
      </div>

      <div className="dialog-body close-body">
        <p className="close-copy">
          Delete the log? The session is removed from the app. The branch stays.
        </p>

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
          autoFocus
        >
          Cancel
        </button>
        <button
          type="button"
          className="btn btn-danger"
          onClick={() => void confirm()}
          disabled={submitting}
        >
          {submitting ? 'Deleting…' : 'Delete'}
        </button>
      </div>
    </Dialog>
  );
}
