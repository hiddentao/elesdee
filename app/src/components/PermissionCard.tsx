import { useId, useState } from 'react';
import type {
  PendingPermission,
  PermissionDecision,
} from '../api/types';
import { AlertIcon } from './icons';

interface PermissionCardProps {
  permission: PendingPermission;
  client: string | null;
  sessionName: string;
  onRespond: (decision: PermissionDecision) => Promise<void>;
}

export function PermissionCard({
  permission,
  client,
  sessionName,
  onRespond,
}: PermissionCardProps) {
  const titleId = useId();
  const [decision, setDecision] = useState<PermissionDecision | null>(null);
  const [error, setError] = useState('');

  async function answer(choice: PermissionDecision) {
    if (decision) {
      return;
    }

    setDecision(choice);
    setError('');

    try {
      await onRespond(choice);
    } catch {
      setDecision(null);
      setError('The answer was not sent. Try again.');
    }
  }

  return (
    <section
      className="permission-card"
      aria-labelledby={titleId}
      aria-busy={decision !== null}
    >
      <div className="permission-head">
        <span className="permission-icon" aria-hidden="true">
          <AlertIcon />
        </span>
        <div className="permission-head-text">
          <h2 className="permission-title" id={titleId}>
            Permission needed
          </h2>
          <span className="permission-context">
            {client ?? 'Agent'} · {sessionName}
          </span>
        </div>
      </div>

      <div className="permission-copy" role="status">
        <p className="permission-tool">
          <span className="permission-label">Tool</span> {permission.tool}
        </p>
        <pre className="permission-summary">{permission.summary}</pre>
        <p className="permission-rationale">
          The agent is paused until you answer.
        </p>
      </div>

      <div className="permission-actions">
        <button
          type="button"
          className="btn btn-ghost"
          disabled={decision !== null}
          onClick={() => void answer('deny')}
        >
          {decision === 'deny' ? 'Denying…' : 'Deny'}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          disabled={decision !== null}
          onClick={() => void answer('allow')}
        >
          {decision === 'allow' ? 'Allowing…' : 'Allow'}
        </button>
      </div>

      {error ? (
        <p className="permission-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
