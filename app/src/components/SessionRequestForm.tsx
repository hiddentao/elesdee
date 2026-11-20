import { useState } from 'react';
import type { SessionRequest } from '../api/types';

interface SessionRequestFormProps {
  request: SessionRequest;
  onApprove: (allowFurther: boolean) => Promise<void>;
  onDeny: () => Promise<void>;
}

export function SessionRequestForm({
  request,
  onApprove,
  onDeny,
}: SessionRequestFormProps) {
  const [allowFurther, setAllowFurther] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch {
      setBusy(false);
      setError('The request was not answered. Try again.');
    }
  }

  return (
    <div className="request-card">
      <p className="request-text">
        <strong>{request.parentSessionName}</strong> wants to start a session in{' '}
        <strong>{request.projectName}</strong>.
      </p>
      <dl className="request-details">
        <div>
          <dt>Client</dt>
          <dd>{request.client}</dd>
        </div>
        {request.name ? (
          <div>
            <dt>Name</dt>
            <dd>{request.name}</dd>
          </div>
        ) : null}
        {request.firstPrompt ? (
          <div>
            <dt>First prompt</dt>
            <dd>{request.firstPrompt}</dd>
          </div>
        ) : null}
        {request.sourceBranch ? (
          <div>
            <dt>Source branch</dt>
            <dd>{request.sourceBranch}</dd>
          </div>
        ) : null}
      </dl>
      <label className="request-allow">
        <input
          type="checkbox"
          checked={allowFurther}
          disabled={busy}
          onChange={(event) => setAllowFurther(event.target.checked)}
        />
        Allow further sessions from this session
      </label>
      <div className="request-actions">
        <button
          type="button"
          className="btn btn-ghost"
          disabled={busy}
          onClick={() => void run(onDeny)}
        >
          Deny
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => void run(() => onApprove(allowFurther))}
        >
          {busy ? 'Starting…' : 'Approve'}
        </button>
      </div>
      {error ? (
        <p className="field-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
