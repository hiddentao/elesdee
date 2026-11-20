import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';
import { createPortal } from 'react-dom';
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
import { subscribeAux, subscribeEvents, subscribeHistory } from '../api/events';
import type {
  AiConfig,
  HistoryItem,
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
import { tabStateLabel, workLabel } from '../state/sessionStatus';
import {
  hasProjects,
  isWaiting,
  matchesPhoneFilter,
  rowPreview,
  startBlocker,
  type PhoneFilter,
} from './list';
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
import { historySupported, mergeHistory } from './history';
import {
  MessageView,
  SessionContextStrip,
  type SessionView,
} from './MessageView';
import { SessionFiles, type PhoneFilePanel } from './SessionFiles';
import { usePhoneNotifications } from './notifications';

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

const SHEET_FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

function sheetFocusables(sheet: HTMLElement): HTMLElement[] {
  return Array.from(
    sheet.querySelectorAll<HTMLElement>(SHEET_FOCUSABLE),
  ).filter(
    (element) =>
      !element.closest('[hidden], [inert]') &&
      element.getAttribute('aria-hidden') !== 'true',
  );
}

/**
 * A bottom sheet that takes focus, keeps Tab inside, closes on Escape and
 * returns focus to its trigger. The sheet renders beside the app root so the
 * rest of the screen can be inert while it is open.
 */
function PhoneSheet({
  id,
  label,
  labelledBy,
  closeLabel,
  initialFocus,
  returnFocus,
  onClose,
  children,
}: {
  id?: string;
  label?: string;
  labelledBy?: string;
  closeLabel: string;
  initialFocus?: string;
  returnFocus?: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const sheetRef = useRef<HTMLDivElement>(null);
  const openerRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const sheet = sheetRef.current;
    if (!sheet) {
      return;
    }

    if (!sheet.contains(document.activeElement)) {
      const preferred =
        (initialFocus
          ? sheet.querySelector<HTMLElement>(initialFocus)
          : null) ??
        sheet.querySelector<HTMLElement>('[aria-pressed="true"]') ??
        sheet.querySelector<HTMLElement>('.phone-sheet-row:not(.is-danger)') ??
        sheetFocusables(sheet)[0] ??
        sheet;
      preferred.focus();
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== 'Tab' || !sheetRef.current) {
        return;
      }
      const items = sheetFocusables(sheetRef.current);
      if (items.length === 0) {
        event.preventDefault();
        sheetRef.current.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      const inside =
        active instanceof HTMLElement && sheetRef.current.contains(active);
      if (!inside) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
        return;
      }
      if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      const target = returnFocus
        ? document.querySelector<HTMLElement>(returnFocus)
        : openerRef.current;
      if (target?.isConnected && !target.closest('[inert]')) {
        target.focus();
      }
    };
  }, [initialFocus, returnFocus]);

  return createPortal(
    <div className="phone-sheet-layer">
      <button
        type="button"
        className="phone-sheet-scrim"
        aria-label={closeLabel}
        aria-hidden="true"
        tabIndex={-1}
        onClick={() => closeRef.current()}
      />
      <div
        id={id}
        ref={sheetRef}
        className="phone-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={labelledBy ? undefined : label}
        aria-labelledby={labelledBy}
        tabIndex={-1}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
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
  const [actionsOpen, setActionsOpen] = useState(false);
  const [keysOpen, setKeysOpen] = useState(false);
  const [railOpen, setRailOpen] = useState(true);
  const [draft, setDraft] = useState('');
  const [latches, setLatches] = useState<KeyModifiers>(noModifiers);
  const [savedKeys, setSavedKeys] = useState<SavedKey[]>(() =>
    readSavedKeys(keyStorage()),
  );
  const [actionNotice, setActionNotice] = useState('');
  const [switchError, setSwitchError] = useState('');
  const [activeId, setActiveId] = useState<string | null>(sessionHashId);
  const [screen, setScreen] = useState<'list' | 'session'>(
    sessionHashId() ? 'session' : 'list',
  );
  const [sessionView, setSessionView] = useState<SessionView>('messages');
  const [filePanel, setFilePanel] = useState<PhoneFilePanel | null>(null);
  const [historyItems, setHistoryItems] = useState<HistoryItem[]>([]);
  const [historyStructured, setHistoryStructured] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [expandedTools, setExpandedTools] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<PhoneFilter>('all');
  const [sheetOpen, setSheetOpen] = useState(false);
  const [dismissedTabs, setDismissedTabs] = useState<Set<string>>(new Set());
  const [languageSheetOpen, setLanguageSheetOpen] = useState(false);
  const [cardAnnouncement, setCardAnnouncement] = useState('');
  const announcedCardRef = useRef<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const tabStripRef = useRef<HTMLDivElement>(null);
  const sessionTitleRef = useRef<HTMLHeadingElement>(null);
  const focusTitleRef = useRef(false);
  const dialogReturnRef = useRef(false);
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;

  usePhoneNotifications({
    sessions,
    activeSessionId: activeId,
    screen,
  });

  useEffect(() => {
    setFilePanel(null);
  }, [activeId]);

  useEffect(() => {
    const onHash = () => {
      const id = sessionHashId();
      if (id) {
        setActiveId(id);
        setScreen('session');
      }
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

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
    if (loading || active) {
      return;
    }
    const fromHash = sessionHashId();
    const target =
      openSessions.find((session) => session.id === fromHash) ??
      openSessions[0];
    if (target) {
      if (target.id !== activeId) {
        setActiveId(target.id);
      }
      return;
    }
    setActiveId(null);
    setScreen('list');
    window.history.replaceState(null, '', window.location.pathname);
  }, [loading, active, openSessions, activeId]);

  useEffect(() => {
    if (active) {
      return;
    }
    setActionsOpen(false);
    setSwitchOpen(false);
    setKeysOpen(false);
    setUsageOpen(false);
    setCloseOpen(false);
    setSwitchError('');
  }, [active]);

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
    if (!focusTitleRef.current || screen !== 'session' || !active) {
      return;
    }
    focusTitleRef.current = false;
    sessionTitleRef.current?.focus();
  }, [screen, active]);

  useEffect(() => {
    const open = usageOpen || closeOpen;
    if (open) {
      dialogReturnRef.current = true;
      return;
    }
    if (!dialogReturnRef.current) {
      return;
    }
    dialogReturnRef.current = false;
    queueMicrotask(() => {
      const current = document.activeElement;
      if (
        current instanceof HTMLElement &&
        current !== document.body &&
        current.isConnected &&
        !current.closest('[inert]')
      ) {
        return;
      }
      document.getElementById('phone-actions-trigger')?.focus();
    });
  }, [usageOpen, closeOpen]);

  const activeStatusRef = useRef(active?.status ?? 'closed');
  activeStatusRef.current = active?.status ?? 'closed';

  useEffect(() => {
    if (!active) {
      return;
    }
    const supported = historySupported(active.client, active.kind);
    setHistoryItems([]);
    setExpandedTools(new Set());
    setHistoryStructured(supported);
    setHistoryLoading(supported);
    setSessionView(supported ? 'messages' : 'terminal');
    if (!supported) {
      return;
    }
    return subscribeHistory(
      active.id,
      0,
      (status) => {
        setHistoryStructured(status.structured);
        setHistoryLoading(false);
        if (!status.structured) {
          setSessionView('terminal');
        }
      },
      (item) =>
        setHistoryItems((current) => mergeHistory(current, item)),
      () => setHistoryLoading(false),
      undefined,
      () => activeStatusRef.current === 'closed',
    );
    // The stream belongs to one session. Session updates change the object,
    // not the conversation source.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id]);

  function closeLanguageSheet() {
    setLanguageSheetOpen(false);
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
    setActionsOpen(false);
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
    setActionsOpen(false);
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

  function submitComposer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!active) {
      return;
    }
    const value = draft;
    setDraft('');
    void sendTerminal(active, `${value}\r`);
  }

  function toggleSessionView() {
    setSessionView((current) =>
      current === 'messages' ? 'terminal' : 'messages',
    );
  }

  function toggleTool(id: string) {
    setExpandedTools((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  async function chooseConfig(configId: string) {
    if (!active) {
      return;
    }
    setSwitchError('');
    try {
      const updated = await switchSessionConfig(active.id, configId);
      mergeSession(updated);
      setSwitchOpen(false);
      setActionNotice('');
    } catch {
      setSwitchError(tRef.current('switchAccount.errorSwitch'));
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

  const visible = sessions.filter((session) =>
    matchesPhoneFilter(session, filter),
  );
  const waiting = visible.filter(isWaiting);
  const rest = visible.filter((session) => !isWaiting(session));
  const activeQuestion = active
    ? questions.find((question) => question.sessionId === active.id) ?? null
    : null;
  const activeRequest = active
    ? requests.find((request) => request.parentSessionId === active.id) ?? null
    : null;
  const countLabel = openSessions.length;
  const canStart = hasProjects(projects);
  const cardKey = active?.pendingPermission
    ? `permission:${active.pendingPermission.requestId}`
    : activeRequest
      ? `request:${activeRequest.id}`
      : activeQuestion
        ? `question:${activeQuestion.id}`
        : null;
  const cardNotice = active?.pendingPermission
    ? `${t('permission.needed')}: ${active.pendingPermission.tool}`
    : activeRequest
      ? t('requests.title')
      : activeQuestion
        ? t('phone.question')
        : '';

  useEffect(() => {
    if (!cardKey) {
      announcedCardRef.current = null;
      setCardAnnouncement('');
      return;
    }
    if (announcedCardRef.current === cardKey) {
      return;
    }
    announcedCardRef.current = cardKey;
    setCardAnnouncement('');
    const timer = window.setTimeout(() => setCardAnnouncement(cardNotice), 0);
    return () => window.clearTimeout(timer);
  }, [cardKey, cardNotice]);

  const hasActiveSessions = sessions.some(
    (session) => session.status === 'active',
  );
  const anySheetOpen =
    sheetOpen ||
    languageSheetOpen ||
    startOpen ||
    (actionsOpen && active !== null) ||
    (switchOpen && active !== null) ||
    (keysOpen && active !== null) ||
    usageOpen ||
    (closeOpen && active !== null);

  return (
    <div className="phone" inert={anySheetOpen}>
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
              <h1 className="phone-title" ref={sessionTitleRef} tabIndex={-1}>
                {active.name}
              </h1>
              <span className="phone-sub">
                {workLabel(active, t) ?? t('status.active')} ·{' '}
                {active.projectId}
              </span>
            </div>
          </>
        ) : (
          <div className="phone-head-text">
            <h1 className="phone-title">{t('phone.sessions')}</h1>
            <span className="phone-sub">{t('phone.thisComputer')}</span>
          </div>
        )}
        {screen === 'list' ? (
          <button
            type="button"
            id="phone-start-trigger"
            className="phone-icon"
            aria-label={t('phone.startSession')}
            aria-haspopup="dialog"
            aria-expanded={startOpen}
            disabled={!canStart}
            title={canStart ? undefined : t('phone.noProjects')}
            onClick={() => setStartOpen(true)}
          >
            <span aria-hidden="true">+</span>
          </button>
        ) : screen === 'session' && active ? (
          <button
            type="button"
            id="phone-actions-trigger"
            className="phone-icon"
            aria-label={t('phone.sessionActions')}
            aria-haspopup="dialog"
            aria-expanded={actionsOpen}
            onClick={() => setActionsOpen(true)}
          >
            <MoreIcon />
          </button>
        ) : null}
        <button
          type="button"
          id="phone-language-trigger"
          className="phone-icon"
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
          {!canStart ? (
            <section className="phone-empty-card">
              <h2 className="phone-empty-title">
                {t('phone.noProjectsTitle')}
              </h2>
              <p className="phone-empty-text">{t('phone.noProjects')}</p>
            </section>
          ) : (
            <>
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
                      active={session.id === activeId}
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
                  <p className="phone-empty">
                    {t(
                      hasActiveSessions
                        ? 'phone.noFilterMatches'
                        : 'phone.noSessions',
                    )}
                  </p>
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
            </>
          )}
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
                  aria-current={
                    session.id === activeId ? 'true' : undefined
                  }
                  onClick={() => selectSession(session.id)}
                >
                  <span className="phone-tab-name">{session.name}</span>
                  <span className="phone-tab-meta">
                    {tabStateLabel(session, t)}
                  </span>
                </button>
              ))}
            </div>
            <button
              type="button"
              id="phone-tabs-trigger"
              className="phone-tab-count"
              aria-label={t('phone.allTabsLabel', {
                count: formatNumber(language, countLabel),
              })}
              aria-haspopup="dialog"
              aria-controls="phone-tabs-sheet"
              aria-expanded={sheetOpen}
              onClick={() => setSheetOpen((open) => !open)}
            >
              {countLabel}
            </button>
          </div>

          {sheetOpen ? (
            <PhoneSheet
              id="phone-tabs-sheet"
              label={t('phone.sheetAllTabs')}
              closeLabel={t('phone.closeTabs')}
              initialFocus=".phone-sheet-row.is-on"
              returnFocus="#phone-tabs-trigger"
              onClose={() => setSheetOpen(false)}
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
            </PhoneSheet>
          ) : null}

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

          <div
            className="phone-session-tools"
            role="group"
            aria-label={t('phone.sessionFiles')}
          >
            <button
              type="button"
              className={`btn btn-secondary phone-tool${filePanel === 'files' ? ' is-on' : ''}`}
              aria-pressed={filePanel === 'files'}
              onClick={() =>
                setFilePanel((current) => (current === 'files' ? null : 'files'))
              }
            >
              {t('phone.files')}
            </button>
            <button
              type="button"
              className={`btn btn-secondary phone-tool${filePanel === 'changes' ? ' is-on' : ''}`}
              aria-pressed={filePanel === 'changes'}
              onClick={() =>
                setFilePanel((current) =>
                  current === 'changes' ? null : 'changes',
                )
              }
            >
              {t('phone.changes')}
            </button>
          </div>

          {filePanel ? (
            <SessionFiles
              session={active}
              panel={filePanel}
              t={t}
              language={language}
            />
          ) : null}

          <SessionContextStrip
            session={active}
            view={sessionView}
            onToggleView={toggleSessionView}
            t={t}
            language={language}
          />

          {sessionView === 'messages' && historyStructured ? (
            <MessageView
              session={active}
              items={historyItems}
              loading={historyLoading}
              expanded={expandedTools}
              onToggleTool={toggleTool}
              t={t}
            />
          ) : (
            <>
              {!historySupported(active.client, active.kind) ? (
                <p className="phone-note">{t('phone.terminalOnly')}</p>
              ) : null}
              <div className="phone-terminal">
                <TerminalPane session={active} readOnly />
              </div>
            </>
          )}

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
            <form className="phone-input-row" onSubmit={submitComposer}>
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
                placeholder={t(
                  sessionView === 'messages'
                    ? 'phone.messagePlaceholder'
                    : 'phone.inputPlaceholder',
                )}
                aria-label={t(
                  sessionView === 'messages'
                    ? 'phone.promptLabel'
                    : 'phone.terminalCommand',
                )}
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
        <PhoneSheet
          id="phone-language-sheet"
          labelledBy="phone-language-title"
          closeLabel={t('common.close')}
          initialFocus='[aria-pressed="true"]'
          returnFocus="#phone-language-trigger"
          onClose={closeLanguageSheet}
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
        </PhoneSheet>
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
            focusTitleRef.current = true;
          }}
        />
      ) : null}

      {actionsOpen && active ? (
        <SessionActionsSheet
          session={active}
          t={t}
          onSwitch={() => {
            setActionsOpen(false);
            setSwitchError('');
            setSwitchOpen(true);
          }}
          onUsage={() => {
            setActionsOpen(false);
            setUsageOpen(true);
          }}
          onCloseSession={() => {
            setActionsOpen(false);
            setCloseOpen(true);
          }}
          onClose={() => setActionsOpen(false)}
        />
      ) : null}

      {switchOpen && active ? (
        <SwitchConfigSheet
          session={active}
          configs={configs}
          providers={providers}
          t={t}
          error={switchError}
          onChoose={(configId) => void chooseConfig(configId)}
          onClose={() => {
            setSwitchError('');
            setSwitchOpen(false);
          }}
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

      {usageOpen
        ? createPortal(
            <UsageDialog
              providers={providers}
              onClose={() => setUsageOpen(false)}
            />,
            document.body,
          )
        : null}

      {closeOpen && active
        ? createPortal(
            <CloseSessionDialog
              session={active}
              onConfirm={confirmClose}
              onClose={() => setCloseOpen(false)}
            />,
            document.body,
          )
        : null}
      {createPortal(
        <p className="sr-only phone-live" role="status" aria-live="polite">
          {cardAnnouncement}
        </p>,
        document.body,
      )}
    </div>
  );
}

function MoreIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" width="20" height="20">
      <circle cx="5" cy="12" r="1.7" fill="currentColor" />
      <circle cx="12" cy="12" r="1.7" fill="currentColor" />
      <circle cx="19" cy="12" r="1.7" fill="currentColor" />
    </svg>
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
          id="phone-add-key-trigger"
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
    <PhoneSheet
      id="phone-keys-sheet"
      label={t('phone.keys')}
      closeLabel={t('common.close')}
      initialFocus='input[type="text"]'
      returnFocus="#phone-add-key-trigger"
      onClose={onClose}
    >
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
    </PhoneSheet>
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
  const [invalid, setInvalid] = useState<'project' | 'config' | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const project = projects.find((entry) => entry.id === projectId) ?? null;
  const blocker = startBlocker(projects, projectId, configId);
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
    setInvalid(null);
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
      const field = !projectId ? 'project' : 'config';
      setInvalid(field);
      setError('');
      document
        .getElementById(
          field === 'project'
            ? 'phone-start-project'
            : 'phone-start-config',
        )
        ?.focus();
      return;
    }
    setInvalid(null);
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
    <PhoneSheet
      id="phone-start-sheet"
      label={t('newSession.title')}
      closeLabel={t('common.close')}
      initialFocus="#phone-start-project"
      returnFocus="#phone-start-trigger"
      onClose={onClose}
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
        {projects.length === 0 ? (
          <p className="phone-sheet-note">{t('phone.noProjects')}</p>
        ) : (
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
              error={
                invalid === 'project'
                  ? t('phone.startIncomplete')
                  : undefined
              }
            />
            <Combobox
              id="phone-start-config"
              label={t('newSession.config')}
              value={configId}
              options={configOptions}
              selectedOption={configOptions.find((option) => option.id === configId) ?? null}
              onChange={(id) => {
                setConfigId(id);
                setInvalid(null);
              }}
              placeholder={t('newSession.configPlaceholder')}
              required
              error={
                invalid === 'config' ? t('phone.startIncomplete') : undefined
              }
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
            {blocker === 'incomplete' ? (
              <p className="phone-sheet-note">
                {t('phone.startIncomplete')}
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
                aria-disabled={
                  submitting || blocker !== null ? 'true' : undefined
                }
              >
                {submitting
                  ? t('newSession.submitting')
                  : t('newSession.submit')}
              </button>
            </div>
          </form>
        )}
    </PhoneSheet>
  );
}

function SessionActionsSheet({
  session,
  t,
  onSwitch,
  onUsage,
  onCloseSession,
  onClose,
}: {
  session: Session;
  t: Translate;
  onSwitch: () => void;
  onUsage: () => void;
  onCloseSession: () => void;
  onClose: () => void;
}) {
  return (
    <PhoneSheet
      id="phone-actions-sheet"
      label={t('phone.sessionActions')}
      closeLabel={t('common.close')}
      initialFocus=".phone-sheet-row:not(.is-danger)"
      returnFocus="#phone-actions-trigger"
      onClose={onClose}
    >
      <div className="phone-sheet-head">
        <h2 className="phone-sheet-title">{t('phone.sessionActions')}</h2>
        <button
          type="button"
          className="phone-sheet-close"
          aria-label={t('common.close')}
          onClick={onClose}
        >
          ×
        </button>
      </div>
      {canSwitch(session) ? (
        <button type="button" className="phone-sheet-row" onClick={onSwitch}>
          <span className="phone-sheet-row-name">
            {t('session.switchAccountMenu')}
          </span>
        </button>
      ) : null}
      <button type="button" className="phone-sheet-row" onClick={onUsage}>
        <span className="phone-sheet-row-name">{t('usage.title')}</span>
      </button>
      <button
        type="button"
        className="phone-sheet-row is-danger"
        onClick={onCloseSession}
      >
        <span className="phone-sheet-row-name">
          {t('session.closeMenuAction')}
        </span>
      </button>
    </PhoneSheet>
  );
}

function SwitchConfigSheet({
  session,
  configs,
  providers,
  t,
  error,
  onChoose,
  onClose,
}: {
  session: Session;
  configs: AiConfig[];
  providers: Provider[];
  t: Translate;
  error: string;
  onChoose: (configId: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const choices = switchableConfigs(configs, session);
  const visible = filterChoices(choices, query, (config) =>
    `${config.name} ${config.client}`,
  );

  return (
    <PhoneSheet
      id="phone-switch-sheet"
      label={t('switchAccount.title')}
      closeLabel={t('common.close')}
      initialFocus='input[type="text"]'
      returnFocus="#phone-actions-trigger"
      onClose={onClose}
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
        {error ? (
          <p className="phone-card-error" role="alert">
            {error}
          </p>
        ) : null}
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
    </PhoneSheet>
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
      aria-current={active ? 'true' : undefined}
      onClick={() => onSelect(session.id)}
    >
      <span className="phone-row-main">
        <span className="phone-row-name">{session.name}</span>
        <span className="phone-row-preview">{rowPreview(session)}</span>
      </span>
      <span className="phone-row-state">{tabStateLabel(session, t)}</span>
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
        aria-current={active ? 'true' : undefined}
        style={{ transform: `translateX(${offset}px)` }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerEnd}
        onPointerCancel={handlePointerEnd}
        onClick={handleClick}
      >
        <span className="phone-sheet-row-name">{session.name}</span>
        <span className="phone-sheet-row-meta">
          {tabStateLabel(session, t)}
          {active ? (
            <span className="phone-sheet-check" aria-hidden="true">
              {' '}✓
            </span>
          ) : null}
        </span>
      </button>
    </div>
  );
}
