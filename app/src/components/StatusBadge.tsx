import type { Session } from '../api/types';
import { statusText, statusTone, workLabel } from '../state/sessionStatus';

interface StatusBadgeProps {
  session: Session;
}

export function StatusBadge({ session }: StatusBadgeProps) {
  const detail = workLabel(session);
  const closed = session.status === 'closed';

  return (
    <span className="status-badge" data-status={statusTone(session)} title={statusText(session)}>
      <span className="status-dot" aria-hidden="true" />
      <span>{closed ? 'Closed' : 'Active'}</span>
      {detail ? <span className="status-detail">· {detail}</span> : null}
    </span>
  );
}
