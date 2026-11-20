import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react';
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
import { GlobeIcon } from '../components/icons';
import { LANGUAGES, formatNumber, type Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
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
  const { t, language, setLanguage } = useLanguage();
  const tRef = useRef(t);
  tRef.current = t;
  const [sessions, setSessions] = useState<Session[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [requests, setRequests] = useState<SessionRequest[]>([]);
  const [activeId, setActiveId] = useState<string | null>(sessionHashId);
  const [screen, setScreen] = useState<'list' | 'session'>(
    sessionHashId() ? 'session' : 'list',
  );
  const [filter, setFilter] = useState<Filter>('all');
  const [sheetOpen, setSheetOpen] = useState(false);
  const [dismissedTabs, setDismissedTabs] = useState<Set<string>>(new Set());
  const [languageSheetOpen, setLanguageSheetOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const tabStripRef = useRef<HTMLDivElement>(null);
  const languageButtonRef = useRef<HTMLButtonElement>(null);
  const languageSheetRef = useRef<HTMLDivElement>(null);
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
      setError(tRef.current('phone.cannotReach'));
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
    () =>
      sessions.filter(
        (session) =>
          session.status === 'active' && !dismissedTabs.has(session.id),
      ),
    [sessions, dismissedTabs],
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

  useEffect(() => {
    if (!languageSheetOpen) {
      return;
    }
    const dialog = languageSheetRef.current;
    const current = dialog?.querySelector<HTMLButtonElement>(
      '[aria-pressed="true"]',
    );
    (current ?? dialog)?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault();
        setLanguageSheetOpen(false);
        languageButtonRef.current?.focus();
        return;
      }
      if (event.key !== 'Tab' || !dialog) {
        return;
      }
      const items = Array.from(
        dialog.querySelectorAll<HTMLElement>('button:not([disabled])'),
      );
      if (items.length === 0) {
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [languageSheetOpen]);

  function closeLanguageSheet() {
    setLanguageSheetOpen(false);
    languageButtonRef.current?.focus();
  }

  function selectSession(id: string) {
    setDismissedTabs((current) => {
      if (!current.has(id)) {
        return current;
      }
      const next = new Set(current);
      next.delete(id);
      return next;
    });
    setActiveId(id);
    setScreen('session');
    setSheetOpen(false);
    window.history.replaceState(null, '', `#session=${encodeURIComponent(id)}`);
  }

  function closeTab(id: string) {
    setDismissedTabs((current) => new Set(current).add(id));
    setSheetOpen(false);
    if (id !== activeId) {
      return;
    }
    const next = openSessions.find((session) => session.id !== id);
    if (next) {
      setActiveId(next.id);
      window.history.replaceState(
        null,
        '',
        `#session=${encodeURIComponent(next.id)}`,
      );
      return;
    }
    setActiveId(null);
    setScreen('list');
    window.history.replaceState(null, '', window.location.pathname);
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
              aria-label={t('phone.back')}
              onClick={showList}
            >
              ‹
            </button>
            <div className="phone-head-text">
              <span className="phone-title">{active.name}</span>
              <span className="phone-sub">
                {workLabel(active, t) ?? t('status.active')} ·{' '}
                {active.projectId}
              </span>
            </div>
          </>
        ) : (
          <div className="phone-head-text">
            <span className="phone-title">{t('phone.sessions')}</span>
            <span className="phone-sub">{t('phone.thisComputer')}</span>
          </div>
        )}
        <button
          type="button"
          className="phone-icon"
          ref={languageButtonRef}
          aria-label={t('settings.languages')}
          aria-haspopup="dialog"
          aria-expanded={languageSheetOpen}
          onClick={() => setLanguageSheetOpen(true)}
        >
          <GlobeIcon />
        </button>
      </header>

      {loading ? (
        <p className="phone-empty">{t('phone.loading')}</p>
      ) : error ? (
        <p className="phone-empty" role="alert">
          {error}
        </p>
      ) : screen === 'list' ? (
        <main className="phone-list">
          <div
            className="phone-filters"
            role="group"
            aria-label={t('phone.filter')}
          >
            {(['all', 'running', 'waiting'] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={filter === value ? 'chip is-on' : 'chip'}
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
              >
                {value === 'all'
                  ? t('phone.all')
                  : value === 'running'
                    ? t('phone.running')
                    : t('status.waiting')}
              </button>
            ))}
          </div>

          {waiting.length > 0 ? (
            <section>
              <h2 className="phone-section-title">
                {t('status.waitingForYou')}
              </h2>
              {waiting.map((session) => (
                <SessionRow
                  key={session.id}
                  session={session}
                  onSelect={selectSession}
                  t={t}
                />
              ))}
            </section>
          ) : null}

          <section>
            {waiting.length > 0 ? (
              <h2 className="phone-section-title">
                {t('phone.everythingElse')}
              </h2>
            ) : null}
            {rest.length === 0 && waiting.length === 0 ? (
              <p className="phone-empty">{t('phone.noSessions')}</p>
            ) : (
              rest.map((session) => (
                <SessionRow
                  key={session.id}
                  session={session}
                  onSelect={selectSession}
                  active={session.id === activeId}
                  t={t}
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
                    {isWaiting(session)
                      ? t('status.waiting')
                      : workLabel(session, t) ?? t('status.active')}
                  </span>
                </button>
              ))}
            </div>
            <button
              type="button"
              className="phone-tab-count"
              aria-label={t('phone.allTabsLabel', {
                count: formatNumber(language, countLabel),
              })}
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
                aria-label={t('phone.closeTabs')}
                onClick={() => setSheetOpen(false)}
              />
              <div
                className="phone-sheet"
                role="dialog"
                aria-label={t('phone.sheetAllTabs')}
              >
                <h2 className="phone-sheet-title">
                  {t(
                    countLabel === 1
                      ? 'phone.sheetTitleOne'
                      : 'phone.sheetTitleMany',
                    { count: formatNumber(language, countLabel) },
                  )}
                </h2>
                {openSessions.some(isWaiting) ? (
                  <>
                    <h3 className="phone-section-title">
                      {t('status.waitingForYou')}
                    </h3>
                    {openSessions.filter(isWaiting).map((session) => (
                      <SheetRow
                        key={session.id}
                        session={session}
                        active={session.id === activeId}
                        onSelect={selectSession}
                        onClose={closeTab}
                        t={t}
                      />
                    ))}
                  </>
                ) : null}
                <h3 className="phone-section-title">
                  {t('phone.everythingElse')}
                </h3>
                {openSessions
                  .filter((session) => !isWaiting(session))
                  .map((session) => (
                    <SheetRow
                      key={session.id}
                      session={session}
                      active={session.id === activeId}
                      onSelect={selectSession}
                      onClose={closeTab}
                      t={t}
                    />
                  ))}
              </div>
            </div>
          ) : null}

          {active.pendingPermission ? (
            <section className="phone-card">
              <span className="phone-card-label">
                {t('permission.needed')}
              </span>
              <p className="permission-tool">
                <span className="permission-label">
                  {t('permission.tool')}
                </span>{' '}
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
                  {t('permission.deny')}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => void answerPermission(active, 'allow')}
                >
                  {t('permission.allow')}
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
              <span className="phone-card-label">
                {t('phone.question')}
              </span>
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
        <p className="phone-empty">{t('phone.chooseSession')}</p>
      )}

      {languageSheetOpen ? (
        <div className="phone-sheet-layer">
          <button
            type="button"
            className="phone-sheet-scrim"
            aria-label={t('common.close')}
            onClick={closeLanguageSheet}
          />
          <div
            className="phone-sheet"
            role="dialog"
            aria-modal="true"
            aria-labelledby="phone-language-title"
            ref={languageSheetRef}
            tabIndex={-1}
          >
            <h2 className="phone-sheet-title" id="phone-language-title">
              {t('settings.languages')}
            </h2>
            <span className="phone-sheet-label">
              {t('settings.languageLabel')}
            </span>
            {LANGUAGES.map((item) => {
              const current = item.code === language;
              return (
                <button
                  key={item.code}
                  type="button"
                  className={
                    current ? 'phone-sheet-row is-on' : 'phone-sheet-row'
                  }
                  aria-pressed={current}
                  onClick={() => {
                    setLanguage(item.code);
                    closeLanguageSheet();
                  }}
                >
                  <span className="phone-sheet-row-name">{item.label}</span>
                  {current ? (
                    <span className="phone-language-check" aria-hidden="true">
                      ✓
                    </span>
                  ) : null}
                </button>
              );
            })}
            <p className="phone-sheet-note">{t('settings.languageNote')}</p>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function SessionRow({
  session,
  active,
  onSelect,
  t,
}: {
  session: Session;
  active?: boolean;
  onSelect: (id: string) => void;
  t: Translate;
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
          {isWaiting(session)
            ? t('status.waitingForYou')
            : session.preview || session.projectId}
        </span>
      </span>
      <span className="phone-row-state">
        {session.status === 'closed'
          ? t('status.closed')
          : workLabel(session, t) ?? t('status.active')}
      </span>
    </button>
  );
}

const SWIPE_ACTION_WIDTH = 92;

function SheetRow({
  session,
  active,
  onSelect,
  onClose,
  t,
}: {
  session: Session;
  active: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  t: Translate;
}) {
  const [offset, setOffset] = useState(0);
  const gesture = useRef<{
    startX: number;
    startY: number;
    startOffset: number;
    horizontal: boolean;
  } | null>(null);
  const suppressClick = useRef(false);

  function handlePointerDown(event: ReactPointerEvent<HTMLButtonElement>) {
    if (event.pointerType === 'mouse' && event.button !== 0) {
      return;
    }
    gesture.current = {
      startX: event.clientX,
      startY: event.clientY,
      startOffset: offset,
      horizontal: false,
    };
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLButtonElement>) {
    const current = gesture.current;
    if (!current) {
      return;
    }
    const dx = event.clientX - current.startX;
    const dy = event.clientY - current.startY;
    if (!current.horizontal) {
      if (Math.abs(dx) < 8) {
        return;
      }
      if (Math.abs(dx) <= Math.abs(dy)) {
        gesture.current = null;
        return;
      }
      current.horizontal = true;
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    setOffset(
      Math.min(
        0,
        Math.max(-SWIPE_ACTION_WIDTH, current.startOffset + dx),
      ),
    );
  }

  function handlePointerEnd(event: ReactPointerEvent<HTMLButtonElement>) {
    const current = gesture.current;
    gesture.current = null;
    if (!current?.horizontal) {
      return;
    }
    suppressClick.current = true;
    const dx = event.clientX - current.startX;
    const final = Math.min(
      0,
      Math.max(-SWIPE_ACTION_WIDTH, current.startOffset + dx),
    );
    setOffset(final <= -SWIPE_ACTION_WIDTH / 2 ? -SWIPE_ACTION_WIDTH : 0);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  function handleClick() {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    if (offset < 0) {
      setOffset(0);
      return;
    }
    onSelect(session.id);
  }

  return (
    <div className="phone-swipe-row">
      <button
        type="button"
        className="phone-swipe-close"
        onFocus={() => setOffset(-SWIPE_ACTION_WIDTH)}
        onClick={() => onClose(session.id)}
        aria-label={t('phone.closeTabLabel', { name: session.name })}
      >
        <span className="phone-swipe-x" aria-hidden="true">
          ×
        </span>
        <span>{t('common.close')}</span>
      </button>
      <button
        type="button"
        className={active ? 'phone-sheet-row is-on' : 'phone-sheet-row'}
        style={{ transform: `translateX(${offset}px)` }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerEnd}
        onPointerCancel={handlePointerEnd}
        onClick={handleClick}
      >
        <span className="phone-sheet-row-name">{session.name}</span>
        <span className="phone-sheet-row-meta">
          {active
            ? t('phone.open')
            : isWaiting(session)
              ? t('status.waiting')
              : workLabel(session, t) ?? t('status.active')}
        </span>
      </button>
    </div>
  );
}
