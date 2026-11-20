import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
} from 'react';
import {
  activateBrowserTab,
  browserBack,
  browserForward,
  browserReload,
  browserScreenshotUrl,
  clickBrowserAt,
  closeBrowserTab,
  createBrowserTab,
  getBrowserState,
  openBrowserUrl,
  pressBrowserKey,
  setupBrowser,
  typeBrowserText,
} from '../api/client';
import type { BrowserState, BrowserTab, Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { CloseIcon, GlobeIcon, PlusIcon } from './icons';

interface BrowserPanelProps {
  session: Session;
}

/// The named keys that the live view sends to the page.
const LIVE_KEYS = new Set([
  'Enter',
  'Tab',
  'Escape',
  'Backspace',
  'Delete',
  'ArrowLeft',
  'ArrowUp',
  'ArrowRight',
  'ArrowDown',
  'Home',
  'End',
  'PageUp',
  'PageDown',
]);

/// The DevTools modifier bits shared with the core input route.
const MOD_ALT = 1;
const MOD_CONTROL = 2;
const MOD_META = 4;
const MOD_SHIFT = 8;
const COMMAND_MODIFIERS = MOD_ALT | MOD_CONTROL | MOD_META;

/// The macOS command keys that the window and the app keep for themselves.
const MAC_APP_KEYS = new Set(['q', 'w', 'm', 'h']);

function isMacPlatform(): boolean {
  return /mac/i.test(navigator.userAgent);
}

/// The chords that act on the app window rather than on the page. They are
/// left to the app and never travel to the session browser.
function isAppShortcut(event: KeyboardEvent<HTMLDivElement>): boolean {
  if (event.altKey || event.shiftKey) {
    return false;
  }
  const key = event.key.toLowerCase();
  if (isMacPlatform()) {
    return event.metaKey && !event.ctrlKey && MAC_APP_KEYS.has(key);
  }
  return event.ctrlKey && !event.metaKey && key === 'w';
}

function modifierMask(event: KeyboardEvent<HTMLDivElement>): number {
  return (
    (event.altKey ? MOD_ALT : 0) |
    (event.ctrlKey ? MOD_CONTROL : 0) |
    (event.metaKey ? MOD_META : 0) |
    (event.shiftKey ? MOD_SHIFT : 0)
  );
}

export function BrowserPanel({ session }: BrowserPanelProps) {
  const { t } = useLanguage();
  const [state, setState] = useState<BrowserState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [address, setAddress] = useState('');
  const addressRef = useRef<HTMLInputElement>(null);
  const addressFocused = useRef(false);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  // Each action result advances the version. A poll answer that began before
  // the latest action describes an older page, so it never replaces that state.
  const stateVersion = useRef(0);

  const activeTab: BrowserTab | null =
    state?.tabs.find((tab) => tab.id === state.activeTabId) ?? null;

  useEffect(() => {
    let stopped = false;
    let timer: number | undefined;

    const load = async () => {
      const version = stateVersion.current;
      try {
        const next = await getBrowserState(session.id);
        if (!stopped && version === stateVersion.current) {
          setState(next);
          setError(null);
        }
      } catch (failure) {
        if (!stopped) {
          setError(
            failure instanceof Error && failure.message
              ? failure.message
              : t('panel.requestFailed'),
          );
        }
      }
      if (!stopped) {
        timer = window.setTimeout(load, 1000);
      }
    };

    setState(null);
    setError(null);
    setBusy(null);
    setAddress('');
    void load();
    return () => {
      stopped = true;
      if (timer !== undefined) {
        window.clearTimeout(timer);
      }
    };
    // The panel is recreated for each session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id]);

  useEffect(() => {
    if (!addressFocused.current) {
      setAddress(activeTab?.url ?? '');
    }
  }, [activeTab?.id, activeTab?.url]);

  async function run(action: () => Promise<BrowserState>, busyLabel: string) {
    setBusy(busyLabel);
    setError(null);
    try {
      const next = await action();
      stateVersion.current += 1;
      setState(next);
    } catch (failure) {
      setError(
        failure instanceof Error && failure.message
          ? failure.message
          : t('panel.browserFailed'),
      );
    } finally {
      setBusy(null);
    }
  }

  function submitAddress(event: FormEvent) {
    event.preventDefault();
    const url = address.trim();
    if (!url) {
      return;
    }
    void run(() => openBrowserUrl(session.id, url), t('panel.browserLoading'));
  }

  function handleTabListKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
      return;
    }
    // Only a tab moves through the strip. Close, new tab and the rest keep
    // their own arrow-key behavior.
    const target = event.target as HTMLElement;
    if (!target.closest('[role="tab"]')) {
      return;
    }
    const tabs = state?.tabs ?? [];
    if (tabs.length === 0) {
      return;
    }
    event.preventDefault();
    const current = Math.max(
      0,
      tabs.findIndex((tab) => tab.id === state?.activeTabId),
    );
    const index =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? tabs.length - 1
          : (current + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) %
            tabs.length;
    const next = tabs[index];
    tabRefs.current[next.id]?.focus();
    if (next.id !== state?.activeTabId) {
      void run(
        () => activateBrowserTab(session.id, next.id),
        t('panel.browserLoading'),
      );
    }
  }

  async function newTab() {
    await run(() => createBrowserTab(session.id), t('panel.browserLoading'));
    addressRef.current?.focus();
  }

  // Live input is serialized so a click, a key and text reach the page in the
  // order they happened. Text collects for a moment and travels as one call.
  const inputChain = useRef<Promise<void>>(Promise.resolve());
  const textBuffer = useRef('');
  const flushScheduled = useRef(false);

  async function sendInput(action: () => Promise<BrowserState>) {
    try {
      const next = await action();
      stateVersion.current += 1;
      setState(next);
      setError(null);
    } catch (failure) {
      setError(
        failure instanceof Error && failure.message
          ? failure.message
          : t('panel.browserFailed'),
      );
    }
  }

  function enqueue(task: () => Promise<void>) {
    inputChain.current = inputChain.current.then(task).catch(() => undefined);
  }

  function flushText() {
    const text = textBuffer.current;
    textBuffer.current = '';
    return text ? sendInput(() => typeBrowserText(session.id, text)) : Promise.resolve();
  }

  function queueText(text: string) {
    textBuffer.current += text;
    if (flushScheduled.current) {
      return;
    }
    flushScheduled.current = true;
    enqueue(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 40));
      flushScheduled.current = false;
      await flushText();
    });
  }

  function pressKey(key: string, modifiers: number) {
    enqueue(async () => {
      await flushText();
      await sendInput(() => pressBrowserKey(session.id, key, modifiers));
    });
  }

  function clickLive(event: MouseEvent<HTMLDivElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return;
    }
    const x = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height));
    enqueue(async () => {
      await flushText();
      await sendInput(() => clickBrowserAt(session.id, x, y));
    });
  }

  function keyDownLive(event: KeyboardEvent<HTMLDivElement>) {
    if (isAppShortcut(event)) {
      return;
    }
    const { key } = event;
    const modifiers = modifierMask(event);
    const command = (modifiers & COMMAND_MODIFIERS) !== 0;

    // A plain character is typed. A letter or digit with Control, Meta or Alt
    // is a page shortcut; another character with a command modifier is not
    // defined, so it stays with the app.
    if (key.length === 1) {
      if (!command) {
        event.preventDefault();
        queueText(key);
        return;
      }
      if (/[a-z0-9]/i.test(key)) {
        event.preventDefault();
        pressKey(
          event.shiftKey ? key.toUpperCase() : key.toLowerCase(),
          modifiers,
        );
      }
      return;
    }

    // Named keys keep their modifiers, so Control and Alt reach the page too.
    if (LIVE_KEYS.has(key)) {
      event.preventDefault();
      pressKey(key, modifiers);
    }
  }

  const loading = busy !== null || activeTab?.loading === true;
  const setup = state?.setup ?? null;
  const setupPhase = setup?.phase ?? 'idle';
  const setupActive =
    setupPhase === 'checking' ||
    setupPhase === 'downloading' ||
    setupPhase === 'verifying' ||
    setupPhase === 'unpacking';
  const setupFailed = setupPhase === 'failed';
  const setupLabel =
    setupPhase === 'downloading'
      ? t('panel.browserSetupDownloading')
      : setupPhase === 'verifying'
        ? t('panel.browserSetupVerifying')
        : setupPhase === 'unpacking'
          ? t('panel.browserSetupUnpacking')
          : t('panel.browserSetupChecking');
  const setupPercent =
    setup?.total && setup.total > 0
      ? Math.min(100, Math.round((setup.downloaded / setup.total) * 100))
      : null;
  const unavailable =
    state !== null && !state.available && !setupActive && !setupFailed;
  const message =
    setupFailed || setupActive ? null : state?.error ?? error;
  const controlsDisabled = busy !== null || setupActive;
  const rovingTabId = state?.activeTabId ?? state?.tabs[0]?.id ?? null;

  return (
    <div className="browser-panel">
      <div className="browser-bar">
        <div className="browser-controls">
          <button
            type="button"
            className="icon-btn browser-control"
            aria-label={t('panel.browserBack')}
            title={t('panel.browserBack')}
            disabled={!activeTab?.canGoBack || controlsDisabled}
            onClick={() =>
              void run(() => browserBack(session.id), t('panel.browserLoading'))
            }
          >
            <span aria-hidden="true">←</span>
          </button>
          <button
            type="button"
            className="icon-btn browser-control"
            aria-label={t('panel.browserForward')}
            title={t('panel.browserForward')}
            disabled={!activeTab?.canGoForward || controlsDisabled}
            onClick={() =>
              void run(() => browserForward(session.id), t('panel.browserLoading'))
            }
          >
            <span aria-hidden="true">→</span>
          </button>
          <button
            type="button"
            className="icon-btn browser-control"
            aria-label={t('panel.browserReload')}
            title={t('panel.browserReload')}
            disabled={!activeTab || controlsDisabled}
            onClick={() =>
              void run(() => browserReload(session.id), t('panel.browserLoading'))
            }
          >
            <span aria-hidden="true">↻</span>
          </button>
        </div>

        <form className="browser-address" onSubmit={submitAddress}>
          <GlobeIcon />
          <input
            ref={addressRef}
            type="text"
            value={address}
            aria-label={t('panel.browserAddress')}
            placeholder="https://"
            spellCheck={false}
            onFocus={() => {
              addressFocused.current = true;
            }}
            onBlur={() => {
              addressFocused.current = false;
            }}
            onChange={(event) => setAddress(event.target.value)}
          />
          <button
            type="submit"
            className="btn btn-ghost browser-go"
            disabled={controlsDisabled || address.trim() === ''}
          >
            {t('panel.browserOpen')}
          </button>
        </form>
      </div>

      <div
        className="browser-tabs"
        role="tablist"
        aria-label={t('panel.browserTabLabel')}
        onKeyDown={handleTabListKeyDown}
      >
        {state?.tabs.map((tab) => {
          const label = tab.title || tab.url || t('panel.browserNoTitle');
          return (
            <div className="browser-tab-item" role="presentation" key={tab.id}>
              <button
                ref={(element) => {
                  tabRefs.current[tab.id] = element;
                }}
                type="button"
                role="tab"
                id={`browser-tab-${tab.id}`}
                aria-selected={tab.id === state.activeTabId}
                aria-controls="browser-tabpanel"
                tabIndex={tab.id === rovingTabId ? 0 : -1}
                className={`browser-tab${
                  tab.id === state.activeTabId ? ' is-on' : ''
                }`}
                title={tab.url || label}
                onClick={() =>
                  void run(
                    () => activateBrowserTab(session.id, tab.id),
                    t('panel.browserLoading'),
                  )
                }
              >
                {tab.loading ? (
                  <span className="browser-tab-loading" aria-hidden="true" />
                ) : null}
                <span className="browser-tab-title">{label}</span>
              </button>
              <button
                type="button"
                className="browser-tab-close"
                aria-label={t('panel.browserCloseTab', { title: label })}
                title={t('panel.browserCloseTab', { title: label })}
                onClick={() =>
                  void run(
                    () => closeBrowserTab(session.id, tab.id),
                    t('panel.browserLoading'),
                  )
                }
              >
                <CloseIcon />
              </button>
            </div>
          );
        })}
        <button
          type="button"
          className="icon-btn browser-new-tab"
          aria-label={t('panel.browserNewTab')}
          title={t('panel.browserNewTab')}
          disabled={controlsDisabled}
          onClick={() => void newTab()}
        >
          <PlusIcon />
        </button>
      </div>

      {setupFailed ? (
        <div className="browser-setup" role="alert">
          <p className="browser-setup-title">{t('panel.browserSetupFailed')}</p>
          {setup?.error ? (
            <p className="browser-setup-detail">{setup.error}</p>
          ) : null}
          <button
            type="button"
            className="btn btn-ghost browser-setup-retry"
            disabled={busy !== null}
            onClick={() =>
              void run(
                () => setupBrowser(session.id),
                t('panel.browserSetupChecking'),
              )
            }
          >
            {t('panel.browserSetupRetry')}
          </button>
        </div>
      ) : message ? (
        <p className="dock-error" role="alert">
          {message}
        </p>
      ) : null}

      {setupActive ? (
        <div className="browser-setup" role="status" aria-live="polite">
          <p className="browser-setup-title">{setupLabel}</p>
          {setupPercent !== null ? (
            <>
              <progress
                className="browser-setup-progress"
                value={setup?.downloaded ?? 0}
                max={setup?.total ?? undefined}
                aria-label={setupLabel}
              />
              <p className="browser-setup-detail">
                {t('panel.browserSetupProgress', { percent: setupPercent })}
              </p>
            </>
          ) : null}
        </div>
      ) : null}

      {loading ? (
        <p className="browser-status" role="status" aria-live="polite">
          {busy ?? t('panel.browserLoading')}
        </p>
      ) : null}

      {unavailable && !message ? (
        <p className="dock-note">{t('panel.browserUnavailable')}</p>
      ) : null}

      <div
        className="browser-view"
        id="browser-tabpanel"
        role="tabpanel"
        aria-labelledby={
          activeTab ? `browser-tab-${activeTab.id}` : undefined
        }
      >
        {activeTab && state && state.screenshotSeq > 0 ? (
          <>
            <div className="browser-stage">
              <img
                key={`${activeTab.id}:${state.screenshotSeq}`}
                className="browser-image"
                src={browserScreenshotUrl(
                  session.id,
                  activeTab.id,
                  state.screenshotSeq,
                )}
                alt={t('panel.browserView', {
                  title: activeTab.title || t('panel.browserNoTitle'),
                })}
              />
              <div
                className="browser-live"
                role="application"
                tabIndex={0}
                aria-label={t('panel.browserLive')}
                aria-describedby="browser-live-hint"
                onMouseDown={(event) => event.currentTarget.focus()}
                onClick={clickLive}
                onKeyDown={keyDownLive}
              />
            </div>
            <p className="browser-live-hint" id="browser-live-hint">
              {t('panel.browserLiveHint')}
            </p>
          </>
        ) : !activeTab ? (
          <p className="dock-note">{t('panel.browserEmpty')}</p>
        ) : null}
      </div>
    </div>
  );
}
