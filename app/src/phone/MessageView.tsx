import { useEffect, useRef } from 'react';
import type { HistoryItem, Session } from '../api/types';
import { formatNumber, type Language, type Translate } from '../i18n';
import { workLabel } from '../state/sessionStatus';
import { toolStateLabel } from './history';

export type SessionView = 'messages' | 'terminal';

interface SessionContextStripProps {
  session: Session;
  view: SessionView;
  onToggleView: () => void;
  t: Translate;
  language: Language;
}

/**
 * Shows the session state and the context that is still free, with the one
 * control that moves between the message view and the real terminal.
 */
export function SessionContextStrip({
  session,
  view,
  onToggleView,
  t,
  language,
}: SessionContextStripProps) {
  const context =
    session.contextLeft === null || session.contextLeft === undefined
      ? t('phone.contextUnknown')
      : t('phone.contextLeft', {
          percent: formatNumber(language, session.contextLeft),
        });

  return (
    <div className="phone-context">
      <div className="phone-context-facts">
        <span className="phone-context-state">
          {workLabel(session, t) ?? t('status.active')}
        </span>
        <span className="phone-context-item">{context}</span>
        {session.branch ? (
          <span className="phone-context-branch" title={session.branch}>
            {session.branch}
          </span>
        ) : null}
      </div>
      <button
        type="button"
        className="phone-view-toggle"
        aria-label={t('phone.viewLabel')}
        onClick={onToggleView}
      >
        {view === 'messages'
          ? t('phone.viewTerminal')
          : t('phone.viewMessages')}
      </button>
    </div>
  );
}

interface MessageViewProps {
  session: Session;
  items: HistoryItem[];
  loading: boolean;
  expanded: ReadonlySet<string>;
  onToggleTool: (id: string) => void;
  t: Translate;
}

/**
 * Shows the agent conversation: my prompts and the agent's replies in full,
 * with tool calls as one-line rows that open on tap.
 */
export function MessageView({
  session,
  items,
  loading,
  expanded,
  onToggleTool,
  t,
}: MessageViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const node = scrollRef.current;
    if (node) {
      node.scrollTop = node.scrollHeight;
    }
  }, [items]);

  if (loading) {
    return (
      <div className="phone-messages-scroll">
        <p className="phone-empty">{t('phone.messagesLoading')}</p>
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <div className="phone-messages-scroll">
        <p className="phone-empty">{t('phone.messagesEmpty')}</p>
      </div>
    );
  }

  return (
    <div className="phone-messages-scroll" ref={scrollRef}>
      <div
        className="phone-messages"
        role="log"
        aria-label={t('phone.conversation')}
      >
      {items.map((item) => {
        if (item.kind === 'prompt') {
          return (
            <article key={item.id} className="phone-message is-prompt">
              <span className="phone-message-label">{t('phone.you')}</span>
              <p className="phone-message-text">{item.text}</p>
            </article>
          );
        }
        if (item.kind === 'reply') {
          return (
            <article
              key={item.id}
              className="phone-message is-reply"
              data-state={item.state}
            >
              <span className="phone-message-label">
                {session.client ?? t('phone.agent')}
              </span>
              <p className="phone-message-text">
                {item.text}
                {item.state === 'streaming' ? (
                  <span className="phone-message-caret" aria-hidden="true">
                    ▍
                  </span>
                ) : null}
              </p>
            </article>
          );
        }

        const open = expanded.has(item.id);
        const state = toolStateLabel(item.state, t);
        return (
          <div key={item.id} className="phone-tool">
            <button
              type="button"
              className="phone-tool-row"
              data-state={item.state}
              aria-expanded={open}
              onClick={() => onToggleTool(item.id)}
            >
              <span className="phone-tool-name">
                {item.tool ?? t('phone.tool')}
              </span>
              <span className="phone-tool-summary">{item.text}</span>
              {state ? <span className="phone-tool-state">{state}</span> : null}
              <span className="phone-tool-marker" aria-hidden="true">
                {open ? '−' : '+'}
              </span>
            </button>
            {open ? (
              <pre className="phone-tool-detail">{item.text}</pre>
            ) : null}
          </div>
        );
      })}
      {session.workState === 'done' ? (
        <p className="phone-messages-done">{t('status.finished')}</p>
      ) : null}
      </div>
    </div>
  );
}
