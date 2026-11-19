import { useRef, type KeyboardEvent } from 'react';
import type { Project, Session } from '../api/types';
import { statusText, statusTone } from '../state/sessionStatus';
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
        <p className="tabs-empty">Open a session from the sidebar.</p>
      ) : (
        <div className="tabs" role="tablist" aria-label={`${project.name} sessions`}>
          {sessions.map((session, index) => {
            const isActive = session.id === activeSessionId;
            const isTabStop = isActive || (activeSessionId === null && index === 0);

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
                aria-label={`${session.name}, ${statusText(session)}`}
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
                  data-status={statusTone(session)}
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
        aria-label="New session"
        title="New session"
        onClick={onNewSession}
      >
        <PlusIcon />
      </button>
    </div>
  );
}
