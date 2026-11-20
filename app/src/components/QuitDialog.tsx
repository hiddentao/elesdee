import { useState } from 'react';
import { Dialog } from './Dialog';

interface QuitDialogProps {
  sessionCount: number;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}

export function QuitDialog({
  sessionCount,
  onConfirm,
  onClose,
}: QuitDialogProps) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
    } catch {
      setSubmitting(false);
      setError('elesdee could not close. Try again.');
    }
  }

  return (
    <Dialog
      titleId="quit-title"
      descriptionId="quit-description"
      busyLabel="Closing elesdee…"
      onClose={onClose}
      busy={submitting}
    >
      <div className="dialog-head">
        <h2 className="dialog-title" id="quit-title">
          Quit elesdee?
        </h2>
        <p className="dialog-sub" id="quit-description">
          {sessionCount === 1
            ? 'One session is still running.'
            : `${sessionCount} sessions are still running.`}
        </p>
      </div>
      <div className="dialog-body">
        <p className="close-copy">
          Quitting stops the running sessions. They stay in the sidebar and can
          be resumed next time.
        </p>
        {error ? (
          <div className="form-error" role="alert">
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
          className="btn btn-primary"
          onClick={() => void confirm()}
          disabled={submitting}
        >
          {submitting ? 'Quitting…' : 'Quit'}
        </button>
      </div>
    </Dialog>
  );
}
