import { useId, useState } from 'react';
import type {
  PendingPermission,
  PermissionDecision,
} from '../api/types';

interface PermissionCardProps {
  permission: PendingPermission;
  onRespond: (decision: PermissionDecision) => Promise<void>;
}

export function PermissionCard({
  permission,
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
      <div className="permission-main">
        <div className="permission-copy" role="status">
          <h2 className="permission-title" id={titleId}>
            Permission needed
          </h2>
          <p className="permission-tool">
            <span className="permission-label">Tool</span> {permission.tool}
          </p>
          <pre className="permission-summary">{permission.summary}</pre>
        </div>

        <div className="permission-actions">
          <button
            type="button"
            className="btn btn-primary"
            disabled={decision !== null}
            onClick={() => void answer('allow')}
          >
            {decision === 'allow' ? 'Allowing…' : 'Allow'}
          </button>
          <button
            type="button"
            className="btn btn-ghost"
            disabled={decision !== null}
            onClick={() => void answer('deny')}
          >
            {decision === 'deny' ? 'Denying…' : 'Deny'}
          </button>
        </div>
      </div>

      {error ? (
        <p className="permission-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
