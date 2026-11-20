import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import {
  answerQuestion,
  approveSessionRequest,
  bootstrap,
  closeSession,
  createSession,
  denySessionRequest,
  listBranches,
  respondPermission,
  sendInput,
  switchSessionConfig,
} from '../api/client';
import { subscribeAux, subscribeEvents } from '../api/events';
import type {
  AiConfig,
  PermissionDecision,
  Project,
  Provider,
  Question,
  Session,
  SessionRequest,
} from '../api/types';
import { CloseSessionDialog } from '../components/CloseSessionDialog';
import { Combobox, type ComboboxLoader } from '../components/Combobox';
import { QuestionForm } from '../components/QuestionForm';
import { SessionRequestForm } from '../components/SessionRequestForm';
import { TerminalPane } from '../components/TerminalPane';
import { UsageDialog } from '../components/UsageDialog';
import { GlobeIcon } from '../components/icons';
import { LANGUAGES, formatNumber, type MessageKey, type Translate } from '../i18n';
import { useLanguage } from '../i18n/LanguageProvider';
import { workLabel } from '../state/sessionStatus';
import {
  MAX_SAVED_KEYS,
  addSavedKey,
  encodeRailKey,
  isRepeatable,
  noModifiers,
  railKeys,
  readSavedKeys,
  savedKeyFrom,
  savedKeyLabel,
  toggleModifier,
  writeSavedKeys,
  type KeyModifiers,
  type Modifier,
  type RailKey,
  type SavedKey,
} from './keys';
import {
  buildStartInput,
  canSwitch,
  filterChoices,
  switchableConfigs,
  usageSummary,
} from './sessionActions';

type Filter = 'all' | 'running' | 'waiting';

function isWaiting(session: Session): boolean {
  return session.waitingFor !== null;
}

function sessionHashId(): string | null {
  const match = window.location.hash.match(/^#session=([^&]+)$/);
  return match ? decodeURIComponent(match[1]) : null;
}

function keyStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export default function PhoneApp() {
  const { t, language, setLanguage } = useLanguage();
  const tRef = useRef(t);
  tRef.current = t;
  const [sessions, setSessions] = useState<Session[]>([]);
  const [questions, setQuestions] = useState<Question[]>([]);
  const [requests, setRequests] = useState<SessionRequest[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [configs, setConfigs] = useState<AiConfig[]>([]);
  const [startOpen, setStartOpen] = useState(false);
  const [switchOpen, setSwitchOpen] = useState(false);
  const [closeOpen, setCloseOpen] = useState(false);
  const [usageOpen, setUsageOpen] = useState(false);
  const [keysOpen, setKeysOpen] = useState(false);
  const [railOpen, setRailOpen] = useState(true);
  const [draft, setDraft] = useState('');
  const [latches, setLatches] = useState<KeyModifiers>(noModifiers);
  const [savedKeys, setSavedKeys] = useState<SavedKey[]>(() =>
    readSavedKeys(keyStorage()),
  );
  const [actionNotice, setActionNotice] = useState('');
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
      setProjects(data.projects ?? []);
      setProviders(data.providers ?? []);
      setConfigs(data.configs ?? []);
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
        } else if (event.type === 'provider.updated') {
          setProviders((current) =>
            current.map((provider) =>
              provider.id === event.provider.id ? event.provider : provider,
            ),
          );
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

  function mergeSession(session: Session) {
    setSessions((current) => {
      const index = current.findIndex((item) => item.id === session.id);
      if (index === -1) return [...current, session];
      const next = current.slice();
      next[index] = session;
      return next;
    });
  }

  async function sendTerminal(session: Session, data: string) {
    try {
      await sendInput(session.id, data);
      setActionNotice('');
    } catch {
      setActionNotice(tRef.current('app.actionFailed'));
    }
  }

  function sendRailKey(entry: RailKey, applyLatches = true) {
    if (!active) {
      return;
    }
    const data = encodeRailKey(
      entry,
      applyLatches ? latches : noModifiers(),
    );
    setLatches(noModifiers());
    void sendTerminal(active, data);
  }

  function toggleLatch(modifier: Modifier) {
    setLatches((current) => toggleModifier(current, modifier));
  }

  function submitTerminal(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!active) {
      return;
    }
    const value = draft;
    setDraft('');
    void sendTerminal(active, `${value}\r`);
  }

  async function chooseConfig(configId: string) {
    if (!active) {
      return;
    }
    try {
      const updated = await switchSessionConfig(active.id, configId);
      mergeSession(updated);
      setSwitchOpen(false);
      setActionNotice('');
    } catch {
      setActionNotice(tRef.current('switchAccount.errorSwitch'));
    }
  }

  async function confirmClose() {
    if (!active) {
      return;
    }
    const closed = active;
    const updated = await closeSession(closed.id);
    mergeSession(updated);
    setCloseOpen(false);
    const next = openSessions.find((session) => session.id !== closed.id);
    if (next) {
      selectSession(next.id);
      return;
    }
    setActiveId(null);
    setScreen('list');
    window.history.replaceState(null, '', window.location.pathname);
  }

  function saveKey(saved: SavedKey): string | null {
    const result = addSavedKey(savedKeys, saved);
    if (result.error) {
      return tRef.current(
        result.error === 'full' ? 'phone.keyFull' : 'phone.keyDuplicate',
      );
    }
    setSavedKeys(result.keys);
    writeSavedKeys(keyStorage(), result.keys);
    return null;
  }

  function removeKey(id: string) {
    const next = savedKeys.filter((entry) => entry.id !== id);
    setSavedKeys(next);
    writeSavedKeys(keyStorage(), next);
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
        {screen === 'list' ? (
          <button
            type="button"
            className="phone-icon"
            aria-label={t('phone.startSession')}
            aria-haspopup="dialog"
            aria-expanded={startOpen}
            onClick={() => setStartOpen(true)}
          >
            <span aria-hidden="true">+</span>
          </button>
        ) : null}
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

          <div
            className="phone-session-actions"
            role="group"
            aria-label={t('phone.sessionActions')}
          >
            {canSwitch(active) ? (
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setSwitchOpen(true)}
              >
                {t('session.switchAccountMenu')}
              </button>
            ) : null}
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setUsageOpen(true)}
            >
              {t('usage.title')}
            </button>
            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => setCloseOpen(true)}
            >
              {t('session.closeMenuAction')}
            </button>
          </div>

          {actionNotice ? (
            <p className="phone-card-error" role="alert">
              {actionNotice}
            </p>
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

          <div className="phone-composer">
            {railOpen ? (
              <PhoneKeyRail
                session={active}
                savedKeys={savedKeys}
                latches={latches}
                t={t}
                onLatch={toggleLatch}
                onKey={sendRailKey}
                onAddKey={() => setKeysOpen(true)}
              />
            ) : null}
            <form className="phone-input-row" onSubmit={submitTerminal}>
              <button
                type="button"
                className="phone-icon phone-keyboard-toggle"
                aria-label={t(railOpen ? 'phone.keysHide' : 'phone.keysShow')}
                aria-pressed={railOpen}
                onClick={() => setRailOpen((open) => !open)}
              >
                <KeyboardIcon />
              </button>
              <input
                className="text-input phone-terminal-input"
                type="text"
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                placeholder={t('phone.inputPlaceholder')}
                aria-label={t('phone.terminalCommand')}
                autoComplete="off"
                autoCapitalize="sentences"
                spellCheck={false}
                enterKeyHint="send"
              />
              <button type="submit" className="btn btn-primary">
                {t('phone.send')}
              </button>
            </form>
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

      {startOpen ? (
        <StartSessionSheet
          projects={projects}
          configs={configs}
          t={t}
          onClose={() => setStartOpen(false)}
          onCreated={(session) => {
            setStartOpen(false);
            setSessions((current) =>
              current.some((entry) => entry.id === session.id)
                ? current
                : [...current, session],
            );
            selectSession(session.id);
          }}
        />
      ) : null}

      {switchOpen && active ? (
        <SwitchConfigSheet
          session={active}
          configs={configs}
          providers={providers}
          t={t}
          onChoose={(configId) => void chooseConfig(configId)}
          onClose={() => setSwitchOpen(false)}
        />
      ) : null}

      {keysOpen && active ? (
        <KeysSheet
          session={active}
          savedKeys={savedKeys}
          t={t}
          onSave={saveKey}
          onRemove={removeKey}
          onClose={() => setKeysOpen(false)}
        />
      ) : null}

      {usageOpen ? (
        <UsageDialog
          providers={providers}
          onClose={() => setUsageOpen(false)}
        />
      ) : null}

      {closeOpen && active ? (
        <CloseSessionDialog
          session={active}
          onConfirm={confirmClose}
          onClose={() => setCloseOpen(false)}
        />
      ) : null}
    </div>
  );
}

function KeyboardIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" width="20" height="20">
      <rect
        x="3"
        y="6"
        width="18"
        height="12"
        rx="2"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
      />
      <path
        d="M6.5 10h1M9.5 10h1M12.5 10h1M15.5 10h1M6.5 13h1M9.5 13h1M12.5 13h1M15.5 13h1M8 16h8"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

function keyName(entry: RailKey, t: Translate): string {
  return entry.nameKey ? t(entry.nameKey as MessageKey) : entry.name;
}

function PhoneKeyRail({
  session,
  savedKeys,
  latches,
  t,
  onLatch,
  onKey,
  onAddKey,
}: {
  session: Session;
  savedKeys: SavedKey[];
  latches: KeyModifiers;
  t: Translate;
  onLatch: (modifier: Modifier) => void;
  onKey: (entry: RailKey, applyLatches?: boolean) => void;
  onAddKey: () => void;
}) {
  const repeatDelay = useRef<ReturnType<typeof setTimeout> | null>(null);
  const repeatTimer = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(
    () => () => {
      if (repeatDelay.current) clearTimeout(repeatDelay.current);
      if (repeatTimer.current) clearInterval(repeatTimer.current);
    },
    [],
  );

  function stopRepeat() {
    if (repeatDelay.current) {
      clearTimeout(repeatDelay.current);
      repeatDelay.current = null;
    }
    if (repeatTimer.current) {
      clearInterval(repeatTimer.current);
      repeatTimer.current = null;
    }
  }

  function startRepeat(entry: RailKey) {
    stopRepeat();
    repeatDelay.current = setTimeout(() => {
      // The latch clears after the first key; held repeats are plain.
      repeatTimer.current = setInterval(() => onKey(entry, false), 80);
    }, 400);
  }

  const rail = railKeys(session.shortcuts ?? [], savedKeys);

  return (
    <div className="phone-key-area">
      <div
        className="phone-key-latches"
        role="group"
        aria-label={t('phone.keyModifiers')}
      >
        {(['shift', 'ctrl', 'alt'] as const).map((modifier) => (
          <button
            key={modifier}
            type="button"
            className={
              latches[modifier] ? 'key-chip latch is-on' : 'key-chip latch'
            }
            aria-label={t(
              modifier === 'shift'
                ? 'phone.keyShift'
                : modifier === 'ctrl'
                  ? 'phone.keyCtrl'
                  : 'phone.keyAlt',
            )}
            aria-pressed={latches[modifier]}
            onClick={() => onLatch(modifier)}
          >
            {modifier === 'shift' ? '⇧' : modifier === 'ctrl' ? '^' : '⌥'}
          </button>
        ))}
      </div>
      <div
        className="phone-key-rail"
        role="toolbar"
        aria-label={t('phone.keys')}
      >
        {rail.map((entry) => {
          const name = keyName(entry, t);
          const label = entry.description
            ? `${name}: ${entry.description}`
            : name;
          return (
            <button
              key={entry.id}
              type="button"
              className="key-chip"
              aria-label={label}
              title={entry.description ?? name}
              onPointerDown={(event) => {
                event.preventDefault();
                onKey(entry);
                if (isRepeatable(entry.key)) {
                  startRepeat(entry);
                }
              }}
              onPointerUp={stopRepeat}
              onPointerLeave={stopRepeat}
              onPointerCancel={stopRepeat}
              onClick={(event) => {
                if (event.detail === 0) {
                  onKey(entry);
                }
              }}
            >
              <span>{entry.label}</span>
              {entry.description ? (
                <span className="key-purpose" aria-hidden="true">
                  {entry.description}
                </span>
              ) : null}
            </button>
          );
        })}
        <button
          type="button"
          className="key-chip add-key"
          aria-label={t('phone.keyAdd')}
          onClick={onAddKey}
        >
          +
        </button>
      </div>
    </div>
  );
}

function KeysSheet({
  session,
  savedKeys,
  t,
  onSave,
  onRemove,
  onClose,
}: {
  session: Session;
  savedKeys: SavedKey[];
  t: Translate;
  onSave: (key: SavedKey) => string | null;
  onRemove: (id: string) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState('');
  const [modifiers, setModifiers] = useState<Modifier[]>([]);
  const [message, setMessage] = useState('');
  const full = savedKeys.length >= MAX_SAVED_KEYS;

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const saved = savedKeyFrom({ name, modifiers });
    if (!saved) {
      setMessage(t('phone.keyInvalid'));
      return;
    }
    const error = onSave(saved);
    if (error) {
      setMessage(error);
      return;
    }
    setName('');
    setModifiers([]);
    setMessage('');
  }

  return (
    <div className="phone-sheet-layer">
      <button
        type="button"
        className="phone-sheet-scrim"
        aria-label={t('common.close')}
        onClick={onClose}
      />
      <div className="phone-sheet" role="dialog" aria-label={t('phone.keys')}>
        <div className="phone-sheet-head">
          <h2 className="phone-sheet-title">{t('phone.keys')}</h2>
          <button
            type="button"
            className="phone-sheet-close"
            aria-label={t('common.close')}
            onClick={onClose}
          >
            ×
          </button>
        </div>
        {(session.shortcuts ?? []).length > 0 ? (
          <>
            <h3 className="phone-sheet-label">{t('phone.keyAgent')}</h3>
            <ul className="phone-key-list">
              {(session.shortcuts ?? []).map((shortcut) => (
                <li className="phone-key-list-row" key={shortcut.key}>
                  <span className="phone-key-list-name">{shortcut.label}</span>
                  <span className="phone-key-list-purpose">
                    {shortcut.description}
                  </span>
                </li>
              ))}
            </ul>
          </>
        ) : null}

        <h3 className="phone-sheet-label">{t('phone.keySaved')}</h3>
        {savedKeys.length === 0 ? (
          <p className="phone-sheet-note">{t('phone.keyNone')}</p>
        ) : (
          <ul className="phone-key-list">
            {savedKeys.map((entry) => (
              <li className="phone-key-list-row" key={entry.id}>
                <span className="phone-key-list-name">
                  {savedKeyLabel(entry.key, entry.modifiers)}
                </span>
                <button
                  type="button"
                  className="phone-key-remove"
                  aria-label={t('phone.keyRemoveLabel', {
                    name: savedKeyLabel(entry.key, entry.modifiers),
                  })}
                  onClick={() => onRemove(entry.id)}
                >
                  {t('phone.keyRemove')}
                </button>
              </li>
            ))}
          </ul>
        )}

        <form className="phone-key-form" onSubmit={submit}>
          <div
            className="phone-modifier-row"
            role="group"
            aria-label={t('phone.keyModifiers')}
          >
            {(['shift', 'ctrl', 'alt'] as const).map((modifier) => (
              <label className="phone-modifier" key={modifier}>
                <input
                  type="checkbox"
                  checked={modifiers.includes(modifier)}
                  onChange={(event) =>
                    setModifiers((current) =>
                      event.target.checked
                        ? [...current, modifier]
                        : current.filter((entry) => entry !== modifier),
                    )
                  }
                />
                <span>
                  {t(
                    modifier === 'shift'
                      ? 'phone.keyShift'
                      : modifier === 'ctrl'
                        ? 'phone.keyCtrl'
                        : 'phone.keyAlt',
                  )}
                </span>
              </label>
            ))}
          </div>
          <input
            className="text-input"
            type="text"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={t('phone.keyNamePlaceholder')}
            aria-label={t('phone.keyName')}
            disabled={full}
          />
          {message ? (
            <p className="phone-card-error" role="alert">
              {message}
            </p>
          ) : null}
          {full ? (
            <p className="phone-sheet-note">{t('phone.keyFull')}</p>
          ) : null}
          <button
            type="submit"
            className="btn btn-primary"
            disabled={full}
          >
            {t('phone.keyAdd')}
          </button>
        </form>
      </div>
    </div>
  );
}

function StartSessionSheet({
  projects,
  configs,
  t,
  onClose,
  onCreated,
}: {
  projects: Project[];
  configs: AiConfig[];
  t: Translate;
  onClose: () => void;
  onCreated: (session: Session) => void;
}) {
  const [projectId, setProjectId] = useState<string | null>(null);
  const [configId, setConfigId] = useState<string | null>(null);
  const [branch, setBranch] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const project = projects.find((entry) => entry.id === projectId) ?? null;
  const projectOptions = projects.map((entry) => ({
    id: entry.id,
    label: entry.name,
    meta: entry.folder,
  }));
  const configOptions = configs.map((entry) => ({
    id: entry.id,
    label: entry.name,
    meta: `${entry.client}${entry.model ? ` · ${entry.model}` : ''}`,
  }));

  const loadBranches = useCallback<ComboboxLoader>(
    async (query, signal) => {
      if (!project) {
        return [];
      }
      const list = await listBranches(project.folder, query, signal);
      return list.map((entry) => ({
        id: entry.name,
        label: entry.name,
        tag: entry.current ? t('newSession.currentTag') : undefined,
      }));
    },
    [project, t],
  );

  function chooseProject(id: string | null) {
    setProjectId(id);
    const next = projects.find((entry) => entry.id === id) ?? null;
    const config =
      configs.find((entry) => entry.id === next?.defaultConfigId) ?? null;
    setConfigId(config?.id ?? null);
    setBranch(next?.defaultSourceBranch ?? null);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const input = buildStartInput({
      projectId,
      configId,
      sourceBranch: branch,
      name,
      firstPrompt: prompt,
    });
    if (!input) {
      setError(t('phone.startIncomplete'));
      return;
    }
    setSubmitting(true);
    setError('');
    try {
      const session = await createSession(input);
      onCreated(session);
    } catch (failure) {
      setSubmitting(false);
      setError(
        failure instanceof Error ? failure.message : t('newSession.errorCreate'),
      );
    }
  }

  return (
    <div className="phone-sheet-layer">
      <button
        type="button"
        className="phone-sheet-scrim"
        aria-label={t('common.close')}
        onClick={onClose}
      />
      <div
        className="phone-sheet"
        role="dialog"
        aria-label={t('newSession.title')}
      >
        <div className="phone-sheet-head">
          <h2 className="phone-sheet-title">{t('newSession.title')}</h2>
          <button
            type="button"
            className="phone-sheet-close"
            aria-label={t('common.close')}
            onClick={onClose}
          >
            ×
          </button>
        </div>
        <form className="phone-start-form" onSubmit={submit} noValidate>
          <Combobox
            id="phone-start-project"
            label={t('newSession.project')}
            value={projectId}
            options={projectOptions}
            selectedOption={projectOptions.find((option) => option.id === projectId) ?? null}
            onChange={chooseProject}
            placeholder={t('newSession.projectPlaceholder')}
            required
            autoFocus
          />
          <Combobox
            id="phone-start-config"
            label={t('newSession.config')}
            value={configId}
            options={configOptions}
            selectedOption={configOptions.find((option) => option.id === configId) ?? null}
            onChange={setConfigId}
            placeholder={t('newSession.configPlaceholder')}
            required
          />
          <Combobox
            key={projectId ?? 'no-project'}
            id="phone-start-branch"
            label={t('newSession.sourceBranch')}
            value={branch}
            loadOptions={loadBranches}
            selectedOption={branch ? { id: branch, label: branch } : null}
            onChange={setBranch}
            placeholder={t('newSession.sourceBranchPlaceholder')}
            disabled={!project}
          />
          <div className="field">
            <label htmlFor="phone-start-name">{t('newSession.name')}</label>
            <input
              id="phone-start-name"
              className="text-input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t('newSession.namePlaceholder')}
            />
          </div>
          <div className="field">
            <label htmlFor="phone-start-prompt">
              {t('newSession.firstPrompt')}
            </label>
            <textarea
              id="phone-start-prompt"
              className="text-input prompt-input"
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              placeholder={t('newSession.firstPromptPlaceholder')}
              rows={3}
            />
          </div>
          {error ? (
            <p className="phone-card-error" role="alert">
              {error}
            </p>
          ) : null}
          <div className="phone-sheet-actions">
            <button
              type="button"
              className="btn btn-secondary"
              onClick={onClose}
              disabled={submitting}
            >
              {t('common.cancel')}
            </button>
            <button
              type="submit"
              className="btn btn-primary"
              disabled={submitting}
            >
              {submitting
                ? t('newSession.submitting')
                : t('newSession.submit')}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function SwitchConfigSheet({
  session,
  configs,
  providers,
  t,
  onChoose,
  onClose,
}: {
  session: Session;
  configs: AiConfig[];
  providers: Provider[];
  t: Translate;
  onChoose: (configId: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const choices = switchableConfigs(configs, session);
  const visible = filterChoices(choices, query, (config) =>
    `${config.name} ${config.client}`,
  );

  return (
    <div className="phone-sheet-layer">
      <button
        type="button"
        className="phone-sheet-scrim"
        aria-label={t('common.close')}
        onClick={onClose}
      />
      <div
        className="phone-sheet"
        role="dialog"
        aria-label={t('switchAccount.title')}
      >
        <div className="phone-sheet-head">
          <h2 className="phone-sheet-title">{t('switchAccount.title')}</h2>
          <button
            type="button"
            className="phone-sheet-close"
            aria-label={t('common.close')}
            onClick={onClose}
          >
            ×
          </button>
        </div>
        {choices.length === 0 ? (
          <p className="phone-sheet-note">{t('phone.switchNone')}</p>
        ) : (
          <>
            <input
              className="text-input"
              type="text"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('phone.switchFilter')}
              aria-label={t('phone.switchFilter')}
            />
            <ul className="phone-sheet-list">
              {visible.map((config) => {
                const provider = providers.find(
                  (entry) => entry.id === config.providerId,
                );
                const summary = provider ? usageSummary(provider) : null;
                return (
                  <li key={config.id}>
                    <button
                      type="button"
                      className="phone-sheet-row"
                      onClick={() => onChoose(config.id)}
                    >
                      <span className="phone-sheet-row-name">
                        {config.name}
                      </span>
                      <span className="phone-sheet-row-meta">
                        {[config.client, provider?.name, summary]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                    </button>
                  </li>
                );
              })}
              {visible.length === 0 ? (
                <li className="phone-sheet-note">
                  {t('combobox.noMatches')}
                </li>
              ) : null}
            </ul>
          </>
        )}
      </div>
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
