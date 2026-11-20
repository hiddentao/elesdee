import { lazy, Suspense } from 'react';
import type {
  PermissionDecision,
  Question,
  Session,
  SessionRequest,
} from '../api/types';
import { waitingForDetail } from '../state/sessionStatus';
import { useLanguage } from '../i18n/LanguageProvider';
import { AlertIcon, ChevronRightIcon } from './icons';
import { PermissionCard } from './PermissionCard';
import { QuestionForm } from './QuestionForm';
import { SessionRequestForm } from './SessionRequestForm';

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
  question?: Question | null;
  onAnswerQuestion?: (answer: string) => Promise<void>;
  request?: SessionRequest | null;
  onApproveRequest?: (allowFurther: boolean) => Promise<void>;
  onDenyRequest?: () => Promise<void>;
}

export function SessionView({
  session,
  onRespondPermission,
  onReopen,
  onResume,
  question,
  onAnswerQuestion,
  request,
  onApproveRequest,
  onDenyRequest,
}: SessionViewProps) {
  const { t } = useLanguage();
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
    <section
      className="session-body"
      aria-label={t('session.bodyLabel', { name: session.name })}
    >
      {closed ? (
        <div className="closed-banner">
          <span className="closed-banner-mark" aria-hidden="true" />
          <div className="closed-banner-text">
            <strong>{t('status.closed')}</strong>
            <span>
              {t('session.logKept')}{' '}
              {session.branch
                ? t('session.branchStays', { branch: session.branch })
                : t('session.folderStays')}
            </span>
          </div>
          <button
            type="button"
            className="btn btn-ghost closed-reopen"
            onClick={onReopen}
          >
            {t('session.reopen')}
            <ChevronRightIcon />
          </button>
        </div>
      ) : request ? (
        <section className="session-question">
          <SessionRequestForm
            request={request}
            onApprove={onApproveRequest ?? (() => Promise.resolve())}
            onDeny={onDenyRequest ?? (() => Promise.resolve())}
          />
        </section>
      ) : paused ? (
        <div className="waiting-note" role="status">
          <AlertIcon />
          <div>
            <strong>{t('status.paused')}</strong>
            <span>{session.preview}</span>
          </div>
          <button type="button" className="btn btn-ghost" onClick={onResume}>
            {t('session.resume')}
          </button>
        </div>
      ) : question ? (
        <section className="question-card session-question">
          <span className="question-session">
            {t('session.questionFromAgent')}
          </span>
          <QuestionForm
            question={question}
            onAnswer={onAnswerQuestion ?? (() => Promise.resolve())}
          />
        </section>
      ) : session.waitingFor && !permission ? (
        <div className="waiting-note" role="status">
          <AlertIcon />
          <div>
            <strong>{t('status.waitingForYou')}</strong>
            <span>
              {waitingForDetail(session.waitingFor, t)}
            </span>
          </div>
        </div>
      ) : null}

      <Suspense
        fallback={
          <div className="terminal-shell">
            <div className="terminal-loading">
              {t('session.loadingTerminal')}
            </div>
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
