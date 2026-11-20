import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  answerQuestion,
  approveSessionRequest,
  bootstrap,
  denySessionRequest,
  respondPermission,
} from '../api/client';
import { subscribeAux, subscribeEvents } from '../api/events';
import type {
  PermissionDecision,
  Question,
  Session,
  SessionRequest,
} from '../api/types';
import { QuestionForm } from '../components/QuestionForm';
import { SessionRequestForm } from '../components/SessionRequestForm';
import { TerminalPane } from '../components/TerminalPane';
import { workLabel } from '../state/sessionStatus';

type Filter = 'all' | 'running' | 'waiting';

function isWaiting(session: Session): boolean {
  return session.waitingFor !== null;
}

function sessionHashId(): string | null {
  const match = window.location.hash.match(/^#session=([^&]+)$/);
  return match ? decodeURIComponent(match[1]) : null;
}

export default function PhoneApp() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [requests, setRequests] = useState<SessionRequest[]>([]);
  const [activeId, setActiveId] = useState<string | null>(sessionHashId);
  const [screen, setScreen] = useState<'list' | 'session'>(
    sessionHashId() ? 'session' : 'list',
  );
  const [filter, setFilter] = useState<Filter>('all');
  const [sheetOpen, setSheetOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const tabStripRef = useRef<HTMLDivElement>(null);
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  const load = useCallback(async () => {
    try {
      const data = await bootstrap();
      setSessions(data.sessions);
      setQuestions(data.questions ?? []);
      setRequests(data.sessionRequests ?? []);
      setError('');
    } catch {
      setError('Cannot reach this computer.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(
    () =>
      subscribeEvents((session) => {
        setSessions((current) => {
          const index = current.findIndex((item) => item.id === session.id);
          if (index === -1) return [...current, session];
          const next = current.slice();
          next[index] = session;
          return next;
        });
      }, undefined, () => void load()),
    [load],
  );

  useEffect(
    () =>
      subscribeAux((event) => {
        if (event.type === 'question.updated') {
          setQuestions((current) => {
            const others = current.filter(
              (question) => question.id !== event.question.id,
            );
            return event.question.status === 'pending'
              ? [...others, event.question]
              : others;
          });
        } else if (event.type === 'request.updated') {
          setRequests((current) => {
            const others = current.filter(
              (request) => request.id !== event.request.id,
            );
            return event.request.status === 'pending'
              ? [...others, event.request]
              : others;
          });
        }
      }, undefined, () => void load()),
    [load],
  );

  const openSessions = useMemo(
    () => sessions.filter((session) => session.status === 'active'),
    [sessions],
  );
  const active = openSessions.find((session) => session.id === activeId) ?? null;
  const activeIndex = active
    ? openSessions.findIndex((session) => session.id === active.id)
    : -1;

  useEffect(() => {
    if (active) {
      return;
    }
    const fromHash = sessionHashId();
    const target =
      openSessions.find((session) => session.id === fromHash) ??
      openSessions[0];
    if (target && target.id !== activeId) {
      setActiveId(target.id);
    }
  }, [active, openSessions, activeId]);

  useEffect(() => {
    if (activeIndex < 0 || !tabStripRef.current) {
      return;
    }
    tabStripRef.current.scrollTo({
      left: tabStripRef.current.clientWidth * activeIndex,
      behavior: 'instant',
    });
  }, [activeIndex]);

  function selectSession(id: string) {
    setActiveId(id);
    setScreen('session');
    setSheetOpen(false);
    window.history.replaceState(null, '', `#session=${encodeURIComponent(id)}`);
  }

  function showList() {
    setScreen('list');
    window.history.replaceState(null, '', window.location.pathname);
  }

  function onTabsScroll() {
    if (!tabStripRef.current) {
      return;
    }
    const width = tabStripRef.current.clientWidth;
    if (width === 0) {
      return;
    }
    const index = Math.round(tabStripRef.current.scrollLeft / width);
    const next = openSessions[Math.max(0, Math.min(index, openSessions.length - 1))];
    if (next && next.id !== activeIdRef.current) {
      setActiveId(next.id);
      window.history.replaceState(
        null,
        '',
        `#session=${encodeURIComponent(next.id)}`,
      );
    }
  }

  async function answerPermission(
    session: Session,
    decision: PermissionDecision,
  ) {
    const pending = session.pendingPermission;
    if (!pending) return;
    await respondPermission(session.id, pending.requestId, decision);
  }

  async function approveRequest(id: string, allowFurther: boolean) {
    const result = await approveSessionRequest(id, allowFurther);
    setRequests((current) => current.filter((request) => request.id !== id));
    if (result.session) {
      const child = result.session;
      setSessions((current) =>
        current.some((session) => session.id === child.id)
          ? current
          : [...current, child],
      );
      selectSession(child.id);
    }
  }

  async function denyRequest(id: string) {
    await denySessionRequest(id);
    setRequests((current) => current.filter((request) => request.id !== id));
  }

  const visible = sessions.filter((session) => {
    if (filter === 'running') return session.status === 'active' && !isWaiting(session);
    if (filter === 'waiting') return isWaiting(session);
    return true;
  });
  const waiting = visible.filter(isWaiting);
  const rest = visible.filter((session) => !isWaiting(session));
  const activeQuestion = active
    ? questions.find((question) => question.sessionId === active.id) ?? null
    : null;
  const activeRequest = active
    ? requests.find((request) => request.parentSessionId === active.id) ?? null
    : null;
  const countLabel = openSessions.length;

  return (
    <div className="phone">
      <header className="phone-head">
        {screen === 'session' && active ? (
          <>
            <button
              type="button"
              className="phone-back"
              aria-label="Back to sessions"
              onClick={showList}
            >
              ‹
            </button>
            <div className="phone-head-text">
              <span className="phone-title">{active.name}</span>
              <span className="phone-sub">
                {workLabel(active) ?? 'Active'} · {active.projectId}
              </span>
            </div>
          </>
        ) : (
          <div className="phone-head-text">
            <span className="phone-title">Sessions</span>
            <span className="phone-sub">This computer</span>
          </div>
        )}
      </header>

      {loading ? (
        <p className="phone-empty">Loading…</p>
      ) : error ? (
        <p className="phone-empty" role="alert">
          {error}
        </p>
      ) : screen === 'list' ? (
        <main className="phone-list">
          <div className="phone-filters" role="group" aria-label="Session filter">
            {(['all', 'running', 'waiting'] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={filter === value ? 'chip is-on' : 'chip'}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {value === 'all' ? 'All' : value === 'running' ? 'Running' : 'Waiting'}
              </button>
            ))}
          </div>

          {waiting.length > 0 ? (
            <section>
              <h2 className="phone-section-title">Waiting for you</h2>
              {waiting.map((session) => (
                <SessionRow
                  key={session.id}
                  session={session}
                  onSelect={selectSession}
                />
              ))}
            </section>
          ) : null}

          <section>
            {waiting.length > 0 ? (
              <h2 className="phone-section-title">Everything else</h2>
            ) : null}
            {rest.length === 0 && waiting.length === 0 ? (
              <p className="phone-empty">No sessions yet.</p>
            ) : (
              rest.map((session) => (
                <SessionRow
                  key={session.id}
                  session={session}
                  onSelect={selectSession}
                  active={session.id === activeId}
                />
              ))
            )}
          </section>
        </main>
      ) : active ? (
        <main className="phone-session">
          <div className="phone-tabbar">
            <div
              className="phone-tabs"
              ref={tabStripRef}
              onScroll={onTabsScroll}
            >
              {openSessions.map((session) => (
                <button
                  key={session.id}
                  type="button"
                  className={
                    session.id === activeId ? 'phone-tab is-on' : 'phone-tab'
                  }
                  onClick={() => selectSession(session.id)}
                >
                  <span className="phone-tab-name">{session.name}</span>
                  <span className="phone-tab-meta">
                    {isWaiting(session) ? 'Waiting' : workLabel(session) ?? 'Active'}
                  </span>
                </button>
              ))}
            </div>
            <button
              type="button"
              className="phone-tab-count"
              aria-label={`All tabs, ${countLabel}`}
              aria-expanded={sheetOpen}
              onClick={() => setSheetOpen((open) => !open)}
            >
              {countLabel}
            </button>
          </div>

          {sheetOpen ? (
            <div className="phone-sheet-layer">
              <button
                type="button"
                className="phone-sheet-scrim"
                aria-label="Close tabs"
                onClick={() => setSheetOpen(false)}
              />
              <div className="phone-sheet" role="dialog" aria-label="All tabs">
                <h2 className="phone-sheet-title">
                  {countLabel} tabs
                </h2>
                {openSessions.some(isWaiting) ? (
                  <>
                    <h3 className="phone-section-title">Waiting for you</h3>
                    {openSessions.filter(isWaiting).map((session) => (
                      <SheetRow
                        key={session.id}
                        session={session}
                        active={session.id === activeId}
                        onSelect={selectSession}
                      />
                    ))}
                  </>
                ) : null}
                <h3 className="phone-section-title">Everything else</h3>
                {openSessions
                  .filter((session) => !isWaiting(session))
                  .map((session) => (
                    <SheetRow
                      key={session.id}
                      session={session}
                      active={session.id === activeId}
                      onSelect={selectSession}
                    />
                  ))}
              </div>
            </div>
          ) : null}

          {active.pendingPermission ? (
            <section className="phone-card">
              <span className="phone-card-label">Permission needed</span>
              <p className="permission-tool">
                <span className="permission-label">Tool</span>{' '}
                {active.pendingPermission.tool}
              </p>
              <pre className="permission-summary">
                {active.pendingPermission.summary}
              </pre>
              <div className="permission-actions">
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => void answerPermission(active, 'deny')}
                >
                  Deny
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => void answerPermission(active, 'allow')}
                >
                  Allow
                </button>
              </div>
            </section>
          ) : activeRequest ? (
            <section className="phone-card">
              <SessionRequestForm
                request={activeRequest}
                onApprove={(allowFurther) =>
                  approveRequest(activeRequest.id, allowFurther)
                }
                onDeny={() => denyRequest(activeRequest.id)}
              />
            </section>
          ) : activeQuestion ? (
            <section className="phone-card">
              <span className="phone-card-label">Question</span>
              <QuestionForm
                question={activeQuestion}
                onAnswer={async (answer) => {
                  await answerQuestion(activeQuestion.id, answer);
                  setQuestions((current) =>
                    current.filter((item) => item.id !== activeQuestion.id),
                  );
                }}
              />
            </section>
          ) : null}

          <div className="phone-terminal">
            <TerminalPane session={active} readOnly />
          </div>
        </main>
      ) : (
        <p className="phone-empty">Choose a session.</p>
      )}
    </div>
  );
}

function SessionRow({
  session,
  active,
  onSelect,
}: {
  session: Session;
  active?: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      className={active ? 'phone-row is-on' : 'phone-row'}
      onClick={() => onSelect(session.id)}
    >
      <span className="phone-row-main">
        <span className="phone-row-name">{session.name}</span>
        <span className="phone-row-preview">
          {isWaiting(session) ? 'Waiting for you' : session.preview || session.projectId}
        </span>
      </span>
      <span className="phone-row-state">
        {session.status === 'closed' ? 'Closed' : workLabel(session) ?? 'Active'}
      </span>
    </button>
  );
}

function SheetRow({
  session,
  active,
  onSelect,
}: {
  session: Session;
  active: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      className={active ? 'phone-sheet-row is-on' : 'phone-sheet-row'}
      onClick={() => onSelect(session.id)}
    >
      <span className="phone-sheet-row-name">{session.name}</span>
      <span className="phone-sheet-row-meta">
        {active ? 'Open' : isWaiting(session) ? 'Waiting' : workLabel(session) ?? 'Active'}
      </span>
    </button>
  );
}
