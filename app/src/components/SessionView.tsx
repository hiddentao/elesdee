import { lazy, Suspense } from 'react';
import type { PermissionDecision, Session } from '../api/types';
import { waitingForDetail } from '../state/sessionStatus';
import { AlertIcon, ChevronRightIcon } from './icons';
import { PermissionCard } from './PermissionCard';

const TerminalPane = lazy(() =>
  import('./TerminalPane').then((module) => ({
    default: module.TerminalPane,
  })),
);

interface SessionViewProps {
  session: Session;
  onRespondPermission: (decision: PermissionDecision) => Promise<void>;
  onReopen: () => void;
  onResume: () => void;
}

export function SessionView({
  session,
  onRespondPermission,
  onReopen,
  onResume,
}: SessionViewProps) {
  const closed = session.status === 'closed';
  const paused =
    session.status === 'active' &&
    session.kind === 'agent' &&
    session.workState === 'paused';
  const permission =
    session.status === 'active' && session.kind === 'agent'
      ? session.pendingPermission ?? null
      : null;

  return (
    <section className="session-body" aria-label={`${session.name} session`}>
      {closed ? (
        <div className="closed-banner">
          <span className="closed-banner-mark" aria-hidden="true" />
          <div className="closed-banner-text">
            <strong>Closed</strong>
            <span>
              The session log is kept.
              {session.branch
                ? ` The branch ${session.branch} stays.`
                : ' The folder stays as it is.'}
            </span>
          </div>
          <button
            type="button"
            className="btn btn-ghost closed-reopen"
            onClick={onReopen}
          >
            Reopen
            <ChevronRightIcon />
          </button>
        </div>
      ) : paused ? (
        <div className="waiting-note" role="status">
          <AlertIcon />
          <div>
            <strong>Paused</strong>
            <span>{session.preview}</span>
          </div>
          <button type="button" className="btn btn-ghost" onClick={onResume}>
            Resume
          </button>
        </div>
      ) : session.waitingFor && !permission ? (
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

      {permission ? (
        <PermissionCard
          key={permission.requestId}
          permission={permission}
          client={session.client}
          sessionName={session.name}
          onRespond={onRespondPermission}
        />
      ) : null}
    </section>
  );
}
