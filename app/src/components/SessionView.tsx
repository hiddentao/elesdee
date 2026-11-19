import { lazy, Suspense } from 'react';
import type { PermissionDecision, Session } from '../api/types';
import { waitingForDetail } from '../state/sessionStatus';
import { AlertIcon } from './icons';
import { PermissionCard } from './PermissionCard';

const TerminalPane = lazy(() =>
  import('./TerminalPane').then((module) => ({
    default: module.TerminalPane,
  })),
);

interface SessionViewProps {
  session: Session;
  onRespondPermission: (decision: PermissionDecision) => Promise<void>;
}

export function SessionView({
  session,
  onRespondPermission,
}: SessionViewProps) {
  const closed = session.status === 'closed';
  const permission =
    session.status === 'active' && session.kind === 'agent'
      ? session.pendingPermission ?? null
      : null;

  return (
    <section className="session-body" aria-label={`${session.name} session`}>
      {permission ? (
        <PermissionCard
          key={permission.requestId}
          permission={permission}
          onRespond={onRespondPermission}
        />
      ) : session.waitingFor && !closed ? (
        <div className="waiting-note" role="status">
          <AlertIcon />
          <div>
            <strong>Waiting for you</strong>
            <span>{waitingForDetail(session.waitingFor)}</span>
          </div>
        </div>
      ) : null}

      <Suspense
        fallback={
          <div className="terminal-shell">
            <div className="terminal-loading">Loading terminal…</div>
          </div>
        }
      >
        <TerminalPane session={session} />
      </Suspense>
    </section>
  );
}
