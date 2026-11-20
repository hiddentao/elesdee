import { useRef, type KeyboardEvent } from 'react';
import type { Project, Session } from '../api/types';
import { useLanguage } from '../i18n/LanguageProvider';
import { statusShape, statusText, statusTone } from '../state/sessionStatus';
import { FolderIcon, PlusIcon } from './icons';

interface TabStripProps {
  project: Project;
  sessions: Session[];
  activeSessionId: string | null;
  onSelectSession: (session: Session) => void;
  onSessionMenu: (
    session: Session,
    point: { x: number; y: number },
    trigger: HTMLElement,
  ) => void;
  onNewSession: () => void;
}

export function TabStrip({
  project,
  sessions,
  activeSessionId,
  onSelectSession,
  onSessionMenu,
  onNewSession,
}: TabStripProps) {
  const { t } = useLanguage();
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  function activateAt(index: number) {
    const session = sessions[index];
    if (!session) {
      return;
    }

    onSelectSession(session);
    tabRefs.current[index]?.focus();
  }

  function handleKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) {
    let nextIndex: number | null = null;

    switch (event.key) {
      case 'ArrowRight':
        nextIndex = (index + 1) % sessions.length;
        break;
      case 'ArrowLeft':
        nextIndex = (index - 1 + sessions.length) % sessions.length;
        break;
      case 'Home':
        nextIndex = 0;
        break;
      case 'End':
        nextIndex = sessions.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    activateAt(nextIndex);
  }

  return (
    <div className="tabbar">
      <div className="tabbar-project">
        <FolderIcon />
        <b>{project.name}</b>
      </div>

      {sessions.length === 0 ? (
        <p className="tabs-empty">{t('session.openFromSidebar')}</p>
      ) : (
        <div
          className="tabs"
          role="tablist"
          aria-label={t('session.projectSessions', { name: project.name })}
        >
          {sessions.map((session, index) => {
            const isActive = session.id === activeSessionId;
            const isTabStop = isActive || (activeSessionId === null && index === 0);
            const tone = statusTone(session);

            return (
              <button
                key={session.id}
                ref={(element) => {
                  tabRefs.current[index] = element;
                }}
                type="button"
                role="tab"
                id={`tab-${session.id}`}
                aria-selected={isActive}
                aria-controls="session-panel"
                aria-label={t('session.tabLabel', {
                  name: session.name,
                  status: statusText(session, t),
                })}
                tabIndex={isTabStop ? 0 : -1}
                className={isActive ? 'tab is-active' : 'tab'}
                title={session.name}
                onClick={() => onSelectSession(session)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  onSessionMenu(
                    session,
                    { x: event.clientX, y: event.clientY },
                    event.currentTarget,
                  );
                }}
                onKeyDown={(event) => handleKeyDown(event, index)}
              >
                <span
                  className="status-marker tab-marker"
                  data-status={tone}
                  data-shape={statusShape(tone)}
                  aria-hidden="true"
                />
                <span className="tab-name">{session.name}</span>
              </button>
            );
          })}
        </div>
      )}

      <button
        type="button"
        className="tab-add"
        aria-label={t('session.newSession')}
        title={t('session.newSession')}
        onClick={onNewSession}
      >
        <PlusIcon />
      </button>
    </div>
  );
}
