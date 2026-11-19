import { useState } from 'react';
import { ApiError } from '../api/client';
import type { Session } from '../api/types';
import { Dialog } from './Dialog';
import { AlertIcon } from './icons';

interface CloseSessionDialogProps {
  session: Session;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function CloseSessionDialog({
  session,
  onConfirm,
  onClose,
}: CloseSessionDialogProps) {
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
          : 'The session was not closed. Try again.',
      );
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      titleId="close-session-title"
      descriptionId="close-session-description"
      busyLabel="Closing session…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="close-session-title">
          Close session?
        </h2>
        <p className="dialog-sub" id="close-session-description">
          {session.name} ·{' '}
          {session.kind === 'terminal' ? 'Terminal session' : 'Agent session'}
          {session.branch ? ` · ${session.branch}` : ''}
        </p>
      </div>

      <div className="dialog-body close-body">
        {session.kind === 'terminal' ? (
          <p className="close-copy">
            The terminal stops. The folder stays as it is.
          </p>
        ) : (
          <p className="close-copy">
            Closing commits the agent's work and, when the repository has a
            remote, pushes the branch. The worktree is removed when that
            succeeds. If it fails, the worktree stays and the reason appears in
            the session preview. The branch stays.
          </p>
        )}

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
          {submitting ? 'Closing…' : 'Close session'}
        </button>
      </div>
    </Dialog>
  );
}
