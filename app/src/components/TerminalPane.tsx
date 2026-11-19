import { useEffect, useRef, useState } from 'react';
import { FitAddon } from '@xterm/addon-fit';
import { Terminal, type ITheme } from '@xterm/xterm';
import { resizeSession, sendInput } from '../api/client';
import { subscribeOutput } from '../api/events';
import type { Session } from '../api/types';

const ANSI = {
  black: '#1c1e1f',
  red: '#d89a8a',
  green: '#9bc49a',
  yellow: '#d6b36a',
  blue: '#9db8d6',
  magenta: '#c7a3c9',
  cyan: '#8fc4c0',
  white: '#e7e4dd',
  brightBlack: '#9a9387',
  brightRed: '#e8b3a6',
  brightGreen: '#b6d6b4',
  brightYellow: '#e6c98c',
  brightBlue: '#b8cbe0',
  brightMagenta: '#d8bcd9',
  brightCyan: '#a9d6d2',
  brightWhite: '#f4f1ea',
} as const;

// xterm cannot read CSS variables, so these palettes mirror the --term-* tokens.
const LIGHT_THEME: ITheme = {
  background: '#1c1e1f',
  foreground: '#e7e4dd',
  cursor: '#c58a7e',
  cursorAccent: '#1c1e1f',
  selectionBackground: 'rgba(197, 138, 126, 0.3)',
  ...ANSI,
};

const DARK_THEME: ITheme = {
  background: '#121110',
  foreground: '#e7e4dd',
  cursor: '#d59a87',
  cursorAccent: '#121110',
  selectionBackground: 'rgba(213, 154, 135, 0.3)',
  ...ANSI,
};

const GAP_MARKER = '\x1b[33m[Earlier output is no longer available.]\x1b[0m';

interface TerminalPaneProps {
  session: Session;
}

export function TerminalPane({ session }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const lastSeqRef = useRef(0);
  const closedRef = useRef(session.status === 'closed');
  closedRef.current = session.status === 'closed';
  const [connectionError, setConnectionError] = useState(false);
  const [reconnected, setReconnected] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const hadErrorRef = useRef(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    lastSeqRef.current = 0;

    const media =
      typeof window.matchMedia === 'function'
        ? window.matchMedia('(prefers-color-scheme: dark)')
        : null;

    const term = new Terminal({
      scrollback: 5000,
      cursorBlink: true,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
      fontSize: 13,
      screenReaderMode: true,
      disableStdin: closedRef.current,
      theme: media?.matches ? DARK_THEME : LIGHT_THEME,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    termRef.current = term;

    const dataDisposable = term.onData((data) => {
      if (closedRef.current) {
        return;
      }
      void sendInput(session.id, data).catch(() => {
        // The output stream reports a lost connection.
      });
    });

    let lastCols = 0;
    let lastRows = 0;

    const fit = () => {
      try {
        fitAddon.fit();
      } catch {
        return;
      }

      if (term.cols === lastCols && term.rows === lastRows) {
        return;
      }

      lastCols = term.cols;
      lastRows = term.rows;

      if (!closedRef.current) {
        void resizeSession(session.id, term.cols, term.rows).catch(() => {
          // A session that just closed cannot be resized.
        });
      }
    };

    fit();

    const observer = new ResizeObserver(() => fit());
    observer.observe(container);

    const onThemeChange = (event: MediaQueryListEvent) => {
      term.options.theme = event.matches ? DARK_THEME : LIGHT_THEME;
    };
    media?.addEventListener('change', onThemeChange);

    return () => {
      media?.removeEventListener('change', onThemeChange);
      observer.disconnect();
      dataDisposable.dispose();
      term.dispose();
      termRef.current = null;
    };
  }, [session.id]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) {
      return;
    }

    setConnectionError(false);

    return subscribeOutput(
      session.id,
      lastSeqRef.current,
      (seq, data, gap) => {
        lastSeqRef.current = seq;
        if (gap) {
          term.writeln(GAP_MARKER);
        }
        term.write(data);
      },
      () => {
        if (!closedRef.current) {
          hadErrorRef.current = true;
          setReconnected(false);
          setConnectionError(true);
        }
      },
      () => {
        if (hadErrorRef.current) {
          hadErrorRef.current = false;
          setReconnected(true);
        }
        setConnectionError(false);
      },
      () => closedRef.current,
    );
  }, [session.id, attempt]);

  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.disableStdin = session.status === 'closed';
    }
    if (session.status === 'closed') {
      setConnectionError(false);
    }
  }, [session.status]);

  return (
    <>
      {connectionError ? (
        <div className="connection-note">
          <span role="status">Output is disconnected.</span>
          <button
            type="button"
            className="link-button"
            onClick={() => setAttempt((count) => count + 1)}
          >
            Retry
          </button>
        </div>
      ) : null}

      {reconnected ? (
        <p className="sr-only" role="status">
          Output reconnected.
        </p>
      ) : null}

      <div className="terminal-shell">
        <div className="terminal-pad">
          <div className="terminal-mount" ref={containerRef} />
        </div>
      </div>
    </>
  );
}
